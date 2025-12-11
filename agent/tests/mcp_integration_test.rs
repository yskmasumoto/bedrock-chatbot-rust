/// AgentとMCPの統合テスト
///
/// このテストはAgentClientとMCPサーバーの統合動作を検証します。
use agent::{AgentClient, AgentError};
use std::env;

#[tokio::test]
async fn test_agent_mcp_connection() {
    // 環境変数でテストをスキップ
    if env::var("SKIP_MCP_INTEGRATION_TEST").is_ok() {
        eprintln!("SKIP_MCP_INTEGRATION_TEST が設定されているため、テストをスキップします");
        return;
    }

    // AgentClientを初期化（AWS認証情報は不要、MCPのみテスト）
    // 注意: AWS認証情報がない環境では new() が失敗する可能性があります
    // その場合は、このテストを #[ignore] でマークしてください
    let result = AgentClient::new("default".to_string(), None).await;

    match result {
        Ok(mut agent) => {
            // MCPサーバーへの接続をテスト
            // 存在しないコマンドでエラーになることを確認
            let connect_result = agent
                .connect_mcp("nonexistent_mcp_command", vec!["test"])
                .await;

            assert!(
                connect_result.is_err(),
                "存在しないコマンドでの接続はエラーになるべき"
            );

            // 接続状態の確認
            assert!(
                !agent.is_mcp_connected(),
                "接続失敗時は未接続状態であるべき"
            );
        }
        Err(e) => {
            eprintln!("AWS認証情報がないためAgentClientの初期化に失敗: {:?}", e);
            eprintln!("このテストはAWS認証情報が必要です");
        }
    }
}

#[tokio::test]
async fn test_agent_mcp_disconnect_without_connection() {
    // 環境変数でテストをスキップ
    if env::var("SKIP_MCP_INTEGRATION_TEST").is_ok() {
        return;
    }

    let result = AgentClient::new("default".to_string(), None).await;

    if let Ok(mut agent) = result {
        // 接続していない状態で切断を試みる
        let disconnect_result = agent.disconnect_mcp().await;

        assert!(
            disconnect_result.is_err(),
            "未接続状態での切断はエラーになるべき"
        );

        // エラーの種類を確認
        if let Err(AgentError::ConfigError(msg)) = disconnect_result {
            assert!(
                msg.contains("not connected"),
                "エラーメッセージに'not connected'が含まれるべき"
            );
        } else {
            panic!("ConfigErrorが返されるべき");
        }
    }
}

#[cfg(test)]
mod real_server_tests {
    use super::*;

    /// 実際のMCPサーバーを使用したAgentの統合テスト
    #[tokio::test]
    #[ignore] // デフォルトではスキップ
    async fn test_agent_with_real_mcp_server() {
        // AgentClientを初期化
        let mut agent = AgentClient::new("default".to_string(), None)
            .await
            .expect("AgentClientの初期化に失敗");

        // MCPサーバーに接続
        agent
            .connect_mcp("uvx", vec!["mcp-server-git"])
            .await
            .expect("MCPサーバーへの接続に失敗");

        // 接続状態を確認
        assert!(agent.is_mcp_connected(), "接続成功後は接続状態であるべき");

        // ツール一覧を取得
        let tools = agent
            .list_mcp_tools()
            .await
            .expect("MCPツール一覧の取得に失敗");

        eprintln!("取得したMCPツール数: {}", tools.len());
        assert!(!tools.is_empty(), "少なくとも1つのツールが存在すること");

        // 明示的に切断
        agent.disconnect_mcp().await.expect("MCP切断に失敗");

        // 切断後は未接続状態
        assert!(!agent.is_mcp_connected(), "切断後は未接続状態であるべき");
    }

    /// 複数回の接続・切断テスト
    #[tokio::test]
    #[ignore]
    async fn test_agent_multiple_connections() {
        let mut agent = AgentClient::new("default".to_string(), None)
            .await
            .expect("AgentClientの初期化に失敗");

        // 1回目の接続
        agent
            .connect_mcp("uvx", vec!["mcp-server-git"])
            .await
            .expect("1回目の接続に失敗");
        assert!(agent.is_mcp_connected());

        // 切断
        agent.disconnect_mcp().await.expect("1回目の切断に失敗");
        assert!(!agent.is_mcp_connected());

        // 2回目の接続
        agent
            .connect_mcp("uvx", vec!["mcp-server-git"])
            .await
            .expect("2回目の接続に失敗");
        assert!(agent.is_mcp_connected());

        // 切断
        agent.disconnect_mcp().await.expect("2回目の切断に失敗");
        assert!(!agent.is_mcp_connected());
    }

    /// 接続を切り替えるテスト（古い接続を自動切断）
    #[tokio::test]
    #[ignore]
    async fn test_agent_connection_replacement() {
        let mut agent = AgentClient::new("default".to_string(), None)
            .await
            .expect("AgentClientの初期化に失敗");

        // 1回目の接続
        agent
            .connect_mcp("uvx", vec!["mcp-server-git"])
            .await
            .expect("1回目の接続に失敗");

        // 切断せずに2回目の接続（自動的に古い接続を切断）
        agent
            .connect_mcp("uvx", vec!["mcp-server-git"])
            .await
            .expect("2回目の接続に失敗");

        // 接続状態を確認
        assert!(agent.is_mcp_connected());

        // ツールが取得できることを確認（新しい接続が有効）
        let tools = agent
            .list_mcp_tools()
            .await
            .expect("ツール一覧の取得に失敗");

        assert!(!tools.is_empty());

        // クリーンアップ
        agent.disconnect_mcp().await.expect("切断に失敗");
    }

    /// Drop時の警告テスト
    #[tokio::test]
    #[ignore]
    async fn test_agent_drop_with_active_connection() {
        // このテストは警告メッセージが出力されることを確認する
        // 実際の検証は目視または標準エラー出力のキャプチャが必要

        let mut agent = AgentClient::new("default".to_string(), None)
            .await
            .expect("AgentClientの初期化に失敗");

        agent
            .connect_mcp("uvx", vec!["mcp-server-git"])
            .await
            .expect("MCPサーバーへの接続に失敗");

        // 明示的に切断せずにドロップ
        // -> Drop trait の実装により警告メッセージが出力されるはず
        drop(agent);

        eprintln!("上記の警告メッセージが表示されていれば、Drop trait が正常に動作しています");
    }
}
