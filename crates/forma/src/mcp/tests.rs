
use super::*;

#[test]
fn mcp_initializes_and_lists_tools() {
    let mut app = McpApp::new();
    let init = app
        .handle_request(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }))
        .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "forma-app-mcp");

    let tools = app
        .handle_request(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "ui_status",
                "arguments": {}
            }
        }))
        .unwrap();
    let text = tools["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["controlCount"], 2);
    assert_eq!(parsed["controls"][0]["key"], "first");
    assert_eq!(parsed["controls"][1]["key"], "second");
}

#[test]
fn mcp_clicks_two_buttons_and_switches_backend_and_platform() {
    let mut app = McpApp::new();
    let click1 = app.call_tool("ui_click", &json!({"key": "first"})).unwrap();
    assert_eq!(click1["clicksAfter"], 1);
    assert_eq!(click1["action"], "actions.first");

    let click2 = app
        .call_tool("ui_click", &json!({"key": "second"}))
        .unwrap();
    assert_eq!(click2["clicksAfter"], 1);
    assert_eq!(click2["action"], "actions.second");

    let status = app
        .call_tool("ui_set_backend", &json!({"backend": "svet"}))
        .unwrap();
    assert_eq!(status["backend"], "svet");
    assert_eq!(status["totalClicks"], 2);

    let status = app
        .call_tool("ui_set_platform", &json!({"platform": "android"}))
        .unwrap();
    assert_eq!(status["platform"], "android");

    let snap = app.call_tool("ui_render_snapshot", &json!({})).unwrap();
    assert!(snap["opaqueOrTranslucentPixels"].as_u64().unwrap() > 100);
}
