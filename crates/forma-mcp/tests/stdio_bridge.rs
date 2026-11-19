use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct MockBridge {
    addr: std::net::SocketAddr,
    recorded: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MockBridge {
    fn start(token: &'static str, return_503_for_ide_status: Arc<AtomicBool>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recorded_clone = Arc::clone(&recorded);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let expected_host = format!("127.0.0.1:{}", addr.port());
        let expected_auth = format!("Bearer {token}");

        let handle = std::thread::spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else {
                    break;
                };
                if stop_clone.load(Ordering::SeqCst) {
                    break;
                }
                let Some((headers, body)) = read_http_request(&mut stream) else {
                    continue;
                };
                assert_eq!(
                    headers.get("host").map(String::as_str),
                    Some(expected_host.as_str())
                );
                assert_eq!(
                    headers.get("authorization").map(String::as_str),
                    Some(expected_auth.as_str())
                );
                assert!(
                    !headers.contains_key("origin"),
                    "MCP bridge requests must not send Origin header"
                );

                let payload: Value = serde_json::from_slice(&body).unwrap();
                recorded_clone.lock().unwrap().push(payload.clone());

                let command = payload["command"].as_str().unwrap_or("");
                let (status_line, resp_body) = if command == "ide_status"
                    && return_503_for_ide_status.load(Ordering::SeqCst)
                {
                    (
                        "HTTP/1.1 503 Service Unavailable",
                        json!({ "error": "Open the IDE browser tab first" }),
                    )
                } else if command == "file_read" {
                    let path = payload["args"]["path"].as_str().unwrap_or("");
                    (
                        "HTTP/1.1 200 OK",
                        json!({
                            "result": {
                                "path": path,
                                "content": "component Demo { Text { text: \"hi\" } }"
                            }
                        }),
                    )
                } else {
                    ("HTTP/1.1 200 OK", json!({ "result": { "ok": true } }))
                };

                let encoded = serde_json::to_vec(&resp_body).unwrap();
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    encoded.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(&encoded).unwrap();
                stream.flush().unwrap();
            }
        });

        Self {
            addr,
            recorded,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for MockBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> Option<(HashMap<String, String>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    let head = std::str::from_utf8(&buf[..header_end]).unwrap();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap();
    assert!(
        request_line.starts_with("POST /__forma_mcp HTTP/1."),
        "unexpected request line: {request_line}"
    );

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }

    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_length);
    Some((headers, body))
}

struct McpProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl McpProcess {
    fn spawn(bridge_path: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_forma-mcp"))
            .env("FORMA_MCP_BRIDGE", bridge_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin: Some(stdin),
            stdout,
        }
    }

    fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    fn send(&mut self, msg: &Value) {
        self.send_raw(&serde_json::to_string(msg).unwrap());
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "unexpected EOF from forma-mcp");
        serde_json::from_str(&line).unwrap()
    }

    fn finish(mut self) -> std::process::ExitStatus {
        self.stdin.take();
        self.child.wait().unwrap()
    }
}

