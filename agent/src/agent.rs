use aws_config::meta::region::RegionProviderChain;
use aws_config::{self, BehaviorVersion};
use aws_sdk_bedrockruntime::Client;
use aws_sdk_bedrockruntime::operation::converse_stream::ConverseStreamOutput as ConverseStreamResponse;
use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, Message};

/// 使用するモデルID
const MODEL_ID: &str = "anthropic.claude-3-5-sonnet-20240620-v1:0";

/// Agent クライアント構造体
///
/// AWS Bedrock との通信と会話履歴を管理する純粋なビジネスロジック層。
/// UI/UX に関する処理は含まず、再利用可能な形で提供される。
pub struct AgentClient {
    client: Client,
    messages: Vec<Message>,
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
    pub async fn new(
        profile: String,
        region: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
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
        })
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
    pub async fn send_message(
        &mut self,
        user_input: &str,
    ) -> Result<ConverseStreamResponse, Box<dyn std::error::Error>> {
        let user_message = Message::builder()
            .role(ConversationRole::User)
            .content(ContentBlock::Text(user_input.to_string()))
            .build()
            .map_err(|e| format!("Failed to build message: {}", e))?;

        self.messages.push(user_message);

        let response = self
            .client
            .converse_stream()
            .model_id(MODEL_ID)
            .set_messages(Some(self.messages.clone()))
            .send()
            .await?;

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
    pub fn add_assistant_message(
        &mut self,
        response_text: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let assistant_message = Message::builder()
            .role(ConversationRole::Assistant)
            .content(ContentBlock::Text(response_text))
            .build()
            .map_err(|e| format!("Failed to build message: {}", e))?;

        self.messages.push(assistant_message);
        Ok(())
    }

    /// 最後に追加されたユーザーメッセージを履歴から削除する
    ///
    /// エラー発生時などに使用し、メッセージ履歴の整合性を保つ。
    pub fn rollback_last_user_message(&mut self) {
        if let Some(last_message) = self.messages.last()
            && matches!(last_message.role, ConversationRole::User)
        {
            self.messages.pop();
        }
    }
}
