# bedrock-agent-rust

Amazon Bedrockとaws_sdk_rustを利用した対話型AIチャットボット + Model Context Protocol (MCP) サーバー統合

## 概要

このプロジェクトは、AWS Bedrockを使用したAIチャットボットに、ローカルMCPサーバーとの統合機能を提供します。

## プロジェクト構成

このプロジェクトはCargo Workspaceとして構成されており、以下のクレートで構成されています：

- **`cli`**: ユーザーインターフェース（コマンドライン）
  - ユーザー入力の受け付け
  - 会話の表示
  - コマンドライン引数の解析
  
- **`agent`**: ビジネスロジック層
  - AWS Bedrockとの通信
  - 会話履歴の管理
  - MCPクライアントとの統合
  
- **`mcp`**: MCP通信ライブラリ
  - ローカルMCPサーバーへの接続
  - ツール・リソース・プロンプトの取得と実行

## 機能

- AWS Bedrock Claude 3.5 Sonnetとの対話型チャット
- ストリーミングレスポンス対応
- 会話履歴の管理
- Model Context Protocol (MCP) サーバー統合
  - ローカルMCPサーバーへの接続
  - ツール一覧の取得と実行
  - リソースの読み込み

## 使用方法

### 基本的なチャットボット

```bash
cargo run --bin agent-cli -- run --aws-profile your-profile-name
```

### MCPサーバーとの統合（コード例）

```rust
use agent::AgentClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // エージェントを初期化
    let mut agent = AgentClient::new("default".to_string(), None).await?;
    
    // MCPサーバーに接続（例: Git操作サーバー）
    agent.connect_mcp("uvx", vec!["mcp-server-git"]).await?;
    
    // 利用可能なツールを確認
    let tools = agent.list_mcp_tools().await?;
    println!("Available tools: {:?}", tools);
    
    Ok(())
}
```

詳細については、[mcp/README.md](mcp/README.md) を参照してください。

## 技術スタック

- **言語**: Rust (edition 2024)
- **非同期ランタイム**: Tokio
- **AWS SDK**: aws-sdk-bedrockruntime
- **MCP SDK**: rmcp (Model Context Protocol Rust SDK)
- **CLI**: clap, rustyline
- **エラーハンドリング**: thiserror (ライブラリ), anyhow (アプリケーション)

## 開発

### ビルド

```bash
cargo build
```

### テスト

```bash
# 全てのテストを実行（実際のMCPサーバーは不要）
SKIP_MCP_INTEGRATION_TEST=1 cargo test

# 実際のMCPサーバーを使用した完全なテスト（uvxなどが必要）
cargo test -- --ignored
```

#### テスト構成

- **mcp/tests/integration_test.rs**: MCPクライアントの統合テスト
- **agent/tests/mcp_integration_test.rs**: AgentとMCPの統合テスト
- **mcp/tests/fixtures/mock_mcp_server.sh**: テスト用モックMCPサーバー

詳細は [mcp/README.md](mcp/README.md) を参照してください。

## アーキテクチャ

プロジェクトは責務分離の原則に従って設計されています：

- **UI層 (`cli`)**: ユーザーとのインタラクションのみを担当
- **ビジネスロジック層 (`agent`)**: AI通信とMCP統合を担当
- **ライブラリ層 (`mcp`)**: MCP通信の低レベル実装

エラーハンドリングについては：
- ライブラリクレート (`agent`, `mcp`) は `thiserror` を使用
- アプリケーションクレート (`cli`) は `anyhow` を使用

## ライセンス

実験的プロジェクト

