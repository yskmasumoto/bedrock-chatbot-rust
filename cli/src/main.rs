use agent::AgentClient;
use clap::{Parser, Subcommand};
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 引数の解析
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            aws_profile,
            region,
        } => {
            run_agent_cli(aws_profile, region).await?;
        }
    }

    Ok(())
}

/// CLI対話型エージェントを実行する
///
/// ユーザー入力の受け付け、ローディング表示、ストリーミングレスポンスの表示など、
/// すべてのUI/UX処理を担当する。
async fn run_agent_cli(
    aws_profile: String,
    region: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing Agent with profile: {}", aws_profile);

    // エージェントクライアントの初期化（ビジネスロジック層）
    let mut agent = AgentClient::new(aws_profile, region).await?;

    // rustylineエディタの初期化（UI層）
    let mut rl = DefaultEditor::new()?;

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
                        std::io::stdout().flush().unwrap();
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

                        // ストリーム受信ループ
                        while let Some(event) = stream.recv().await? {
                            // 最初のイベントが届いたタイミングでローディングを消す
                            if is_first_event {
                                loading_task.abort();
                                // ローディングの `...` を消してカーソルを戻す
                                print!(
                                    "\r{} >                                     \r{} > ",
                                    AGENT_NAME, AGENT_NAME
                                );
                                std::io::stdout().flush()?;
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
                        if is_first_event {
                            // イベントが一つも来ずに終了した場合もローディングを消す
                            loading_task.abort();
                            print!(
                                "\r{} >                                     \r{} > ",
                                AGENT_NAME, AGENT_NAME
                            );
                        }

                        println!(); // 最後に改行

                        // アシスタントのメッセージを履歴に追加（ビジネスロジック層）
                        agent.add_assistant_message(full_response_text)?;
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
