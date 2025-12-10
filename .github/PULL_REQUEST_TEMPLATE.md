## 概要 (Summary)
## 関連Issue (Related Issues)
- Closes #

## 変更対象 (Scope)
- [ ] `agent` crate (Business Logic)
- [ ] `cli` crate (UI/UX)
- [ ] Configuration (Cargo.toml, workflows, etc.)
- [ ] Documentation / Others

## 実装の解説 (Implementation Details)
## 品質チェックリスト (Quality Assurance)
### Rust Project Guidelines
- [ ] **責務の分離**: UI層（`main.rs`など）に複雑なロジックを記述せず、適切なモジュール/ライブラリに分離しました
- [ ] **エラーハンドリング**: `unwrap()` や `expect()` の乱用を避け、`Result`型や `anyhow`/`thiserror` で適切に処理しました
- [ ] **テスト**: ロジックの変更に伴い、適切なテストコードを追加・更新しました（またはモックテストを作成しました）

### Pre-submission Checks
- [ ] `cargo fmt` を実行し、フォーマットエラーがないことを確認しました
- [ ] `cargo clippy` を実行し、Lintエラーがないことを確認しました
- [ ] `cargo build` が成功することを確認しました

## レビュアーへの特記事項 (Notes for Reviewer)
