//! UI関連の定数とヘルパー関数
//!
//! ユーザーインターフェースに関する設定値と、
//! 表示やアニメーションのヘルパー関数を提供します。

use std::io::Write;

/// ユーザー名の表示
pub const USER_NAME: &str = "User";

/// アシスタント名の表示
pub const AGENT_NAME: &str = "Assistant";

/// ローディングアニメーションの更新間隔（ミリ秒）
pub const LOADING_ANIMATION_INTERVAL: u64 = 200;

/// ローディングアニメーションに使用する文字
pub const LOADING_ANIMATION_CHARACTER: &str = ".";

/// ローディングアニメーションをクリアするためのスペース文字列
/// (ローディング中に表示される可能性のある最大文字数を想定: 約30-40文字分のドット)
const CLEAR_LINE_SPACES: &str = "                                     "; // 37 spaces

/// ローディングアニメーションをクリアしてカーソルを戻す
///
/// 行頭に戻り、スペースで上書きしてから再度行頭に戻り、プロンプトを表示する。
pub fn clear_loading_animation() {
    print!(
        "\r{} > {}\r{} > ",
        AGENT_NAME, CLEAR_LINE_SPACES, AGENT_NAME
    );
    let _ = std::io::stdout().flush();
}

/// アシスタントの応答プロンプトを表示する
///
/// # Errors
/// 標準出力へのフラッシュに失敗した場合
pub fn print_assistant_prompt() -> std::io::Result<()> {
    print!("{} > ", AGENT_NAME);
    std::io::stdout().flush()
}

/// ユーザー入力プロンプトを生成する
pub fn user_prompt() -> String {
    format!("{} > ", USER_NAME)
}
