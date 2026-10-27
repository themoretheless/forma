//! Built-in Model Context Protocol (MCP) server for Forma applications (`--mcp` / `forma::mcp`).
//!
//! Exposes a zero-dependency stdio JSON-RPC 2.0 MCP server so AI agents can inspect
//! the `.ui` scene, accessibility tree, click buttons (`first`/`second`), switch
//! UI backends (`egui`/`svet`) and target platforms (`desktop`/`ios`/`android`),
//! render pixel snapshots, and launch desktop or Android Emulator windows.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

/// Runtime state managed by an interactive Forma MCP session.
pub struct McpApp {
    pub runtime: crate::Runtime,
    pub source: String,
    pub template: String,
    pub backend: String,
    pub platform: String,
    pub events: Vec<Value>,
}

impl Default for McpApp {
    fn default() -> Self {
        Self::new()
    }
}

impl McpApp {
    pub fn new() -> Self {
        let source = crate::TWO_BUTTONS_EXAMPLE.to_string();
        let template = crate::BUTTON_COMPONENT.to_string();
        let runtime = crate::Runtime::from_sources(&source, &template)
            .expect("valid built-in TwoButtons.ui");
        Self {
            runtime,
            source,
            template,
            backend: std::env::var("FORMA_BACKEND").unwrap_or_else(|_| "egui".into()),
            platform: std::env::var("FORMA_PLATFORM").unwrap_or_else(|_| "desktop".into()),
            events: Vec::new(),
        }
    }

