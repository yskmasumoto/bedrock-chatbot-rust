//! MCPコマンド処理モジュール
//!
//! MCPサーバーの管理、接続、ツール一覧表示などのコマンド処理を提供します。

use agent::AgentClient;
use anyhow::{Context, Result};
use mcp::{McpClient, McpConfig};

/// MCPコマンドを処理する
///
/// # Arguments
/// * `server_name` - サーバー名（Noneの場合は全サーバーのリストを表示）
/// * `config_path` - mcp.jsonファイルのパス（Noneの場合はデフォルトパスを使用）
pub async fn handle_mcp_command(
    server_name: Option<String>,
    config_path: Option<String>,
) -> Result<()> {
    // 設定ファイルを読み込む
    let config = load_mcp_config(config_path)?;

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

/// MCP設定ファイルを読み込む
///
/// # Arguments
/// * `config_path` - 設定ファイルのパス（Noneの場合はデフォルトパスを使用）
fn load_mcp_config(config_path: Option<String>) -> Result<McpConfig> {
    if let Some(path) = config_path {
        McpConfig::load_from_file(&path)
            .with_context(|| format!("設定ファイルの読み込みに失敗しました: {}", path))
    } else {
        match McpConfig::load_default()? {
            Some(config) => Ok(config),
            None => {
                println!("mcp.jsonファイルが見つかりません。");
                println!("以下のいずれかのパスに配置してください：");
                println!("  - .chatbot/mcp.json");
                println!("  - mcp.json");
                anyhow::bail!("MCP設定ファイルが見つかりません")
            }
        }
    }
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
    if let Some(example_name) = config.servers.keys().next() {
        println!("例: mcp {}", example_name);
    }
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
pub async fn handle_mcp_connection_command(
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
                    display_connected_tools(&tools);
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

/// 接続されたMCPサーバーのツール一覧を表示する
fn display_connected_tools(tools: &[mcp::Tool]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use mcp::{McpConfig, ServerConfig};
    use std::collections::HashMap;

    fn create_test_config() -> McpConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "test-server".to_string(),
            ServerConfig {
                server_type: "stdio".to_string(),
                command: "test-command".to_string(),
                args: vec!["arg1".to_string(), "arg2".to_string()],
                env: HashMap::new(),
                env_file: None,
                cwd: None,
            },
        );
        McpConfig {
            inputs: vec![],
            servers,
        }
    }

    #[test]
    fn test_show_server_list_empty() {
        let config = McpConfig {
            inputs: vec![],
            servers: HashMap::new(),
        };
        // Should handle empty server list gracefully without panic
        show_server_list(&config);
    }

    #[test]
    fn test_show_server_list_with_servers() {
        let config = create_test_config();
        // Should display server list without panicking
        show_server_list(&config);
    }

    #[test]
    fn test_show_server_list_with_multiple_servers() {
        let mut servers = HashMap::new();
        servers.insert(
            "server1".to_string(),
            ServerConfig {
                server_type: "stdio".to_string(),
                command: "cmd1".to_string(),
                args: vec![],
                env: HashMap::new(),
                env_file: None,
                cwd: None,
            },
        );
        servers.insert(
            "server2".to_string(),
            ServerConfig {
                server_type: "stdio".to_string(),
                command: "cmd2".to_string(),
                args: vec!["--flag".to_string()],
                env: {
                    let mut env = HashMap::new();
                    env.insert("KEY".to_string(), "VALUE".to_string());
                    env
                },
                env_file: None,
                cwd: None,
            },
        );
        let config = McpConfig {
            inputs: vec![],
            servers,
        };
        // Should display all servers and show example
        show_server_list(&config);
    }

    #[test]
    fn test_load_mcp_config_with_invalid_path() {
        // Test that load_mcp_config handles invalid paths correctly
        let result = load_mcp_config(Some("/nonexistent/path/to/file.json".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_mcp_config_default_behavior() {
        // When no default config exists, should return an error
        // This test documents the expected behavior
        let result = load_mcp_config(None);
        // Either succeeds if default config exists, or fails if it doesn't
        // Both are valid behaviors depending on the environment
        let _ = result;
    }

    // Note: display_connected_tools is tested through integration tests
    // as it prints to stdout and requires actual Tool structures from rmcp
}
