use aws_config::meta::region::RegionProviderChain;
use aws_config::{self, BehaviorVersion};
use aws_sdk_bedrockruntime::Client;
use aws_sdk_bedrockruntime::types::{
    ContentBlock, ConversationRole, ConverseStreamOutput, Message,
};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::Write;
use std::time::Duration;
use tokio::time::sleep;

// 使用するモデルID
const MODEL_ID: &str = "anthropic.claude-3-5-sonnet-20240620-v1:0";

// ローディングアニメーションの設定
const LOADING_ANIMATION_INTERVAL: u64 = 200;
const LOADING_ANIMATION_CHARACTER: &str = ".";

// エージェントの設定
const USER_NAME: &str = "User";
const AGENT_NAME: &str = "Assistant";

pub async fn run_agent(
    profile: String,
    region: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing Agent with profile: {}", profile);

    let region_provider = RegionProviderChain::first_try(region.map(aws_config::Region::new))
        .or_default_provider()
        .or_else(aws_config::Region::new("us-east-1"));

    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(region_provider)
        .profile_name(&profile)
        .load()
        .await;

    let client = Client::new(&config);
    let mut messages: Vec<Message> = Vec::new();
    let mut rl = DefaultEditor::new()?;

    println!("Using Model: {}", MODEL_ID);
    println!("+--------------------------------------------------+");
    println!("| AI Agent Started. Type 'exit' or 'quit' to stop. |");
    println!("+--------------------------------------------------+");

    loop {
        let readline = rl.readline(&format!("{} > ", USER_NAME));
        match readline {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                    break;
                }

                let _ = rl.add_history_entry(input);

                let user_message = Message::builder()
                    .role(ConversationRole::User)
                    .content(ContentBlock::Text(input.to_string()))
                    .build()
                    .map_err(|e| format!("Failed to build message: {}", e))?;

                messages.push(user_message);

                print!("{} > ", AGENT_NAME);
                std::io::stdout().flush()?;

                // --- ローディングアニメーション開始 ---
                let loading_task = tokio::spawn(async {
                    loop {
                        sleep(Duration::from_millis(LOADING_ANIMATION_INTERVAL)).await; // 更新頻度を少し上げました
                        print!("{}", LOADING_ANIMATION_CHARACTER);
                        std::io::stdout().flush().unwrap();
                    }
                });

                // APIリクエスト送信（ここではまだストリームの中身は来ない）
                let response_result = client
                    .converse_stream()
                    .model_id(MODEL_ID)
                    .set_messages(Some(messages.clone()))
                    .send()
                    .await;

                match response_result {
                    Ok(output) => {
                        let mut stream = output.stream;
                        let mut full_response_text = String::new();
                        let mut is_first_event = true; // 最初のイベント判定フラグ

                        // ストリーム受信ループ
                        while let Some(event) = stream.recv().await? {
                            // ★修正点: 最初のイベントが届いたタイミングでローディングを消す
                            if is_first_event {
                                loading_task.abort();
                                // ローディングの `...` を消してカーソルを戻す
                                print!(
                                    "\rAssistant >                                     \rAssistant > "
                                );
                                std::io::stdout().flush()?;
                                is_first_event = false;
                            }

                            match event {
                                ConverseStreamOutput::ContentBlockDelta(delta) => {
                                    if let Some(delta_block) = delta.delta {
                                        if let Ok(text) = delta_block.as_text() {
                                            print!("{}", text);
                                            full_response_text.push_str(text);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        // ストリーム終了処理
                        if is_first_event {
                            // もしイベントが一つも来ずに終了した場合（エラー等）もローディングを消す
                            loading_task.abort();
                            print!(
                                "\rAssistant >                                     \rAssistant > "
                            );
                        }

                        println!(); // 最後に改行

                        let assistant_message = Message::builder()
                            .role(ConversationRole::Assistant)
                            .content(ContentBlock::Text(full_response_text))
                            .build()
                            .map_err(|e| format!("Failed to build message: {}", e))?;

                        messages.push(assistant_message);
                    }
                    Err(e) => {
                        loading_task.abort(); // エラー時も止める
                        println!("\n[Error] Bedrock API call failed: {}", e);
                        messages.pop();
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
