mod conversation;
mod mcp_commands;
mod ui;

use agent::AgentClient;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mcp::McpConfig;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use conversation::{process_conversation_turn, spawn_loading_animation};
use mcp_commands::{handle_mcp_command, handle_mcp_connection_command};
use ui::{print_assistant_prompt, user_prompt};

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

    // mcp.json設定ファイルを読み込む（オプション）
    let mcp_config = match McpConfig::load_default() {
        Ok(Some(config)) => {
            println!("MCP設定ファイルを読み込みました。");
            println!("利用可能なMCPサーバー: {}", config.server_names().len());
            Some(config)
        }
        Ok(None) => {
            println!("MCP設定ファイルが見つかりません。MCPなしで起動します。");
            None
        }
        Err(e) => {
            println!("警告: MCP設定ファイルの読み込みに失敗しました: {}", e);
            println!("MCPなしで起動します。");
            None
        }
    };

    // rustylineエディタの初期化（UI層）
    let mut rl = DefaultEditor::new().context("Failed to initialize rustyline editor")?;

    println!("Using Model: {}", agent.model_id());
    println!("+--------------------------------------------------+");
    println!("| AI Agent Started. Type 'exit' or 'quit' to stop. |");
    if mcp_config.is_some() {
        println!("| MCP commands: 'mcp <server_name>' to connect    |");
    }
    println!("+--------------------------------------------------+");

    loop {
        // ユーザー入力の受け付け
        let readline = rl.readline(&user_prompt());
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

                // MCPコマンドの処理
                if let Some(server_name) = input.strip_prefix("mcp ") {
                    if let Some(ref config) = mcp_config {
                        handle_mcp_connection_command(&mut agent, config, server_name.trim())
                            .await?;
                    } else {
                        println!("MCP設定ファイルが読み込まれていません。");
                    }
                    continue;
                }

                // 履歴に追加
                let _ = rl.add_history_entry(input);

                // アシスタントの応答開始を表示
                print_assistant_prompt()?;

                // ローディングアニメーション開始
                let loading_task = spawn_loading_animation();

                // メッセージ送信（ビジネスロジック層）
                let response_result = agent.send_message(input).await;

                match response_result {
                    Ok(response) => {
                        // ツール使用フローを処理
                        match process_conversation_turn(&mut agent, response, &loading_task).await {
                            Ok(_) => {}
                            Err(e) => {
                                loading_task.abort();
                                println!("\n[Error] Conversation processing failed: {}", e);
                                agent.rollback_last_user_message();
                            }
                        }
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

    // 会話終了時のクリーンアップ：MCPサーバーとの接続を切断
    if agent.is_mcp_connected() {
        println!("MCPサーバーとの接続を切断中...");
        if let Err(e) = agent.disconnect_mcp().await {
            eprintln!("警告: MCP切断に失敗しました: {}", e);
        } else {
            println!("MCPサーバーとの接続を切断しました。");
        }
    }

    Ok(())
}
