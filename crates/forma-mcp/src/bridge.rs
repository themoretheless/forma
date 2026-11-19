use serde::Deserialize;
use serde_json::{Value, json};
use std::error::Error;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

const BRIDGE_PATH: &str = "/__forma_mcp";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
struct Descriptor {
    url: String,
    token: String,
}

#[derive(Debug, Clone)]
enum DescriptorSource {
    Explicit(PathBuf),
    Default { cwd: PathBuf, workspace: PathBuf },
}

#[derive(Debug)]
pub struct Bridge {
    source: DescriptorSource,
    timeout: Duration,
}

#[derive(Debug, PartialEq, Eq)]
struct Endpoint {
    host: String,
    port: u16,
    host_header: String,
}

impl Bridge {
    pub fn new(descriptor_path: PathBuf) -> Self {
        Self::with_timeout(descriptor_path, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(descriptor_path: PathBuf, timeout: Duration) -> Self {
        Self {
            source: DescriptorSource::Explicit(descriptor_path),
            timeout,
        }
    }

    pub fn from_environment() -> Result<Self, Box<dyn Error>> {
        let source = match std::env::var_os("FORMA_MCP_BRIDGE") {
            Some(path) => DescriptorSource::Explicit(PathBuf::from(path)),
            None => DescriptorSource::Default {
                cwd: std::env::current_dir()?.join(".forma").join("bridge.json"),
                workspace: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.forma/bridge.json"),
            },
        };
        Ok(Self {
            source,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    fn resolve_descriptor_path(&self) -> &Path {
        match &self.source {
            DescriptorSource::Explicit(path) => path,
            DescriptorSource::Default { cwd, workspace } => {
                if cwd.exists() {
                    cwd
                } else if workspace.exists() {
                    workspace
                } else {
                    cwd
                }
            }
        }
    }

    pub fn call(&self, command: &str, args: Value) -> Result<Value, Box<dyn Error>> {
        let descriptor_path = self.resolve_descriptor_path();
        let bytes = std::fs::read(descriptor_path).map_err(|error| {
            format!(
                "Cannot read IDE bridge descriptor {}: {error}",
                descriptor_path.display()
            )
        })?;
        let descriptor: Descriptor = serde_json::from_slice(&bytes)?;
        let Some(endpoint) = parse_bridge_url(&descriptor.url) else {
            return Err("Invalid bridge address".into());
        };
        if descriptor.token.contains(['\r', '\n']) {
            return Err("Invalid bridge token".into());
        }

        let addrs = resolve_loopback_addrs(&endpoint)?;
        let mut stream = connect_with_timeout(&addrs, self.timeout)?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;

        let body = serde_json::to_vec(&json!({ "command": command, "args": args }))?;
        let header = format!(
            "POST {BRIDGE_PATH} HTTP/1.1\r\n\
             Host: {}\r\n\
             Authorization: Bearer {}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            endpoint.host_header,
            descriptor.token,
            body.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(&body)?;
        stream.flush()?;

        let (status, response_body) = read_http_response(&mut stream)?;
        let result: Value = match serde_json::from_slice(&response_body) {
            Ok(value) => value,
            Err(error) if !(200..300).contains(&status) => {
                return Err(format!("IDE bridge returned HTTP {status}: {error}").into());
            }
            Err(error) => return Err(error.into()),
        };

        if !(200..300).contains(&status) {
            return Err(bridge_error(&result).into());
        }
        if result
            .get("error")
            .is_some_and(|error| !error.is_null() && error != "")
        {
            return Err(bridge_error(&result).into());
        }
        Ok(result.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn bridge_error(result: &Value) -> String {
    result
        .get("error")
        .and_then(Value::as_str)
        .filter(|error| !error.is_empty())
        .unwrap_or("Bridge failed")
        .to_owned()
}

pub fn is_bridge_url(address: &str) -> bool {
    parse_bridge_url(address).is_some()
}

fn parse_bridge_url(address: &str) -> Option<Endpoint> {
    let without_scheme = address.strip_prefix("http://")?;
    let authority = without_scheme.strip_suffix(BRIDGE_PATH)?;
    if authority.contains(['/', '?', '#', '@']) || authority.chars().any(char::is_whitespace) {
        return None;
    }

    let (host, rest) = ["[::1]", "127.0.0.1", "localhost"]
        .into_iter()
        .find_map(|candidate| {
            authority
                .strip_prefix(candidate)
                .map(|tail| (candidate, tail))
        })?;

    let (port, host_header) = if rest.is_empty() {
        (80, host.to_owned())
    } else {
        let port_str = rest.strip_prefix(':')?;
        let port: u16 = port_str.parse().ok()?;
        if port == 80 || port.to_string() != port_str {
            return None;
        }
        (port, format!("{host}:{port}"))
    };

    if format!("http://{host_header}{BRIDGE_PATH}") != address {
        return None;
    }

    Some(Endpoint {
        host: host.to_owned(),
        port,
        host_header,
    })
}

fn resolve_loopback_addrs(endpoint: &Endpoint) -> Result<Vec<SocketAddr>, Box<dyn Error>> {
    match endpoint.host.as_str() {
        "127.0.0.1" => Ok(vec![SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            endpoint.port,
        )]),
        "[::1]" => Ok(vec![SocketAddr::new(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            endpoint.port,
        )]),
        "localhost" => {
            let addrs: Vec<SocketAddr> = ("localhost", endpoint.port)
                .to_socket_addrs()?
                .filter(|addr| addr.ip().is_loopback())
                .collect();
            if addrs.is_empty() {
                return Err("localhost did not resolve to a loopback address".into());
            }
            Ok(addrs)
        }
        _ => Err("Invalid bridge address".into()),
    }
}

fn connect_with_timeout(
    addrs: &[SocketAddr],
    timeout: Duration,
) -> Result<TcpStream, Box<dyn Error>> {
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(error.into()),
        None => Err("No loopback bridge addresses available".into()),
    }
}

fn read_http_response(stream: &mut TcpStream) -> Result<(u16, Vec<u8>), Box<dyn Error>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    let header_end = loop {
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
        let read = read_chunk(stream, &mut chunk)?;
        if read == 0 {
            return Err("Incomplete HTTP response from IDE bridge".into());
        }
        if buf.len() + read > MAX_RESPONSE_BYTES {
            return Err("IDE bridge response exceeded maximum size".into());
        }
        buf.extend_from_slice(&chunk[..read]);
    };

    let header_str = std::str::from_utf8(&buf[..header_end])
        .map_err(|_| "IDE bridge returned non-UTF-8 HTTP headers")?;
    let mut lines = header_str.split("\r\n");
    let status_line = lines
        .next()
        .ok_or("Missing HTTP status line from IDE bridge")?;
    let status = parse_status_code(status_line)?;

    let mut content_length: Option<usize> = None;
    let mut is_chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            let parsed: usize = value
                .parse()
                .map_err(|_| "Invalid Content-Length header from IDE bridge")?;
            if parsed > MAX_RESPONSE_BYTES {
                return Err("IDE bridge response exceeded maximum size".into());
            }
            content_length = Some(parsed);
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            is_chunked = true;
        }
    }

    let mut body_buf = buf.split_off(header_end + 4);

    if is_chunked {
        loop {
            if let Some(decoded) = try_decode_chunked(&body_buf)? {
                return Ok((status, decoded));
            }
            let read = read_chunk(stream, &mut chunk)?;
            if read == 0 {
                return Err("Incomplete chunked HTTP response from IDE bridge".into());
            }
            if body_buf.len() + read > MAX_RESPONSE_BYTES {
                return Err("IDE bridge response exceeded maximum size".into());
            }
            body_buf.extend_from_slice(&chunk[..read]);
        }
    }

    if let Some(expected_len) = content_length {
        while body_buf.len() < expected_len {
            let read = read_chunk(stream, &mut chunk)?;
            if read == 0 {
                return Err("Truncated HTTP response body from IDE bridge".into());
            }
            if body_buf.len() + read > MAX_RESPONSE_BYTES {
                return Err("IDE bridge response exceeded maximum size".into());
            }
            body_buf.extend_from_slice(&chunk[..read]);
        }
        body_buf.truncate(expected_len);
        return Ok((status, body_buf));
    }

    loop {
        let read = read_chunk(stream, &mut chunk)?;
        if read == 0 {
            break;
        }
        if body_buf.len() + read > MAX_RESPONSE_BYTES {
            return Err("IDE bridge response exceeded maximum size".into());
        }
        body_buf.extend_from_slice(&chunk[..read]);
    }
    Ok((status, body_buf))
}

fn read_chunk(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match stream.read(buf) {
            Ok(n) => return Ok(n),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_status_code(status_line: &str) -> Result<u16, Box<dyn Error>> {
    let mut parts = status_line.split_whitespace();
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err("Invalid HTTP status line from IDE bridge".into());
    }
    let code_str = parts
        .next()
        .ok_or("Missing HTTP status code from IDE bridge")?;
    let code: u16 = code_str
        .parse()
        .map_err(|_| "Invalid HTTP status code from IDE bridge")?;
    Ok(code)
}

fn try_decode_chunked(raw: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut pos = 0;

    loop {
        let Some(line_rel) = raw[pos..].windows(2).position(|w| w == b"\r\n") else {
            return Ok(None);
        };
        let size_line = std::str::from_utf8(&raw[pos..pos + line_rel])
            .map_err(|_| "Invalid chunk size header from IDE bridge")?;
        let hex_part = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(hex_part, 16)
            .map_err(|_| "Invalid chunk size from IDE bridge")?;
        pos += line_rel + 2;

        if size == 0 {
            loop {
                let Some(trailer_rel) = raw[pos..].windows(2).position(|w| w == b"\r\n") else {
                    return Ok(None);
                };
                pos += trailer_rel + 2;
                if trailer_rel == 0 {
                    return Ok(Some(out));
                }
            }
        }

        if raw.len() < pos + size + 2 {
            return Ok(None);
        }
        if out.len() + size > MAX_RESPONSE_BYTES {
            return Err("IDE bridge response exceeded maximum size".into());
        }
        out.extend_from_slice(&raw[pos..pos + size]);
        pos += size;
        if &raw[pos..pos + 2] != b"\r\n" {
            return Err("Malformed chunk terminator from IDE bridge".into());
        }
        pos += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn accepts_only_canonical_loopback_bridge_urls() {
        for address in [
            "http://127.0.0.1:5173/__forma_mcp",
            "http://localhost:4000/__forma_mcp",
            "http://[::1]:4000/__forma_mcp",
            "http://127.0.0.1/__forma_mcp",
        ] {
            assert!(is_bridge_url(address), "{address}");
        }
        for address in [
            "https://127.0.0.1:5173/__forma_mcp",
            "http://127.0.0.1:80/__forma_mcp",
            "http://127.0.0.1:05173/__forma_mcp",
            "http://example.com:5173/__forma_mcp",
            "http://127.0.0.1.example.com:5173/__forma_mcp",
            "http://user@127.0.0.1:5173/__forma_mcp",
            "http://127.0.0.1:5173/__forma_mcp/../other",
            "http://127.0.0.1:5173/__forma_mcp?next=http://example.com",
            "http://127.0.0.1:5173/extra/__forma_mcp",
            "http://127.0.0.1:5173/other",
            "not a url",
        ] {
            assert!(!is_bridge_url(address), "{address}");
        }
    }

    #[test]
    fn reads_content_length_response_even_without_eof() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!(
            "forma-mcp-bridge-cl-{}-{}",
            std::process::id(),
            port
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let descriptor_path = dir.join("bridge.json");
        std::fs::write(
            &descriptor_path,
            format!(r#"{{"url":"http://127.0.0.1:{port}/__forma_mcp","token":"secret-tok"}}"#),
        )
        .unwrap();

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 1024];
            let _ = sock.read(&mut req_buf).unwrap();
            let body = br#"{"result":{"ok":true}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
            sock.write_all(body).unwrap();
            sock.flush().unwrap();
            // Hold connection open until client finishes reading to verify immediate Content-Length completion.
            let _ = done_rx.recv();
        });

        let bridge = Bridge::with_timeout(descriptor_path, Duration::from_secs(2));
        let value = bridge.call("ide_status", json!({})).unwrap();
        assert_eq!(value, json!({ "ok": true }));
        let _ = done_tx.send(());
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_chunked_transfer_encoding_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!(
            "forma-mcp-bridge-ch-{}-{}",
            std::process::id(),
            port
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let descriptor_path = dir.join("bridge.json");
        std::fs::write(
            &descriptor_path,
            format!(r#"{{"url":"http://127.0.0.1:{port}/__forma_mcp","token":"secret-tok"}}"#),
        )
        .unwrap();

        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req_buf = [0u8; 1024];
            let _ = sock.read(&mut req_buf).unwrap();
            let payload = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: application/json\r\n",
                "Transfer-Encoding: chunked\r\n\r\n",
                "b\r\n",
                r#"{"result":{"#,
                "\r\n",
                "8\r\n",
                r#""n":42}}"#,
                "\r\n",
                "0\r\n\r\n",
            );
            sock.write_all(payload.as_bytes()).unwrap();
            sock.flush().unwrap();
        });

        let bridge = Bridge::with_timeout(descriptor_path, Duration::from_secs(2));
        let value = bridge.call("ide_status", json!({})).unwrap();
        assert_eq!(value, json!({ "n": 42 }));
        handle.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