#[test]
fn stdio_server_unifies_studio_bridge_and_runtime_tools_end_to_end() {
    const TOKEN: &str = "integration-token-123";
    let dir = TempDir::new("forma-mcp-e2e");
    let bridge_path = dir.0.join("bridge.json");
    let return_503 = Arc::new(AtomicBool::new(false));
    let mock = MockBridge::start(TOKEN, Arc::clone(&return_503));
    let descriptor_json = format!(
        r#"{{"url":"http://127.0.0.1:{}/__forma_mcp","token":"{TOKEN}"}}"#,
        mock.addr.port()
    );
    std::fs::write(&bridge_path, &descriptor_json).unwrap();

    let mut proc = McpProcess::spawn(&bridge_path);

    // 1. Initialize handshake -> notifications/initialized (no reply) -> ping ({}).
    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "rust-test", "version": "1.0.0" }
        }
    }));
    let init = proc.recv();
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["serverInfo"]["name"], "forma");

    proc.send(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "ping"
    }));
    let ping = proc.recv();
    assert_eq!(ping["id"], 2);
    assert_eq!(ping["result"], json!({}));

    // 2. tools/list (57 unified tools) and resources/list + resources/read (all 3 resources).
    proc.send(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }));
    let tools_resp = proc.recv();
    let tools = tools_resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 57);
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert!(tool["annotations"]["readOnlyHint"].is_boolean());
        assert!(tool["annotations"]["destructiveHint"].is_boolean());
    }

    proc.send(&json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }));
    let res_list = proc.recv();
    let resources = res_list["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 3);
    assert_eq!(resources[0]["uri"], "forma://language");
    assert_eq!(resources[1]["uri"], "forma://ui/source");
    assert_eq!(resources[2]["uri"], "forma://ui/status");

    for (req_id, uri, needle) in [
        (5, "forma://language", "Forma"),
        (6, "forma://ui/source", "component"),
        (7, "forma://ui/status", "controlCount"),
    ] {
        proc.send(&json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "resources/read",
            "params": { "uri": uri }
        }));
        let res_read = proc.recv();
        assert!(
            res_read["result"]["contents"][0]["text"]
                .as_str()
                .unwrap()
                .contains(needle),
            "{uri} must contain {needle}"
        );
    }

    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "resources/read",
        "params": { "uri": "forma://missing" }
    }));
    let res_err = proc.recv();
    assert_eq!(res_err["error"]["code"], -32602);

    // 3. Valid Studio tool call, invalid arguments (must not reach HTTP bridge), and unknown tool.
    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/call",
        "params": { "name": "file_read", "arguments": { "path": "ui/Demo.ui" } }
    }));
    let read_resp = proc.recv();
    assert_eq!(read_resp["result"]["isError"], false);
    let parsed_tool: Value =
        serde_json::from_str(read_resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed_tool["path"], "ui/Demo.ui");
    assert_eq!(mock.recorded.lock().unwrap().len(), 1);

    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "tools/call",
        "params": { "name": "file_read", "arguments": { "path": "", "extra": 1 } }
    }));
    let invalid_args = proc.recv();
    assert_eq!(invalid_args["result"]["isError"], true);
    assert_eq!(mock.recorded.lock().unwrap().len(), 1);

    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": { "name": "nonexistent_tool", "arguments": {} }
    }));
    let unknown_tool = proc.recv();
    assert_eq!(unknown_tool["error"]["code"], -32602);

    // 4. Unavailable Studio (503 + missing bridge.json) vs offline headless Runtime tools.
    return_503.store(true, Ordering::SeqCst);
    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "tools/call",
        "params": { "name": "ide_status", "arguments": {} }
    }));
    let unavailable_tab = proc.recv();
    assert_eq!(unavailable_tab["result"]["isError"], true);
    assert!(
        unavailable_tab["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Open the IDE browser tab first")
    );

    std::fs::remove_file(&bridge_path).unwrap();
    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 13,
        "method": "tools/call",
        "params": { "name": "ide_status", "arguments": {} }
    }));
    let missing_desc = proc.recv();
    assert_eq!(missing_desc["result"]["isError"], true);
    assert!(
        missing_desc["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Cannot read IDE bridge descriptor")
    );

    // Runtime tools still execute cleanly offline without bridge.json.
    for (req_id, tool_name, arguments) in [
        (14, "ui_status", json!({})),
        (15, "ui_click", json!({ "key": "first" })),
        (16, "ui_render_snapshot", json!({})),
        (17, "ui_events", json!({})),
    ] {
        proc.send(&json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "tools/call",
            "params": { "name": tool_name, "arguments": arguments }
        }));
        let runtime_resp = proc.recv();
        assert_eq!(
            runtime_resp["result"]["isError"], false,
            "{tool_name} failed"
        );
        let parsed: Value = serde_json::from_str(
            runtime_resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        match tool_name {
            "ui_status" => assert_eq!(parsed["controlCount"], 2),
            "ui_click" => assert_eq!(parsed["clicksAfter"], 1),
            "ui_render_snapshot" => {
                assert!(parsed["opaqueOrTranslucentPixels"].as_u64().unwrap() > 100);
            }
            "ui_events" => assert_eq!(parsed["events"].as_array().unwrap().len(), 1),
            _ => unreachable!(),
        }
    }

    // Restore descriptor and verify Studio bridge calls recover too.
    std::fs::write(&bridge_path, &descriptor_json).unwrap();
    return_503.store(false, Ordering::SeqCst);
    proc.send(&json!({
        "jsonrpc": "2.0",
        "id": 18,
        "method": "tools/call",
        "params": { "name": "ide_status", "arguments": {} }
    }));
    let recovered = proc.recv();
    assert_eq!(recovered["result"]["isError"], false);

    // 5. Malformed JSON (-32700 without id) and invalid request (42 and missing method, -32600 without id).
    proc.send_raw("{not valid json");
    let parse_err = proc.recv();
    assert_eq!(parse_err["error"]["code"], -32700);
    assert!(parse_err.get("id").is_none());

    proc.send_raw("42");
    let non_obj_err = proc.recv();
    assert_eq!(non_obj_err["error"]["code"], -32600);
    assert!(non_obj_err.get("id").is_none());

    proc.send(&json!({ "jsonrpc": "2.0" }));
    let no_method_err = proc.recv();
    assert_eq!(no_method_err["error"]["code"], -32600);
    assert!(no_method_err.get("id").is_none());

    let status = proc.finish();
    assert!(status.success());
}
