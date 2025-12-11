use agent::AgentClient;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mcp::{McpClient, McpConfig};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::Write;
use std::time::Duration;
use tokio::time::sleep;

// UI関連の設定
const USER_NAME: &str = "User";
const AGENT_NAME: &str = "Assistant";
const LOADING_ANIMATION_INTERVAL: u64 = 200;
const LOADING_ANIMATION_CHARACTER: &str = ".";
// ローディングアニメーションをクリアするためのスペース文字列
// (ローディング中に表示される可能性のある最大文字数を想定: 約30-40文字分のドット)
const CLEAR_LINE_SPACES: &str = "                                     "; // 37 spaces

// CLIの引数構造体定義
#[derive(Parser)]
#[command(name = "agent-cli")]
#[command(about = "A simple AI Agent CLI using AWS Bedrock", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// エージェントを起動します
    Run {
        /// 使用するAWSプロファイル名
        #[arg(long)]
        aws_profile: String,

        /// リージョン (オプション: デフォルトはプロファイル設定またはus-east-1など)
        #[arg(long)]
        region: Option<String>,
    },
    /// MCPサーバーの情報を表示します
    Mcp {
        /// 特定のMCPサーバー名（省略時は全サーバーのリストを表示）
        server_name: Option<String>,

        /// mcp.jsonファイルのパス（省略時は.vscode/mcp.jsonまたはmcp.jsonを使用）
        #[arg(long)]
        config: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // 引数の解析
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            aws_profile,
            region,
        } => {
            run_agent_cli(aws_profile, region).await?;
        }
        Commands::Mcp {
            server_name,
            config,
        } => {
            handle_mcp_command(server_name, config).await?;
        }
    }

    Ok(())
}

