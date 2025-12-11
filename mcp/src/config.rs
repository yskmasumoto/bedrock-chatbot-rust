/// mcp.json設定ファイルの構造体定義
///
/// Visual Studio Codeの`.vscode/mcp.json`仕様に準拠した
/// MCP設定ファイルのパースと管理機能を提供します。
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// mcp.jsonファイルのルート構造
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// 入力プロンプト定義（オプション）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<InputConfig>,

    /// MCPサーバーの設定マップ
    pub servers: HashMap<String, ServerConfig>,
}

/// 入力プロンプトの設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    /// 入力タイプ（例: "promptString"）
    #[serde(rename = "type")]
    pub input_type: String,

    /// 入力ID
    pub id: String,

    /// 説明文
    pub description: String,

    /// パスワード入力かどうか
    #[serde(default)]
    pub password: bool,
}

/// MCPサーバーの設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// サーバータイプ（現在は"stdio"のみサポート）
    #[serde(rename = "type")]
    pub server_type: String,

    /// 実行するコマンド
    pub command: String,

    /// コマンドライン引数（オプション）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// 環境変数（オプション）
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,

    /// 環境変数ファイルのパス（オプション）
    #[serde(rename = "envFile", skip_serializing_if = "Option::is_none")]
    pub env_file: Option<String>,

    /// 作業ディレクトリ（オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl McpConfig {
    /// mcp.jsonファイルを読み込む
    ///
    /// # Arguments
    /// * `path` - mcp.jsonファイルのパス
    ///
    /// # Returns
    /// パースされた設定
    ///
    /// # Errors
    /// ファイルの読み込みやパースに失敗した場合
    pub fn load_from_file(path: impl Into<PathBuf>) -> Result<Self, std::io::Error> {
        let path = path.into();
        let content = std::fs::read_to_string(&path)?;
        let config: McpConfig = serde_json::from_str(&content).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse mcp.json: {}", e),
            )
        })?;
        Ok(config)
    }

    /// デフォルトの設定ファイルパスを取得
    ///
    /// 以下の順序で検索：
    /// 1. `.vscode/mcp.json`（VS Code規約）
    /// 2. `mcp.json`（カレントディレクトリ）
    pub fn default_path() -> Option<PathBuf> {
        let vscode_path = PathBuf::from(".vscode/mcp.json");
        if vscode_path.exists() {
            return Some(vscode_path);
        }

        let current_path = PathBuf::from("mcp.json");
        if current_path.exists() {
            return Some(current_path);
        }

        None
    }

    /// デフォルトパスから設定を読み込む
    ///
    /// # Returns
    /// 設定が見つかった場合はSome(config)、見つからない場合はNone
    pub fn load_default() -> Result<Option<Self>, std::io::Error> {
        match Self::default_path() {
            Some(path) => Ok(Some(Self::load_from_file(path)?)),
            None => Ok(None),
        }
    }

    /// サーバー名のリストを取得
    pub fn server_names(&self) -> Vec<&String> {
        self.servers.keys().collect()
    }

    /// 特定のサーバー設定を取得
    pub fn get_server(&self, name: &str) -> Option<&ServerConfig> {
        self.servers.get(name)
    }
}

impl ServerConfig {
    /// ${workspaceFolder}などの変数を展開した実際のコマンドを取得
    ///
    /// # Arguments
    /// * `workspace_folder` - ワークスペースフォルダのパス
    pub fn resolve_command(&self, workspace_folder: Option<&str>) -> String {
        let mut command = self.command.clone();
        if let Some(workspace) = workspace_folder {
            command = command.replace("${workspaceFolder}", workspace);
        }
        command
    }

    /// 変数を展開した引数リストを取得
    pub fn resolve_args(&self, workspace_folder: Option<&str>) -> Vec<String> {
        self.args
            .iter()
            .map(|arg| {
                let mut resolved = arg.clone();
                if let Some(workspace) = workspace_folder {
                    resolved = resolved.replace("${workspaceFolder}", workspace);
                }
                resolved
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_config() {
        let json = r#"
        {
          "servers": {
            "test-server": {
              "type": "stdio",
              "command": "./target/release/server"
            }
          }
        }
        "#;

        let config: McpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert!(config.servers.contains_key("test-server"));

        let server = config.get_server("test-server").unwrap();
        assert_eq!(server.server_type, "stdio");
        assert_eq!(server.command, "./target/release/server");
        assert!(server.args.is_empty());
    }

    #[test]
    fn test_parse_config_with_args_and_env() {
        let json = r#"
        {
          "servers": {
            "test-server": {
              "type": "stdio",
              "command": "cargo",
              "args": ["run", "--", "--debug"],
              "env": {
                "RUST_LOG": "debug"
              }
            }
          }
        }
        "#;

        let config: McpConfig = serde_json::from_str(json).unwrap();
        let server = config.get_server("test-server").unwrap();

        assert_eq!(server.args.len(), 3);
        assert_eq!(server.args[0], "run");
        assert_eq!(server.env.get("RUST_LOG"), Some(&"debug".to_string()));
    }

    #[test]
    fn test_resolve_workspace_folder() {
        let server = ServerConfig {
            server_type: "stdio".to_string(),
            command: "${workspaceFolder}/target/release/app".to_string(),
            args: vec![
                "--config".to_string(),
                "${workspaceFolder}/config.toml".to_string(),
            ],
            env: HashMap::new(),
            env_file: None,
            cwd: None,
        };

        let resolved_command = server.resolve_command(Some("/home/user/project"));
        assert_eq!(resolved_command, "/home/user/project/target/release/app");

        let resolved_args = server.resolve_args(Some("/home/user/project"));
        assert_eq!(resolved_args[1], "/home/user/project/config.toml");
    }
}
