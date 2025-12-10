---
applyTo: "**/*.rs"
---

# Rust Project Review Instructions

使用言語: 日本語

## 1. アーキテクチャと責務の分離 (Architecture)

**原則**: Cargo Workspace機能を利用し、`cli` (Interface) と `agent` (Logic) を明確に分離する。

**ルール**:
- **`cli` crate**:
  - ユーザー入力の受け付け (`rustyline`)、コマンドライン引数の解析 (`clap`)、結果の表示のみを担当する。
  - 複雑な条件分岐やステート管理は `agent` 側に委譲する。
- **`agent` crate**:
  - AWS Bedrock APIとの通信、会話履歴(`messages`)の管理を担当する。
  - `println!` 等のUI出力を行わず、純粋にデータ(`Result<T>`)を返す設計にする。

## 2. エラーハンドリング (Error Handling)

**現状と移行方針**:
- 現在のコードベースでは `Result<(), Box<dyn std::error::Error>>` が多用されていますが、**これは非推奨です**。
- **AIによる修正・追加実装の際は、以下のモダンなエラーハンドリングへ積極的にリファクタリングしてください。**

**推奨パターン**:
- **アプリケーション (`cli`)**:
  - `anyhow::Result` を使用し、`.context("...")` でエラー発生時の状況を付与する。
  - エラーは握りつぶさず、`main` 関数まで伝播させて表示する。
- **ライブラリ (`agent`)**:
  - `thiserror` を使用し、ドメイン固有のエラー型（例: `AgentError::NetworkError`, `AgentError::ApiLimit`）を定義する。

**コード例 (推奨)**:
```rust
// agent/src/lib.rs
#[derive(thiserror::Error, Debug)]
pub enum AgentError {
    #[error("AWS Bedrock API error: {0}")]
    BedrockError(#[from] aws_sdk_bedrockruntime::Error),
    #[error("Invalid input: {0}")]
    ValidationError(String),
}

// cli/src/main.rs
fn run() -> anyhow::Result<()> {
    let agent = AgentClient::new().context("Failed to initialize agent")?;
    // ...
    Ok(())
}
```

## 3. 非同期処理 (Async/Await)

**ルール**:

- tokio::main マクロを使用し、非同期関数内でのブロッキング操作（重い計算やstd::thread::sleep）を避ける。

- 待機が必要な場合は必ず tokio::time::sleep を使用する。
