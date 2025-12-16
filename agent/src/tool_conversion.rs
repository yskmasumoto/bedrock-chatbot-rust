//! ツール変換ヘルパー関数
//!
//! MCPツールとBedrockツールの間の変換処理を提供します。

use crate::AgentError;
use aws_sdk_bedrockruntime::types::{Tool, ToolInputSchema, ToolSpecification};
use aws_smithy_types::Document;

/// MCPツールをBedrockツール形式に変換する
///
/// # Arguments
/// * `mcp_tools` - MCPツールのリスト
///
/// # Returns
/// * `Ok(Vec<Tool>)` - Bedrock形式のツール定義リスト
/// * `Err(AgentError)` - 変換に失敗した場合
///
/// # Note
/// input_schemaがnullのツールはスキップされます（Bedrockはnullを受け付けないため）
pub fn convert_mcp_tools_to_bedrock(
    mcp_tools: Vec<mcp::Tool>,
) -> Result<Vec<Tool>, AgentError> {
    let mut bedrock_tools = Vec::new();

    for mcp_tool in mcp_tools {
        // MCPツールのスキーマをJSON Valueに変換
        let input_schema_json = serde_json::to_value(&mcp_tool.input_schema)?;

        // input_schemaがnullの場合はスキップ（Bedrockはnullを受け付けない）
        if input_schema_json.is_null() {
            eprintln!(
                "[Warning] Skipping tool '{}' due to missing input_schema",
                mcp_tool.name
            );
            continue;
        }

        // JSON ValueをAWS Smithy Documentに変換
        let schema_document = crate::agent::json_to_document(input_schema_json)?;

        // ToolSpecificationを構築
        let tool_spec = build_tool_specification(
            &mcp_tool.name,
            &mcp_tool.description.unwrap_or_default(),
            schema_document,
        )?;

        bedrock_tools.push(Tool::ToolSpec(tool_spec));
    }

    Ok(bedrock_tools)
}

/// ツール仕様を構築する
///
/// # Arguments
/// * `name` - ツール名
/// * `description` - ツールの説明
/// * `schema_document` - 入力スキーマ（AWS Document形式）
///
/// # Returns
/// * `Ok(ToolSpecification)` - 構築されたツール仕様
/// * `Err(AgentError)` - 構築に失敗した場合
fn build_tool_specification(
    name: &str,
    description: &str,
    schema_document: Document,
) -> Result<ToolSpecification, AgentError> {
    ToolSpecification::builder()
        .name(name)
        .description(description)
        .input_schema(ToolInputSchema::Json(schema_document))
        .build()
        .map_err(|e| AgentError::MessageBuildError(format!("Failed to build tool spec: {}", e)))
}