    pub fn from_runtime(
        runtime: crate::Runtime,
        source: impl Into<String>,
        template: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            source: source.into(),
            template: template.into(),
            backend: std::env::var("FORMA_BACKEND").unwrap_or_else(|_| "egui".into()),
            platform: std::env::var("FORMA_PLATFORM").unwrap_or_else(|_| "desktop".into()),
            events: Vec::new(),
        }
    }

    pub fn controls_json(&self) -> Vec<Value> {
        let count = self.runtime.control_count();
        (0..count)
            .map(|i| {
                let b = self.runtime.control_bounds(i);
                json!({
                    "index": i,
                    "key": self.runtime.control_key(i),
                    "label": self.runtime.control_label(i),
                    "action": self.runtime.control_action(i),
                    "clicks": self.runtime.control_clicks(i),
                    "disabled": self.runtime.control_disabled(i),
                    "focused": self.runtime.control_focused(i),
                    "hovered": self.runtime.control_hovered(i),
                    "bounds": b,
                })
            })
            .collect()
    }

    pub fn status_json(&self) -> Value {
        json!({
            "backend": self.backend,
            "platform": self.platform,
            "width": self.runtime.render_width(),
            "height": self.runtime.render_height(),
            "controlCount": self.runtime.control_count(),
            "totalClicks": self.runtime.clicks(),
            "focusedIndex": self.runtime.focused_index(),
            "controls": self.controls_json(),
            "eventCount": self.events.len(),
        })
    }

    pub fn click_control(&mut self, key_or_index: &str) -> Result<Value, String> {
        let count = self.runtime.control_count();
        let idx = if let Ok(parsed) = key_or_index.parse::<usize>() {
            if parsed >= count {
                return Err(format!("Control index {parsed} out of range (0..{count})"));
            }
            parsed
        } else {
            (0..count)
                .find(|&i| {
                    self.runtime.control_key(i) == key_or_index
                        || self.runtime.control_label(i) == key_or_index
                })
                .ok_or_else(|| format!("Control '{key_or_index}' not found"))?
        };

        let before = self.runtime.control_clicks(idx);
        self.runtime.focus_control(idx);
        self.runtime.activate_control(idx);
        let _ = self.runtime.sync_ui_bindings_native();
        let after = self.runtime.control_clicks(idx);
        let key = self.runtime.control_key(idx);
        let label = self.runtime.control_label(idx);
        let action = self.runtime.control_action(idx);
        let ev = json!({
            "kind": "clicked",
            "index": idx,
            "key": key,
            "label": label,
            "action": action,
            "clicksBefore": before,
            "clicksAfter": after,
            "backend": self.backend,
            "platform": self.platform,
        });
        self.events.push(ev.clone());
        Ok(ev)
    }

    pub fn pointer_event(&mut self, x: f32, y: f32, kind: &str) -> Result<Value, String> {
        let code = match kind {
            "move" | "0" => 0,
            "down" | "press" | "1" => 1,
            "up" | "release" | "2" => 2,
            "cancel" | "leave" | "3" => 3,
            "click" | "tap" => {
                self.runtime.pointer(x, y, 1);
                self.runtime.pointer(x, y, 2);
                let _ = self.runtime.sync_ui_bindings_native();
                let hit = self.runtime.hit_index(x, y);
                let ev = json!({
                    "kind": "tap",
                    "x": x,
                    "y": y,
                    "hitIndex": hit,
                    "totalClicks": self.runtime.clicks(),
                });
                self.events.push(ev.clone());
                return Ok(ev);
            }
            other => return Err(format!("Unknown pointer kind '{other}' (expected move|down|up|cancel|click)")),
        };
        self.runtime.pointer(x, y, code);
        let _ = self.runtime.sync_ui_bindings_native();
        Ok(json!({
            "x": x,
            "y": y,
            "kind": kind,
            "hitIndex": self.runtime.hit_index(x, y),
            "totalClicks": self.runtime.clicks(),
        }))
    }

    pub fn load_ui(&mut self, source: &str, template: Option<&str>) -> Result<Value, String> {
        let tpl = template.unwrap_or(&self.template);
        let next = crate::Runtime::from_sources(source, tpl)?;
        self.runtime = next;
        self.source = source.to_string();
        if let Some(t) = template {
            self.template = t.to_string();
        }
        Ok(self.status_json())
    }

    pub fn render_snapshot(&self, width: Option<u32>, height: Option<u32>, scale: Option<f32>) -> Value {
        let s = scale.unwrap_or(1.0).clamp(0.25, 4.0);
        let w = width.unwrap_or_else(|| (self.runtime.render_width() * s).round().max(1.0) as u32);
        let h = height.unwrap_or_else(|| (self.runtime.render_height() * s).round().max(1.0) as u32);
        let pixels = self.runtime.pixels(w, h, s);
        let non_empty = pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
        // FNV-1a checksum of RGBA buffer for deterministic snapshot assertions
        let mut hash: u64 = 0xcbf29ce484222325;
        for &b in &pixels {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        json!({
            "width": w,
            "height": h,
            "scale": s,
            "byteLength": pixels.len(),
            "opaqueOrTranslucentPixels": non_empty,
            "fnv1a64": format!("{hash:016x}"),
            "backend": self.backend,
            "platform": self.platform,
        })
    }

    pub fn handle_request(&mut self, req: &Value) -> Option<Value> {
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        // JSON-RPC notifications have no "id" and expect no response.
        let id = req.get("id").cloned();
        if method.starts_with("notifications/") || id.is_none() {
            return None;
        }
        let id = id.unwrap_or(Value::Null);
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

        let response = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "forma-app-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tool_definitions()
            })),
            "resources/list" => Ok(json!({
                "resources": [
                    {
                        "uri": "forma://ui/source",
                        "name": "Active .ui markup",
                        "description": "Current .ui markup loaded in the Forma Runtime",
                        "mimeType": "text/plain"
                    },
                    {
                        "uri": "forma://ui/status",
                        "name": "Runtime status & controls",
                        "description": "JSON snapshot of backend, platform, and interactive controls",
                        "mimeType": "application/json"
                    }
                ]
            })),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                match uri {
                    "forma://ui/source" => Ok(json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "text/plain",
                            "text": self.source
                        }]
                    })),
                    "forma://ui/status" => Ok(json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": serde_json::to_string_pretty(&self.status_json()).unwrap_or_default()
                        }]
                    })),
                    _ => Err(format!("Unknown resource URI: {uri}")),
                }
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match self.call_tool(name, &args) {
                    Ok(val) => Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                        }]
                    })),
                    Err(err) => Ok(json!({
                        "isError": true,
                        "content": [{
                            "type": "text",
                            "text": err
                        }]
                    })),
                }
            }
            _ => Err(format!("Method not found: {method}")),
        };

        Some(match response {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            }),
            Err(message) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": message
                }
            }),
        })
    }

    pub fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "ui_status" => Ok(self.status_json()),
            "ui_controls" => Ok(json!({ "controls": self.controls_json() })),
            "ui_accessibility_tree" => {
                let raw = self.runtime.accessibility_tree_json();
                serde_json::from_str(&raw).map_err(|e| e.to_string())
            }
            "ui_click" => {
                let target = args
                    .get("key")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| args.get("index").and_then(Value::as_u64).map(|n| n.to_string()))
                    .ok_or("Provide 'key' (e.g. 'first' or 'second') or 'index' (0, 1)")?;
                self.click_control(&target)
            }
            "ui_pointer" => {
                let x = args.get("x").and_then(Value::as_f64).ok_or("Missing numeric 'x'")? as f32;
                let y = args.get("y").and_then(Value::as_f64).ok_or("Missing numeric 'y'")? as f32;
                let kind = args.get("kind").and_then(Value::as_str).unwrap_or("click");
                self.pointer_event(x, y, kind)
            }
            "ui_set_backend" => {
                let b = args.get("backend").and_then(Value::as_str).unwrap_or("");
                if !matches!(b, "egui" | "svet" | "ask") {
                    return Err("backend must be 'egui', 'svet', or 'ask'".into());
                }
                self.backend = b.to_string();
                self.events.push(json!({"kind": "backend_changed", "backend": b}));
                Ok(self.status_json())
            }
            "ui_set_platform" => {
                let p = args.get("platform").and_then(Value::as_str).unwrap_or("");
                if !matches!(p, "desktop" | "ios" | "android") {
                    return Err("platform must be 'desktop', 'ios', or 'android'".into());
                }
                self.platform = p.to_string();
                self.events.push(json!({"kind": "platform_changed", "platform": p}));
                Ok(self.status_json())
            }
            "ui_source_read" => Ok(json!({
                "source": self.source,
                "template": self.template
            })),
            "ui_source_write" => {
                let source = args
                    .get("source")
                    .and_then(Value::as_str)
                    .ok_or("Missing 'source' .ui markup")?;
                let template = args.get("template").and_then(Value::as_str);
                self.load_ui(source, template)
            }
            "ui_render_snapshot" => {
                let w = args.get("width").and_then(Value::as_u64).map(|v| v as u32);
                let h = args.get("height").and_then(Value::as_u64).map(|v| v as u32);
                let s = args.get("scale").and_then(Value::as_f64).map(|v| v as f32);
                Ok(self.render_snapshot(w, h, s))
            }
            "ui_events" => {
                let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(false);
                let out = self.events.clone();
                if clear {
                    self.events.clear();
                }
                Ok(json!({ "events": out }))
            }
            "app_launch" => {
                let backend = args
                    .get("backend")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.backend)
                    .to_string();
                let platform = args
                    .get("platform")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.platform)
                    .to_string();
                self.backend = backend.clone();
                self.platform = platform.clone();
                launch_target(&backend, &platform)
            }
            _ => Err(format!("Unknown tool: {name}")),
        }
    }
}