/// CLI対話型エージェントを実行する
///
/// ユーザー入力の受け付け、ローディング表示、ストリーミングレスポンスの表示など、
/// すべてのUI/UX処理を担当する。
async fn run_agent_cli(aws_profile: String, region: Option<String>) -> Result<()> {
    println!("Initializing Agent with profile: {}", aws_profile);

    // エージェントクライアントの初期化（ビジネスロジック層）
    let mut agent = AgentClient::new(aws_profile, region)
        .await
        .context("Failed to initialize AgentClient")?;

    // rustylineエディタの初期化（UI層）
    let mut rl = DefaultEditor::new().context("Failed to initialize rustyline editor")?;

    println!("Using Model: {}", agent.model_id());
    println!("+--------------------------------------------------+");
    println!("| AI Agent Started. Type 'exit' or 'quit' to stop. |");
    println!("+--------------------------------------------------+");

    loop {
        // ユーザー入力の受け付け
        let readline = rl.readline(&format!("{} > ", USER_NAME));
        match readline {
            Ok(line) => {
                let input = line.trim();

                // 空入力はスキップ
                if input.is_empty() {
                    continue;
                }

                // 終了コマンドの処理
                if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                    break;
                }

                // 履歴に追加
                let _ = rl.add_history_entry(input);

                // アシスタントの応答開始を表示
                print!("{} > ", AGENT_NAME);
                std::io::stdout().flush()?;

                // ローディングアニメーション開始
                let loading_task = tokio::spawn(async {
                    loop {
                        sleep(Duration::from_millis(LOADING_ANIMATION_INTERVAL)).await;
                        print!("{}", LOADING_ANIMATION_CHARACTER);
                        // エラーが発生した場合はログに記録してループを抜ける
                        if std::io::stdout().flush().is_err() {
                            break;
                        }
                    }
                });

                // メッセージ送信（ビジネスロジック層）
                let response_result = agent.send_message(input).await;

                match response_result {
                    Ok(response) => {
                        // ストリーム処理用の変数
                        let mut stream = response.stream;
                        let mut full_response_text = String::new();
                        let mut is_first_event = true;
                        let mut loading_stopped = false;

                        // ストリーム受信ループ
                        while let Some(event) =
                            stream.recv().await.context("Stream receive error")?
                        {
                            // 最初のイベントが届いたタイミングでローディングを消す
                            if is_first_event {
                                loading_task.abort();
                                loading_stopped = true;
                                clear_loading_animation();
                                is_first_event = false;
                            }

                            // テキストチャンクの表示
                            if let aws_sdk_bedrockruntime::types::ConverseStreamOutput::ContentBlockDelta(delta) = event
                                && let Some(delta_block) = delta.delta
                                && let Ok(text) = delta_block.as_text()
                            {
                                print!("{}", text);
                                full_response_text.push_str(text);
                                std::io::stdout().flush()?;
                            }
                        }

                        // ストリーム終了処理
                        if !loading_stopped {
                            // イベントが一つも来ずに終了した場合もローディングを消す
                            loading_task.abort();
                            clear_loading_animation();
                        }

                        println!(); // 最後に改行

                        // アシスタントのメッセージを履歴に追加（ビジネスロジック層）
                        agent
                            .add_assistant_message(full_response_text)
                            .context("Failed to add assistant message")?;
                    }
                    Err(e) => {
                        loading_task.abort();
                        println!("\n[Error] Bedrock API call failed: {}", e);
                        // エラー時はユーザーメッセージを履歴から削除
                        agent.rollback_last_user_message();
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}

/// ローディングアニメーションをクリアしてカーソルを戻す
///
/// 行頭に戻り、スペースで上書きしてから再度行頭に戻り、プロンプトを表示する。
fn clear_loading_animation() {
    print!(
        "\r{} > {}\r{} > ",
        AGENT_NAME, CLEAR_LINE_SPACES, AGENT_NAME
    );
    let _ = std::io::stdout().flush();
}

/// MCPコマンドを処理する
///
/// # Arguments
/// * `server_name` - サーバー名（Noneの場合は全サーバーのリストを表示）
/// * `config_path` - mcp.jsonファイルのパス（Noneの場合はデフォルトパスを使用）
async fn handle_mcp_command(
    server_name: Option<String>,
    config_path: Option<String>,
) -> Result<()> {
    // 設定ファイルを読み込む
    let config = if let Some(path) = config_path {
        McpConfig::load_from_file(&path)
            .with_context(|| format!("設定ファイルの読み込みに失敗しました: {}", path))?
    } else {
        match McpConfig::load_default()? {
            Some(config) => config,
            None => {
                println!("mcp.jsonファイルが見つかりません。");
                println!("以下のいずれかのパスに配置してください：");
                println!("  - .vscode/mcp.json");
                println!("  - mcp.json");
                return Ok(());
            }
        }
    };

    match server_name {
        // サーバー名が指定された場合：そのサーバーのツール一覧を表示
        Some(name) => {
            show_server_tools(&config, &name).await?;
        }
        // サーバー名が指定されていない場合：全サーバーのリストを表示
        None => {
            show_server_list(&config);
        }
    }

    Ok(())
}

/// 全MCPサーバーのリストを表示
fn show_server_list(config: &McpConfig) {
    if config.servers.is_empty() {
        println!("設定されているMCPサーバーはありません。");
        return;
    }

    println!("利用可能なMCPサーバー：");
    println!();

    for (name, server) in &config.servers {
        println!("  📦 {}", name);
        println!("     タイプ: {}", server.server_type);
        println!("     コマンド: {}", server.command);

        if !server.args.is_empty() {
            println!("     引数: {}", server.args.join(" "));
        }

        if !server.env.is_empty() {
            println!("     環境変数: {} 個", server.env.len());
        }

        println!();
    }

    println!("ツール一覧を表示するには: mcp <サーバー名>");
    println!("例: mcp {}", config.servers.keys().next().unwrap());
}

/// 特定のMCPサーバーのツール一覧を表示
async fn show_server_tools(config: &McpConfig, server_name: &str) -> Result<()> {
    // サーバー設定を取得
    let server = config
        .get_server(server_name)
        .with_context(|| format!("サーバー '{}' が見つかりません", server_name))?;

    // stdio以外のタイプはサポート外
    if server.server_type != "stdio" {
        anyhow::bail!(
            "サーバータイプ '{}' はサポートされていません。現在は'stdio'のみ対応しています。",
            server.server_type
        );
    }

    println!("MCPサーバー '{}' に接続中...", server_name);

    // カレントディレクトリをワークスペースフォルダとして使用
    let workspace_folder = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from));

    let command = server.resolve_command(workspace_folder.as_deref());
    let args = server.resolve_args(workspace_folder.as_deref());

    // 引数をVec<&str>に変換
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    // MCPクライアントで接続
    let client = McpClient::new(&command, args_refs)
        .await
        .with_context(|| format!("MCPサーバー '{}' への接続に失敗しました", server_name))?;

    // サーバー情報を表示
    if let Some(info) = client.server_info() {
        println!("サーバー情報:");
        println!("  {:?}", info);
        println!();
    }

    // ツール一覧を取得
    println!("利用可能なツール：");
    let tools = client
        .list_tools()
        .await
        .context("ツール一覧の取得に失敗しました")?;

    if tools.is_empty() {
        println!("  （ツールなし）");
    } else {
        for tool in &tools {
            println!("  🔧 {}", tool.name);
            if let Some(description) = &tool.description {
                println!("     説明: {}", description);
            }
            println!();
        }
        println!("合計: {} 個のツール", tools.len());
    }

    // 切断
    client.disconnect().await?;

    Ok(())
}
