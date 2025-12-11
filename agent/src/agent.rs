use aws_config::meta::region::RegionProviderChain;
use aws_config::{self, BehaviorVersion};
use aws_sdk_bedrockruntime::Client;
use aws_sdk_bedrockruntime::operation::converse_stream::ConverseStreamOutput as ConverseStreamResponse;
use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, Message};
use mcp::McpClient;

/// 使用するモデルID
const MODEL_ID: &str = "anthropic.claude-3-5-sonnet-20240620-v1:0";

/// AgentClientのエラー型
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("AWS Bedrock API error: {0}")]
    BedrockError(String),

    #[error("Message building error: {0}")]
    MessageBuildError(String),

    #[error("AWS SDK error: {0}")]
    AwsSdkError(String),

    #[error("MCP error: {0}")]
    McpError(#[from] mcp::McpError),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Agent クライアント構造体
///
/// AWS Bedrock との通信と会話履歴を管理する純粋なビジネスロジック層。
/// UI/UX に関する処理は含まず、再利用可能な形で提供される。
pub struct AgentClient {
    client: Client,
    messages: Vec<Message>,
    mcp_client: Option<McpClient>,
}

impl Drop for AgentClient {
    fn drop(&mut self) {
        // MCPクライアントが接続されている場合は、適切にクリーンアップする
        // disconnect()は非同期メソッドだが、Dropは同期的なため、
        // ここでは接続が残る可能性があることをログに記録する
        if self.mcp_client.is_some() {
            eprintln!(
                "Warning: AgentClient dropped with active MCP connection. Consider calling disconnect_mcp() before dropping."
            );
        }
    }
}

impl AgentClient {
    /// 新しい AgentClient を作成する
    ///
    /// # Arguments
    /// * `profile` - 使用する AWS プロファイル名
    /// * `region` - リージョン（オプション）。指定しない場合はデフォルトプロファイルの設定またはus-east-1を使用
    ///
    /// # Returns
    /// 初期化された `AgentClient` インスタンス
    pub async fn new(profile: String, region: Option<String>) -> Result<Self, AgentError> {
        let region_provider = RegionProviderChain::first_try(region.map(aws_config::Region::new))
            .or_default_provider()
            .or_else(aws_config::Region::new("us-east-1"));

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(region_provider)
            .profile_name(&profile)
            .load()
            .await;

        let client = Client::new(&config);

        Ok(Self {
            client,
            messages: Vec::new(),
            mcp_client: None,
        })
    }

    /// MCPサーバーに接続する
    ///
    /// # Arguments
    /// * `command` - 起動するコマンド名（例: "uvx", "npx"）
    /// * `args` - コマンド引数のベクター（例: vec!["mcp-server-git"]）
    ///
    /// # Returns
    /// * `Ok(())` - 接続に成功した場合
    /// * `Err(AgentError)` - 接続に失敗した場合
    ///
    /// # Note
    /// 既に接続されている場合は、古い接続を切断してから新しい接続を確立します。
    pub async fn connect_mcp(&mut self, command: &str, args: Vec<&str>) -> Result<(), AgentError> {
        // 既存の接続があれば切断
        if let Some(existing_client) = self.mcp_client.take() {
            let _ = existing_client.disconnect().await;
        }

        let mcp_client = McpClient::new(command, args).await?;
        self.mcp_client = Some(mcp_client);
        Ok(())
    }

    /// MCPサーバーから切断する
    ///
    /// # Returns
    /// * `Ok(())` - 切断に成功した場合
    /// * `Err(AgentError)` - 切断に失敗した場合、または接続されていない場合
    pub async fn disconnect_mcp(&mut self) -> Result<(), AgentError> {
        if let Some(client) = self.mcp_client.take() {
            client.disconnect().await?;
            Ok(())
        } else {
            Err(AgentError::ConfigError(
                "MCP client is not connected".to_string(),
            ))
        }
    }

    /// MCPサーバーが接続されているかを確認する
    pub fn is_mcp_connected(&self) -> bool {
        self.mcp_client.is_some()
    }

    /// MCPサーバーから利用可能なツール一覧を取得する
    ///
    /// # Returns
    /// * `Ok(Vec<mcp::Tool>)` - ツール一覧
    /// * `Err(AgentError)` - MCPが接続されていない、または取得に失敗した場合
    pub async fn list_mcp_tools(&self) -> Result<Vec<mcp::Tool>, AgentError> {
        match &self.mcp_client {
            Some(client) => Ok(client.list_tools().await?),
            None => Err(AgentError::ConfigError(
                "MCP client is not connected".to_string(),
            )),
        }
    }

    /// 使用しているモデルIDを取得する
    pub fn model_id(&self) -> &str {
        MODEL_ID
    }

    /// ユーザーのメッセージを送信し、レスポンスのストリームを返す
    ///
    /// # Arguments
    /// * `user_input` - ユーザーの入力テキスト
    ///
    /// # Returns
    /// * `Ok(ConverseStreamResponse)` - ストリーミングレスポンス
    /// * `Err` - エラーが発生した場合
    ///
    /// # Note
    /// この関数はメッセージ履歴を更新します。エラーが発生した場合、
    /// 呼び出し元は `rollback_last_user_message()` を呼び出して履歴を元に戻すことができます。
    ///
    /// # Performance Note
    /// この関数は会話履歴全体をクローンします。AWS SDK APIが所有権を要求するため必要です。
    /// 長い会話では、パフォーマンスへの影響が発生する可能性があります。
    pub async fn send_message(
        &mut self,
        user_input: &str,
    ) -> Result<ConverseStreamResponse, AgentError> {
        let user_message = Message::builder()
            .role(ConversationRole::User)
            .content(ContentBlock::Text(user_input.to_string()))
            .build()
            .map_err(|e| {
                AgentError::MessageBuildError(format!("Failed to build message: {}", e))
            })?;

        self.messages.push(user_message);

        let response = self
            .client
            .converse_stream()
            .model_id(MODEL_ID)
            .set_messages(Some(self.messages.clone()))
            .send()
            .await
            .map_err(|e| AgentError::AwsSdkError(e.to_string()))?;

        Ok(response)
    }

    /// アシスタントのメッセージを会話履歴に追加する
    ///
    /// # Arguments
    /// * `response_text` - アシスタントの完全なレスポンステキスト
    ///
    /// # Returns
    /// * `Ok(())` - 成功
    /// * `Err` - メッセージ構築に失敗した場合
    pub fn add_assistant_message(&mut self, response_text: String) -> Result<(), AgentError> {
        let assistant_message = Message::builder()
            .role(ConversationRole::Assistant)
            .content(ContentBlock::Text(response_text))
            .build()
            .map_err(|e| {
                AgentError::MessageBuildError(format!("Failed to build message: {}", e))
            })?;

        self.messages.push(assistant_message);
        Ok(())
    }

    /// 最後に追加されたユーザーメッセージを履歴から削除する
    ///
    /// エラー発生時などに使用し、メッセージ履歴の整合性を保つ。
    ///
    /// # Returns
    /// * `true` - ユーザーメッセージが削除された
    /// * `false` - 最後のメッセージがユーザーメッセージでないため、何も削除されなかった
    pub fn rollback_last_user_message(&mut self) -> bool {
        if let Some(last_message) = self.messages.last()
            && matches!(last_message.role, ConversationRole::User)
        {
            self.messages.pop();
            true
        } else {
            false
        }
    }
}
