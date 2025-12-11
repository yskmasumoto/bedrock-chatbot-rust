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

## テスト

### 基本的なテストの実行

```bash
# 全てのテストを実行（実際のMCPサーバーは不要）
SKIP_MCP_INTEGRATION_TEST=1 cargo test

# mcpクレートのテストのみ実行
SKIP_MCP_INTEGRATION_TEST=1 cargo test --package mcp

# 特定の統合テストを実行
SKIP_MCP_INTEGRATION_TEST=1 cargo test --test integration_test
```

### 実際のMCPサーバーを使用したテスト

実際のMCPサーバー（例: `uvx mcp-server-git`）がインストールされている場合、
より完全なテストを実行できます：

```bash
# ignoredテストを含めて実行
cargo test -- --ignored

# 特定の実サーバーテストを実行
cargo test --test integration_test real_server_tests::test_with_real_mcp_server -- --ignored
```

### テスト構成

```
mcp/
├── tests/
│   ├── integration_test.rs       # 統合テスト
│   └── fixtures/                 # テスト用のフィクスチャ
│       └── mock_mcp_server.sh    # モックMCPサーバー

agent/
└── tests/
    └── mcp_integration_test.rs   # MCPとagentの統合テスト
```

## ライセンス

このプロジェクトは実験的なものです。
