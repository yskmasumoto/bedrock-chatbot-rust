use aws_config::meta::region::RegionProviderChain;
use aws_config::{self, BehaviorVersion};
use aws_sdk_bedrockruntime::Client;
use aws_sdk_bedrockruntime::operation::converse_stream::ConverseStreamOutput as ConverseStreamResponse;
use aws_sdk_bedrockruntime::types::{
    ContentBlock, ConversationRole, Message, Tool, ToolConfiguration, ToolInputSchema,
    ToolSpecification,
};
use aws_smithy_types::Document;
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

    /// MCPツールを実行する
    ///
    /// # Arguments
    /// * `tool_name` - 実行するツール名
    /// * `arguments` - ツールに渡す引数（JSON形式）
    ///
    /// # Returns
    /// * `Ok(serde_json::Value)` - ツールの実行結果
    /// * `Err(AgentError)` - MCPが接続されていない、または実行に失敗した場合
    pub async fn call_mcp_tool(
        &self,
        tool_name: String,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<serde_json::Value, AgentError> {
        match &self.mcp_client {
            Some(client) => Ok(client.call_tool(tool_name, arguments).await?),
            None => Err(AgentError::ConfigError(
                "MCP client is not connected".to_string(),
            )),
        }
    }

    /// MCPツールをBedrockツール形式に変換する
    ///
    /// # Returns
    /// * `Vec<Tool>` - Bedrock形式のツール定義リスト
    ///
    /// # Note
    /// AWS SDK bedrockruntime v1.120.0 includes full Converse API tool support.
    async fn convert_mcp_tools_to_bedrock(&self) -> Result<Vec<Tool>, AgentError> {
        let mcp_tools = self.list_mcp_tools().await?;
        let mut bedrock_tools = Vec::new();

        for mcp_tool in mcp_tools {
            // MCPツールのスキーマをJSON Valueに変換
            let input_schema_json = serde_json::to_value(&mcp_tool.input_schema).map_err(|e| {
                AgentError::MessageBuildError(format!("Failed to serialize tool schema: {}", e))
            })?;

            // input_schemaがnullの場合はスキップ（Bedrockはnullを受け付けない）
            if input_schema_json.is_null() {
                eprintln!(
                    "[Warning] Skipping tool '{}' due to missing input_schema",
                    mcp_tool.name
                );
                continue;
            }

            // JSON ValueをAWS Smithy Documentに変換
            let schema_document = json_to_document(input_schema_json)?;

            // ToolSpecificationを構築
            let tool_spec = ToolSpecification::builder()
                .name(mcp_tool.name.clone())
                .description(mcp_tool.description.clone().unwrap_or_default())
                .input_schema(ToolInputSchema::Json(schema_document))
                .build()
                .map_err(|e| {
                    AgentError::MessageBuildError(format!("Failed to build tool spec: {}", e))
                })?;

            bedrock_tools.push(Tool::ToolSpec(tool_spec));
        }

        Ok(bedrock_tools)
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
    /// MCPサーバーに接続されている場合、ツール定義を自動的にBedrockに送信します。
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

        let mut request = self
            .client
            .converse_stream()
            .model_id(MODEL_ID)
            .set_messages(Some(self.messages.clone()));

        // MCP接続時は自動的にツール定義を送信
        if self.is_mcp_connected() {
            match self.convert_mcp_tools_to_bedrock().await {
                Ok(tools) if !tools.is_empty() => {
                    let tool_config = ToolConfiguration::builder()
                        .set_tools(Some(tools))
                        .build()
                        .map_err(|e| {
                            AgentError::MessageBuildError(format!(
                                "Failed to build tool config: {}",
                                e
                            ))
                        })?;
                    request = request.tool_config(tool_config);
                }
                Ok(_) => {
                    // ツールが空の場合は何もしない
                }
                Err(e) => {
                    eprintln!("Warning: Failed to convert MCP tools: {}", e);
                    // ツール変換に失敗しても会話は続行
                }
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| AgentError::AwsSdkError(e.to_string()))?;

        Ok(response)
    }

    /// アシスタントのメッセージを会話履歴に追加する
    ///
    /// # Arguments
    /// * `content_blocks` - アシスタントのコンテンツブロック（テキストやツール使用を含む）
    ///
    /// # Returns
    /// * `Ok(())` - 成功
    /// * `Err` - メッセージ構築に失敗した場合
    pub fn add_assistant_message_with_blocks(
        &mut self,
        content_blocks: Vec<ContentBlock>,
    ) -> Result<(), AgentError> {
        let mut builder = Message::builder().role(ConversationRole::Assistant);

        for block in content_blocks {
            builder = builder.content(block);
        }

        let assistant_message = builder.build().map_err(|e| {
            AgentError::MessageBuildError(format!("Failed to build message: {}", e))
        })?;

        self.messages.push(assistant_message);
        Ok(())
    }

    /// ツール実行結果を会話履歴に追加する
    ///
    /// # Arguments
    /// * `tool_use_id` - ツール使用ID
    /// * `tool_result` - ツールの実行結果（JSON形式）
    ///
    /// # Returns
    /// * `Ok(())` - 成功
    /// * `Err` - メッセージ構築に失敗した場合
    ///
    /// # Note
    /// Tool result handling requires newer AWS SDK version with proper Document conversion.
    /// Currently using text-based result for compatibility.
    pub fn add_tool_result(
        &mut self,
        tool_use_id: String,
        tool_result: serde_json::Value,
    ) -> Result<(), AgentError> {
        use aws_sdk_bedrockruntime::types::{ToolResultBlock, ToolResultContentBlock};

        // Convert JSON to string for now since Document conversion is not straightforward
        let result_text = serde_json::to_string(&tool_result).map_err(|e| {
            AgentError::MessageBuildError(format!("Failed to serialize tool result: {}", e))
        })?;

        let result_content = ToolResultContentBlock::Text(result_text);

        let tool_result_block = ToolResultBlock::builder()
            .tool_use_id(tool_use_id)
            .content(result_content)
            .build()
            .map_err(|e| {
                AgentError::MessageBuildError(format!("Failed to build tool result: {}", e))
            })?;

        let user_message = Message::builder()
            .role(ConversationRole::User)
            .content(ContentBlock::ToolResult(tool_result_block))
            .build()
            .map_err(|e| {
                AgentError::MessageBuildError(format!("Failed to build message: {}", e))
            })?;

        self.messages.push(user_message);
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

    /// serde_json::Value を aws_smithy_types::Document に変換する
    ///
    /// # Arguments
    /// * `value` - 変換元のJSON Value
    ///
    /// # Returns
    /// * `Ok(Document)` - 変換されたDocument
    /// * `Err(AgentError)` - 変換に失敗した場合
    pub fn json_to_document(&self, value: serde_json::Value) -> Result<Document, AgentError> {
        json_to_document(value)
    }

    /// aws_smithy_types::Document を serde_json::Value に変換する
    ///
    /// # Arguments
    /// * `doc` - 変換元のDocument
    ///
    /// # Returns
    /// * `Ok(serde_json::Value)` - 変換されたJSON Value
    /// * `Err(AgentError)` - 変換に失敗した場合
    pub fn document_to_json(&self, doc: Document) -> Result<serde_json::Value, AgentError> {
        document_to_json(doc)
    }
}

/// serde_json::Value を aws_smithy_types::Document に変換する
///
/// # Arguments
/// * `value` - 変換元のJSON Value
///
/// # Returns
/// * `Ok(Document)` - 変換されたDocument
/// * `Err(AgentError)` - 変換に失敗した場合
pub fn json_to_document(value: serde_json::Value) -> Result<Document, AgentError> {
    match value {
        serde_json::Value::Null => Ok(Document::Null),
        serde_json::Value::Bool(b) => Ok(Document::Bool(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= 0 {
                    Ok(Document::Number(aws_smithy_types::Number::PosInt(i as u64)))
                } else {
                    Ok(Document::Number(aws_smithy_types::Number::NegInt(i)))
                }
            } else if let Some(f) = n.as_f64() {
                Ok(Document::Number(aws_smithy_types::Number::Float(f)))
            } else {
                Err(AgentError::MessageBuildError(
                    "Invalid number format".to_string(),
                ))
            }
        }
        serde_json::Value::String(s) => Ok(Document::String(s)),
        serde_json::Value::Array(arr) => {
            let docs: Result<Vec<Document>, AgentError> =
                arr.into_iter().map(json_to_document).collect();
            Ok(Document::Array(docs?))
        }
        serde_json::Value::Object(obj) => {
            let map: Result<std::collections::HashMap<String, Document>, AgentError> = obj
                .into_iter()
                .map(|(k, v)| json_to_document(v).map(|d| (k, d)))
                .collect();
            Ok(Document::Object(map?))
        }
    }
}

/// aws_smithy_types::Document を serde_json::Value に変換する
///
/// # Arguments
/// * `doc` - 変換元のDocument
///
/// # Returns
/// * `Ok(serde_json::Value)` - 変換されたJSON Value
/// * `Err(AgentError)` - 変換に失敗した場合
pub fn document_to_json(doc: Document) -> Result<serde_json::Value, AgentError> {
    match doc {
        Document::Null => Ok(serde_json::Value::Null),
        Document::Bool(b) => Ok(serde_json::Value::Bool(b)),
        Document::Number(n) => match n {
            aws_smithy_types::Number::PosInt(i) => Ok(serde_json::json!(i)),
            aws_smithy_types::Number::NegInt(i) => Ok(serde_json::json!(i)),
            aws_smithy_types::Number::Float(f) => Ok(serde_json::json!(f)),
        },
        Document::String(s) => Ok(serde_json::Value::String(s)),
        Document::Array(arr) => {
            let values: Result<Vec<serde_json::Value>, AgentError> =
                arr.into_iter().map(document_to_json).collect();
            Ok(serde_json::Value::Array(values?))
        }
        Document::Object(obj) => {
            let map: Result<serde_json::Map<String, serde_json::Value>, AgentError> = obj
                .into_iter()
                .map(|(k, v)| document_to_json(v).map(|j| (k, j)))
                .collect();
            Ok(serde_json::Value::Object(map?))
        }
    }
}
