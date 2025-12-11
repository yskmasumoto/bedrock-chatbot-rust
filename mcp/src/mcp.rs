use rmcp::{
    model::{CallToolRequestParam, Resource, ServerInfo, Tool},
    service::{RoleClient, RunningService, ServiceError, ServiceExt},
    transport::{ConfigureCommandExt, TokioChildProcess},
    RmcpError,
};
use serde_json::Value;
use tokio::process::Command;

/// MCPクライアントのエラー型
#[derive(thiserror::Error, Debug)]
pub enum McpError {
    #[error("MCP transport error: {0}")]
    TransportError(String),

    #[error("MCP protocol error: {0}")]
    ProtocolError(#[from] RmcpError),

    #[error("MCP service error: {0}")]
    ServiceError(#[from] ServiceError),

    #[error("MCP client initialization error: {0}")]
    InitializationError(String),

    #[error("Task join error: {0}")]
    TaskJoinError(#[from] tokio::task::JoinError),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Server connection error: {0}")]
    ConnectionError(String),
}

/// ローカルMCPサーバーとの通信を管理するクライアント
///
/// このクライアントは以下の機能を提供します：
/// - ローカルプロセスとしてのMCPサーバーの起動と接続
/// - リソース情報の取得
/// - ツール情報の取得と実行
pub struct McpClient {
    /// MCP RPC サービスクライアント
    client: RunningService<RoleClient, ()>,
}

impl McpClient {
    /// 新しい MCP クライアントを作成し、ローカルサーバーに接続する
    ///
    /// # Arguments
    /// * `command` - 起動するコマンド名（例: "uvx", "npx"）
    /// * `args` - コマンド引数のベクター（例: vec!["mcp-server-git"]）
    ///
    /// # Returns
    /// * `Ok(McpClient)` - 接続に成功した場合
    /// * `Err(McpError)` - 接続に失敗した場合
    ///
    /// # Examples
    /// ```no_run
    /// # use mcp::McpClient;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = McpClient::new("uvx", vec!["mcp-server-git"]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(command: &str, args: Vec<&str>) -> Result<Self, McpError> {
        let transport = TokioChildProcess::new(Command::new(command).configure(|cmd| {
            for arg in args {
                cmd.arg(arg);
            }
        }))
        .map_err(|e| McpError::TransportError(e.to_string()))?;

        let client = ()
            .serve(transport)
            .await
            .map_err(|e| McpError::InitializationError(e.to_string()))?;

        Ok(Self { client })
    }

    /// サーバーの情報を取得する
    ///
    /// # Returns
    /// サーバーの基本情報（名前、バージョン等）。情報が利用できない場合は None
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.client.peer_info()
    }

    /// 利用可能なツールの一覧を取得する
    ///
    /// # Returns
    /// * `Ok(Vec<Tool>)` - ツール一覧
    /// * `Err(McpError)` - 取得に失敗した場合
    pub async fn list_tools(&self) -> Result<Vec<Tool>, McpError> {
        let response = self.client.list_tools(Default::default()).await?;
        Ok(response.tools)
    }

    /// 指定されたツールを実行する
    ///
    /// # Arguments
    /// * `tool_name` - 実行するツール名
    /// * `arguments` - ツールに渡す引数（JSON形式）
    ///
    /// # Returns
    /// * `Ok(Value)` - ツールの実行結果
    /// * `Err(McpError)` - 実行に失敗した場合
    pub async fn call_tool(
        &self,
        tool_name: String,
        arguments: Option<serde_json::Map<String, Value>>,
    ) -> Result<Value, McpError> {
        let result = self
            .client
            .call_tool(CallToolRequestParam {
                name: tool_name.into(),
                arguments,
            })
            .await?;

        // 結果をJSON形式で返す
        Ok(serde_json::to_value(&result).unwrap_or(Value::Null))
    }

    /// 利用可能なリソースの一覧を取得する
    ///
    /// # Returns
    /// * `Ok(Vec<Resource>)` - リソース一覧
    /// * `Err(McpError)` - 取得に失敗した場合
    pub async fn list_resources(&self) -> Result<Vec<Resource>, McpError> {
        let response = self.client.list_resources(Default::default()).await?;
        Ok(response.resources)
    }

    /// 指定されたURIのリソースを読み込む
    ///
    /// # Arguments
    /// * `uri` - リソースのURI
    ///
    /// # Returns
    /// * `Ok(Value)` - リソースの内容
    /// * `Err(McpError)` - 読み込みに失敗した場合
    pub async fn read_resource(&self, uri: String) -> Result<Value, McpError> {
        let result = self
            .client
            .read_resource(rmcp::model::ReadResourceRequestParam {
                uri: uri.into(),
            })
            .await?;

        // 結果をJSON形式で返す
        Ok(serde_json::to_value(&result).unwrap_or(Value::Null))
    }

    /// 利用可能なプロンプトの一覧を取得する
    ///
    /// # Returns
    /// * `Ok(Vec<Prompt>)` - プロンプト一覧
    /// * `Err(McpError)` - 取得に失敗した場合
    pub async fn list_prompts(&self) -> Result<Vec<rmcp::model::Prompt>, McpError> {
        let response = self.client.list_prompts(Default::default()).await?;
        Ok(response.prompts)
    }

    /// MCPサーバーとの接続を切断する
    ///
    /// # Returns
    /// * `Ok(())` - 切断に成功した場合
    /// * `Err(McpError)` - 切断に失敗した場合
    pub async fn disconnect(self) -> Result<(), McpError> {
        self.client.cancel().await?;
        Ok(())
    }
}