fn launch_target(backend: &str, platform: &str) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::process::{Command, Stdio};
        if platform == "android" {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            let script = root.join("scripts/run-android-emulator.sh");
            if script.exists() {
                let flag = match backend {
                    "egui" => "--egui",
                    "svet" => "--svet",
                    _ => "--ask",
                };
                let child = Command::new(&script)
                    .arg(flag)
                    .current_dir(root)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Failed to spawn Android emulator script: {e}"))?;
                return Ok(json!({
                    "launched": "android-emulator",
                    "pid": child.id(),
                    "backend": backend,
                    "platform": platform,
                }));
            }
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.arg("--platform").arg(platform);
        if matches!(backend, "egui" | "svet") {
            cmd.arg("--backend").arg(backend);
        } else {
            cmd.arg("--ask");
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch native window: {e}"))?;
        Ok(json!({
            "launched": "native-window",
            "pid": child.id(),
            "backend": backend,
            "platform": platform,
        }))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (backend, platform);
        Err("app_launch is only available on native targets".into())
    }
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "ui_status",
            "description": "Read current Forma UI runtime status: active backend (egui/svet), platform (desktop/ios/android), viewport dimensions, controls, and click counts",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "ui_controls",
            "description": "List all controls in the active .ui scene with their keys, labels, actions, bounds, and click counts",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "ui_accessibility_tree",
            "description": "Read the semantic AccessKit-compatible accessibility tree of the active .ui scene",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "ui_click",
            "description": "Activate/click a button in the .ui scene by its key (e.g. 'first' or 'second'), label, or zero-based index",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "Control key (e.g. 'first', 'second') or label" },
                    "index": { "type": "integer", "description": "Zero-based control index" }
                }
            }
        }),
        json!({
            "name": "ui_pointer",
            "description": "Send a pointer or touch event (move, down, up, cancel, click) at logical coordinates (x, y)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "kind": { "type": "string", "enum": ["move", "down", "up", "cancel", "click"] }
                },
                "required": ["x", "y"]
            }
        }),
        json!({
            "name": "ui_set_backend",
            "description": "Switch the UI execution backend between 'egui' (Immediate-Mode), 'svet' (Reactive GPU/UMA), or 'ask'",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "backend": { "type": "string", "enum": ["egui", "svet", "ask"] }
                },
                "required": ["backend"]
            }
        }),
        json!({
            "name": "ui_set_platform",
            "description": "Switch the target platform between 'desktop', 'ios', and 'android'",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "platform": { "type": "string", "enum": ["desktop", "ios", "android"] }
                },
                "required": ["platform"]
            }
        }),
        json!({
            "name": "ui_source_read",
            "description": "Read the active .ui markup and component template",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "ui_source_write",
            "description": "Hot-reload new .ui markup into the Forma Runtime",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": ".ui markup source" },
                    "template": { "type": "string", "description": "Optional component template .ui source" }
                },
                "required": ["source"]
            }
        }),
        json!({
            "name": "ui_render_snapshot",
            "description": "Render the current .ui scene to an RGBA pixel buffer and return dimensions, coverage, and deterministic FNV-1a checksum",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "width": { "type": "integer" },
                    "height": { "type": "integer" },
                    "scale": { "type": "number" }
                }
            }
        }),
        json!({
            "name": "ui_events",
            "description": "Read recorded UI interaction events (button clicks, backend/platform switches)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clear": { "type": "boolean" }
                }
            }
        }),
        json!({
            "name": "app_launch",
            "description": "Spawn a native window (desktop/ios) or launch the APK in the Android Emulator with the chosen backend (egui/svet)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "backend": { "type": "string", "enum": ["egui", "svet", "ask"] },
                    "platform": { "type": "string", "enum": ["desktop", "ios", "android"] }
                }
            }
        }),
    ]
}

/// Runs the stdio JSON-RPC 2.0 MCP server loop until EOF.
pub fn serve_stdio(mut app: McpApp) -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                writeln!(stdout, "{err}").map_err(|e| e.to_string())?;
                stdout.flush().ok();
                continue;
            }
        };
        if let Some(resp) = app.handle_request(&req) {
            writeln!(stdout, "{resp}").map_err(|e| e.to_string())?;
            stdout.flush().ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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

        let click2 = app.call_tool("ui_click", &json!({"key": "second"})).unwrap();
        assert_eq!(click2["clicksAfter"], 1);
        assert_eq!(click2["action"], "actions.second");

        let status = app.call_tool("ui_set_backend", &json!({"backend": "svet"})).unwrap();
        assert_eq!(status["backend"], "svet");
        assert_eq!(status["totalClicks"], 2);

        let status = app.call_tool("ui_set_platform", &json!({"platform": "android"})).unwrap();
        assert_eq!(status["platform"], "android");

        let snap = app.call_tool("ui_render_snapshot", &json!({})).unwrap();
        assert!(snap["opaqueOrTranslucentPixels"].as_u64().unwrap() > 100);
    }
}
