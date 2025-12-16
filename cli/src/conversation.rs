//! 会話処理モジュール
//!
//! Bedrockとの会話フロー、ツール実行、ストリーミングレスポンスの処理を提供します。

use agent::AgentClient;
use anyhow::{Context, Result};
use aws_sdk_bedrockruntime::types::{ContentBlock, ConverseStreamOutput, ToolUseBlock};
use std::io::Write;
use std::time::Duration;
use tokio::time::sleep;

use crate::ui::{clear_loading_animation, print_assistant_prompt, LOADING_ANIMATION_CHARACTER, LOADING_ANIMATION_INTERVAL};

/// 会話のターンを処理する（ツール使用を含む）
///
/// ストリーミングレスポンスを処理し、必要に応じてツールを実行して会話を継続する。
///
/// # Arguments
/// * `agent` - AgentClientへの可変参照
/// * `response` - Bedrockからのレスポンス
/// * `loading_task` - ローディングアニメーションタスク
pub async fn process_conversation_turn(
    agent: &mut AgentClient,
    response: agent::ConverseStreamResponse,
    loading_task: &tokio::task::JoinHandle<()>,
) -> Result<()> {
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

        process_stream_event(event, &mut content_blocks, &mut current_text, &mut current_tool_use, agent)?;
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
        process_tool_usage(agent, &content_blocks).await?;
    }

    Ok(())
}

/// ストリームイベントを処理する
fn process_stream_event(
    event: ConverseStreamOutput,
    content_blocks: &mut Vec<ContentBlock>,
    current_text: &mut String,
    current_tool_use: &mut Option<(String, String, String)>,
    agent: &AgentClient,
) -> Result<()> {
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
                    if let Some((_, _, input)) = current_tool_use.as_mut() {
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
                *current_tool_use = Some((
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
                let tool_use_block = build_tool_use_block(tool_use_id, name, input, agent)?;
                content_blocks.push(ContentBlock::ToolUse(tool_use_block));
            }
        }
        _ => {}
    }

    Ok(())
}

/// ツール使用ブロックを構築する
fn build_tool_use_block(
    tool_use_id: String,
    name: String,
    input: String,
    agent: &AgentClient,
) -> Result<ToolUseBlock> {
    // JSON形式のinputをパース
    let input_json: serde_json::Value =
        serde_json::from_str(&input).context("Failed to parse tool use input as JSON")?;

    // Convert serde_json::Value to AWS Document using agent's utility function
    let input_doc = agent
        .json_to_document(input_json)
        .context("Failed to convert JSON to Document")?;

    ToolUseBlock::builder()
        .tool_use_id(tool_use_id)
        .name(name)
        .input(input_doc)
        .build()
        .context("Failed to build ToolUseBlock")
}

/// ツール使用を処理する
async fn process_tool_usage(agent: &mut AgentClient, content_blocks: &[ContentBlock]) -> Result<()> {
    // ツール実行して結果を返す
    for block in content_blocks {
        if let ContentBlock::ToolUse(tool_use) = block {
            execute_tool(agent, tool_use).await?;
        }
    }

    // ツール実行後、再度Bedrockに問い合わせて最終的な応答を得る
    println!();
    print_assistant_prompt()?;

    // ローディングアニメーション再開
    let loading_task = spawn_loading_animation();

    // ツール結果後のフォローアップリクエストを送信
    let follow_up_response = agent
        .send_tool_result_follow_up()
        .await
        .context("Failed to send follow-up message after tool use")?;

    // 再帰的に処理（ツール使用が連鎖する可能性があるため）
    Box::pin(process_conversation_turn(agent, follow_up_response, &loading_task)).await?;

    // 最後のユーザーメッセージ（空）をロールバック
    agent.rollback_last_user_message();

    Ok(())
}

/// ツールを実行して結果を履歴に追加する
async fn execute_tool(agent: &mut AgentClient, tool_use: &ToolUseBlock) -> Result<()> {
    println!("\n🔧 ツール実行中: {}...", tool_use.name());

    // Convert AWS Document to serde_json::Value for MCP tool call
    let input_doc = tool_use.input();
    let arguments = convert_tool_input(agent, input_doc);

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

    Ok(())
}

/// ツール入力をJSON形式に変換する
fn convert_tool_input(
    agent: &AgentClient,
    input_doc: &aws_smithy_types::Document,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    match agent.document_to_json(input_doc.clone()) {
        Ok(json_val) => {
            // MCP expects arguments as a Map, extract object if present
            match json_val {
                serde_json::Value::Object(map) => Some(map),
                _ => {
                    eprintln!("⚠️  Warning: Tool input is not an object, using empty arguments");
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
    }
}

/// ローディングアニメーションタスクを開始する
pub fn spawn_loading_animation() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async {
        loop {
            sleep(Duration::from_millis(LOADING_ANIMATION_INTERVAL)).await;
            print!("{}", LOADING_ANIMATION_CHARACTER);
            if std::io::stdout().flush().is_err() {
                break;
            }
        }
    })
}
