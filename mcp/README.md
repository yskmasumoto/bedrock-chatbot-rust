# MCP クレート

Model Context Protocol (MCP) のRust実装クライアント。

## 概要

このクレートは、ローカルで実行されるMCPサーバーとの通信を提供します。

## 機能

- ローカルプロセスとしてのMCPサーバーの起動と接続
- ツール一覧の取得とツールの実行
- リソースの一覧取得と読み込み
- プロンプトの一覧取得

## 使用例

### 基本的な使用方法

```rust
use mcp::McpClient;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // MCPサーバーに接続
    let client = McpClient::new("uvx", vec!["mcp-server-git"]).await?;
    
    // サーバー情報を取得
    if let Some(info) = client.server_info() {
        println!("Connected to: {:?}", info);
    }
    
    // 利用可能なツール一覧を取得
    let tools = client.list_tools().await?;
    for tool in tools {
        println!("Tool: {}", tool.name);
    }
    
    // ツールを実行
    let args = serde_json::json!({
        "repo_path": "."
    });
    let result = client.call_tool(
        "git_status".to_string(),
        args.as_object().cloned()
    ).await?;
    println!("Result: {}", result);
    
    // 接続を切断
    client.disconnect().await?;
    
    Ok(())
}
```

### AgentClientとの統合

```rust
use agent::AgentClient;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // エージェントを初期化
    let mut agent = AgentClient::new(
        "default".to_string(),
        None
    ).await?;
    
    // MCPサーバーに接続
    agent.connect_mcp("uvx", vec!["mcp-server-git"]).await?;
    
    // MCPツール一覧を取得
    let tools = agent.list_mcp_tools().await?;
    println!("Available MCP tools: {:?}", tools);
    
    // 使用後は切断
    agent.disconnect_mcp().await?;
    
    Ok(())
}
```

## 依存関係

- `rmcp`: Model Context Protocol のRust SDK（v0.11.0）
- `tokio`: 非同期ランタイム
- `thiserror`: エラー処理
- `serde_json`: JSON シリアライゼーション

## エラーハンドリング

このクレートは `thiserror` を使用したカスタムエラー型 `McpError` を提供します：

```rust
pub enum McpError {
    TransportError(String),
    ProtocolError(Box<RmcpError>),  // Note: Boxed to reduce enum size
    ServiceError(ServiceError),
    InitializationError(String),
    TaskJoinError(tokio::task::JoinError),
    ToolNotFound(String),
    ResourceNotFound(String),
    InvalidArguments(String),
    ConnectionError(String),
    SerializationError(serde_json::Error),
}
```
    ConnectionError(String),
}
```

## ライセンス

このプロジェクトは実験的なものです。
