#!/bin/bash
# モックMCPサーバー - テスト用の簡易実装
# Model Context Protocol (MCP) の基本的な応答を返すモックサーバー

# JSONRPCメッセージを読み取り、適切なレスポンスを返す

# 初期化メッセージへの応答
function send_initialize_response() {
    cat <<EOF
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"mock-mcp-server","version":"0.1.0"}}}
EOF
}

# ツール一覧への応答
function send_list_tools_response() {
    cat <<EOF
{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"test_tool","description":"テスト用のツール","inputSchema":{"type":"object","properties":{"test_param":{"type":"string","description":"テストパラメータ"}}}}]}}
EOF
}

# リソース一覧への応答
function send_list_resources_response() {
    cat <<EOF
{"jsonrpc":"2.0","id":3,"result":{"resources":[{"uri":"test://resource","name":"Test Resource","description":"テスト用リソース","mimeType":"text/plain"}]}}
EOF
}

# プロンプト一覧への応答
function send_list_prompts_response() {
    cat <<EOF
{"jsonrpc":"2.0","id":4,"result":{"prompts":[{"name":"test_prompt","description":"テスト用プロンプト"}]}}
EOF
}

# メイン処理ループ
while IFS= read -r line; do
    # 空行はスキップ
    if [ -z "$line" ]; then
        continue
    fi
    
    # デバッグ用（標準エラー出力に記録）
    echo "Received: $line" >&2
    
    # メソッド名を抽出して適切なレスポンスを返す
    if echo "$line" | grep -q '"method":"initialize"'; then
        send_initialize_response
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        send_list_tools_response
    elif echo "$line" | grep -q '"method":"resources/list"'; then
        send_list_resources_response
    elif echo "$line" | grep -q '"method":"prompts/list"'; then
        send_list_prompts_response
    elif echo "$line" | grep -q '"method":"initialized"'; then
        # initializedは通知なのでレスポンス不要
        continue
    fi
done
