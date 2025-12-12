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

/// 会話中のMCPサーバー接続コマンドを処理する
///
/// # Arguments
/// * `agent` - AgentClientへの可変参照
/// * `config` - MCP設定
/// * `server_name` - 接続するサーバー名
async fn handle_mcp_connection_command(
    agent: &mut AgentClient,
    config: &McpConfig,
    server_name: &str,
) -> Result<()> {
    // サーバー設定を取得
    let server = match config.get_server(server_name) {
        Some(s) => s,
        None => {
            println!("エラー: サーバー '{}' が見つかりません", server_name);
            println!("利用可能なサーバー: {:?}", config.server_names());
            return Ok(());
        }
    };

    // stdio以外のタイプはサポート外
    if server.server_type != "stdio" {
        println!(
            "エラー: サーバータイプ '{}' はサポートされていません。",
            server.server_type
        );
        return Ok(());
    }

    // 既存の接続がある場合は切断
    if agent.is_mcp_connected() {
        println!("既存のMCPサーバーとの接続を切断中...");
        agent
            .disconnect_mcp()
            .await
            .context("既存のMCP接続の切断に失敗しました")?;
        println!("既存のMCPサーバーとの接続を切断しました。");
    }

    // カレントディレクトリをワークスペースフォルダとして使用
    let workspace_folder = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from));

    let command = server.resolve_command(workspace_folder.as_deref());
    let args = server.resolve_args(workspace_folder.as_deref());

    println!("MCPサーバー '{}' に接続中...", server_name);

    // 引数をVec<&str>に変換（ライフタイムに注意）
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    // AgentClientを通じて接続
    match agent.connect_mcp(&command, args_refs).await {
        Ok(()) => {
            println!("✅ MCPサーバー '{}' に接続しました。", server_name);

            // ツール一覧を取得して表示
            match agent.list_mcp_tools().await {
                Ok(tools) => {
                    if tools.is_empty() {
                        println!("   利用可能なツール: なし");
                    } else {
                        println!("   利用可能なツール: {} 個", tools.len());
                        for tool in tools.iter().take(5) {
                            println!("     - {}", tool.name);
                        }
                        if tools.len() > 5 {
                            println!("     ... 他 {} 個", tools.len() - 5);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("   警告: ツール一覧の取得に失敗しました: {}", e);
                }
            }
        }
        Err(e) => {
            println!("❌ MCPサーバーへの接続に失敗しました: {}", e);
            println!("   コマンド: {} {}", command, args.join(" "));
        }
    }

    Ok(())
}

/// 会話のターンを処理する（ツール使用を含む）
///
/// ストリーミングレスポンスを処理し、必要に応じてツールを実行して会話を継続する。
///
/// # Arguments
/// * `agent` - AgentClientへの可変参照
/// * `response` - Bedrockからのレスポンス
/// * `loading_task` - ローディングアニメーションタスク
async fn process_conversation_turn(
    agent: &mut AgentClient,
    response: agent::ConverseStreamResponse,
    loading_task: &tokio::task::JoinHandle<()>,
) -> Result<()> {
    use aws_sdk_bedrockruntime::types::{ContentBlock, ConverseStreamOutput, ToolUseBlock};

    let mut stream = response.stream;
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut current_tool_use: Option<(String, String, String)> = None; // (tool_use_id, name, input)
    let mut is_first_event = true;
    let mut loading_stopped = false;

    // ストリーム受信ループ
    while let Some(event) = stream.recv().await.context("Stream receive error")? {
        // 最初のイベントが届いたタイミングでローディングを消す
        if is_first_event {
            loading_task.abort();
            loading_stopped = true;
            clear_loading_animation();
            is_first_event = false;
        }

        match event {
            // テキストチャンク
            ConverseStreamOutput::ContentBlockDelta(delta) => {
                if let Some(delta_block) = delta.delta {
                    if let Ok(text) = delta_block.as_text() {
                        print!("{}", text);
                        current_text.push_str(text);
                        std::io::stdout().flush()?;
                    } else if let Ok(tool_use_delta) = delta_block.as_tool_use() {
                        // ツール使用のinputが段階的に来る
                        if let Some((_, _, ref mut input)) = current_tool_use {
                            input.push_str(tool_use_delta.input());
                        }
                    }
                }
            }
            // コンテンツブロック開始
            ConverseStreamOutput::ContentBlockStart(start) => {
                if let Some(start_block) = start.start
                    && let Ok(tool_use) = start_block.as_tool_use()
                {
                    // ツール使用開始
                    current_tool_use = Some((
                        tool_use.tool_use_id().to_string(),
                        tool_use.name().to_string(),
                        String::new(),
                    ));
                }
            }
            // コンテンツブロック終了
            ConverseStreamOutput::ContentBlockStop(_) => {
                // テキストブロックが完了した場合
                if !current_text.is_empty() {
                    content_blocks.push(ContentBlock::Text(current_text.clone()));
                    current_text.clear();
                }

                // ツール使用ブロックが完了した場合
                if let Some((tool_use_id, name, input)) = current_tool_use.take() {
                    // JSON形式のinputをパース
                    let input_json: serde_json::Value = serde_json::from_str(&input)
                        .context("Failed to parse tool use input as JSON")?;

                    // Convert serde_json::Value to AWS Document using agent's utility function
                    let input_doc = agent
                        .json_to_document(input_json.clone())
                        .context("Failed to convert JSON to Document")?;

                    let tool_use_block = ToolUseBlock::builder()
                        .tool_use_id(tool_use_id.clone())
                        .name(name.clone())
                        .input(input_doc)
                        .build()
                        .context("Failed to build ToolUseBlock")?;

                    content_blocks.push(ContentBlock::ToolUse(tool_use_block));
                }
            }
            _ => {}
        }
    }

    // ストリーム終了処理
    if !loading_stopped {
        loading_task.abort();
        clear_loading_animation();
    }

    // 残りのテキストがあれば追加
    if !current_text.is_empty() {
        content_blocks.push(ContentBlock::Text(current_text));
    }

    println!(); // 最後に改行

    // アシスタントのメッセージを履歴に追加
    agent
        .add_assistant_message_with_blocks(content_blocks.clone())
        .context("Failed to add assistant message")?;

    // ツール使用があればそれを処理
    let has_tool_use = content_blocks
        .iter()
        .any(|block| matches!(block, ContentBlock::ToolUse(_)));

    if has_tool_use && agent.is_mcp_connected() {
        // ツール実行して結果を返す
        for block in &content_blocks {
            if let ContentBlock::ToolUse(tool_use) = block {
                println!("\n🔧 ツール実行中: {}...", tool_use.name());

                // Convert AWS Document to serde_json::Value for MCP tool call
                let input_doc = tool_use.input();
                let arguments = match agent.document_to_json(input_doc.clone()) {
                    Ok(json_val) => {
                        // MCP expects arguments as a Map, extract object if present
                        match json_val {
                            serde_json::Value::Object(map) => Some(map),
                            _ => {
                                eprintln!(
                                    "⚠️  Warning: Tool input is not an object, using empty arguments"
                                );
                                None
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "⚠️  Warning: Failed to convert tool input: {}, using empty arguments",
                            e
                        );
                        None
                    }
                };

                // MCPツールを実行
                match agent
                    .call_mcp_tool(tool_use.name().to_string(), arguments)
                    .await
                {
                    Ok(result) => {
                        println!("✅ ツール実行完了");

                        // ツール結果を履歴に追加
                        agent
                            .add_tool_result(tool_use.tool_use_id().to_string(), result)
                            .context("Failed to add tool result")?;
                    }
                    Err(e) => {
                        eprintln!("❌ ツール実行エラー: {}", e);

                        // エラーもツール結果として返す
                        let error_result = serde_json::json!({
                            "error": e.to_string()
                        });
                        agent
                            .add_tool_result(tool_use.tool_use_id().to_string(), error_result)
                            .context("Failed to add tool error result")?;
                    }
                }
            }
        }

        // ツール実行後、再度Bedrockに問い合わせて最終的な応答を得る
        println!("\n{} > ", AGENT_NAME);
        std::io::stdout().flush()?;

        // ローディングアニメーション再開
        let loading_task2 = tokio::spawn(async {
            loop {
                sleep(Duration::from_millis(LOADING_ANIMATION_INTERVAL)).await;
                print!("{}", LOADING_ANIMATION_CHARACTER);
                if std::io::stdout().flush().is_err() {
                    break;
                }
            }
        });

        // 空のユーザーメッセージを送信してBedrockにツール結果を処理させる
        // 実際にはツール結果が既に履歴に追加されているので、それに基づいて応答する
        let follow_up_response = agent
            .send_message("")
            .await
            .context("Failed to send follow-up message after tool use")?;

        // 再帰的に処理（ツール使用が連鎖する可能性があるため）
        // Box::pin を使用して無限サイズのfutureを回避
        Box::pin(process_conversation_turn(
            agent,
            follow_up_response,
            &loading_task2,
        ))
        .await?;

        // 最後のユーザーメッセージ（空）をロールバック
        agent.rollback_last_user_message();
    }

    Ok(())
}
