//! Native clipboard adapters shared by both window hosts.
use crate::Runtime;

/// Preserve Latin-layout shortcuts; use the physical Latin key only when a
/// command arrives as a non-ASCII layout character (for example Cyrillic).
pub(crate) fn command_key<'a>(
    physical: &winit::keyboard::PhysicalKey,
    logical: &'a str,
) -> &'a str {
    if logical.is_ascii() {
        return logical;
    }
    use winit::keyboard::{KeyCode, PhysicalKey};
    match physical {
        PhysicalKey::Code(KeyCode::KeyA) => "a",
        PhysicalKey::Code(KeyCode::KeyC) => "c",
        PhysicalKey::Code(KeyCode::KeyX) => "x",
        PhysicalKey::Code(KeyCode::KeyV) => "v",
        PhysicalKey::Code(KeyCode::KeyZ) => "z",
        PhysicalKey::Code(KeyCode::KeyY) => "y",
        _ => logical,
    }
}

/// Handle a clipboard command. Read/write failures leave editor contents intact.
pub(crate) fn handle_key(runtime: &mut Runtime, key: &str) -> bool {
    handle_with(runtime, key, clipboard_read, clipboard_write)
}

fn handle_with(
    runtime: &mut Runtime,
    key: &str,
    mut read: impl FnMut() -> Result<String, std::io::Error>,
    mut write: impl FnMut(&str) -> Result<(), std::io::Error>,
) -> bool {
    if !runtime.text_editing() {
        return false;
    }
    match key.to_ascii_lowercase().as_str() {
        "v" => {
            if let Ok(text) = read() {
                if !text.is_empty() {
                    runtime.text_insert(&text);
                }
            }
        }
        "c" | "x" => {
            let text = runtime.text_selected();
            if !text.is_empty() && write(&text).is_ok() && key.eq_ignore_ascii_case("x") {
                runtime.text_insert("");
            }
        }
        _ => return false,
    }
    true
}

fn decode_clipboard(bytes: Vec<u8>) -> Result<String, std::io::Error> {
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

const CLIPBOARD_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(450);

fn wait_with_timeout(
    child: &mut std::process::Child,
) -> Result<std::process::ExitStatus, std::io::Error> {
    let deadline = std::time::Instant::now() + CLIPBOARD_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Clipboard helper timed out",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn read_clipboard_child(mut child: std::process::Child) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("Clipboard stdout unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).map(|_| buf)
    });
    let status = wait_with_timeout(&mut child)?;
    if !status.success() {
        return Err(std::io::Error::other("Clipboard read failed"));
    }
    reader
        .join()
        .map_err(|_| std::io::Error::other("Clipboard reader thread panicked"))?
}

// Always close stdin and reap the child, including after a broken pipe or timeout.
fn write_clipboard_child(
    child: &mut std::process::Child,
    text: &str,
) -> Result<(), std::io::Error> {
    use std::io::Write;
    let written = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(text.as_bytes()),
        None => Err(std::io::Error::other("Clipboard stdin unavailable")),
    };
    let status = wait_with_timeout(child);
    written?;
    if status?.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("Clipboard write failed"))
    }
}

#[cfg(target_os = "macos")]
fn clipboard_read() -> Result<String, std::io::Error> {
    let child = std::process::Command::new("/usr/bin/pbpaste")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    decode_clipboard(read_clipboard_child(child)?)
}
#[cfg(target_os = "macos")]
fn clipboard_write(text: &str) -> Result<(), std::io::Error> {
    let mut child = std::process::Command::new("/usr/bin/pbcopy")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    write_clipboard_child(&mut child, text)
}
#[cfg(target_os = "windows")]
fn clipboard_read() -> Result<String, std::io::Error> {
    let child=std::process::Command::new("powershell")
        .args(["-NoProfile","-NonInteractive","-Command","[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Get-Clipboard -Raw"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut s = decode_clipboard(read_clipboard_child(child)?)?;
    if s.ends_with("\r\n") {
        s.truncate(s.len() - 2);
    } else if s.ends_with('\n') {
        s.pop();
    }
    Ok(s)
}
#[cfg(target_os = "windows")]
fn clipboard_write(text: &str) -> Result<(), std::io::Error> {
    let mut child = std::process::Command::new("clip")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    write_clipboard_child(&mut child, text)
}
#[cfg(all(unix, not(target_os = "macos")))]
fn clipboard_read() -> Result<String, std::io::Error> {
    for (cmd, args) in [
        ("wl-paste", &["--no-newline"][..]),
        ("xclip", &["-selection", "clipboard", "-o"][..]),
        ("xsel", &["--clipboard", "--output"][..]),
    ] {
        if let Ok(child) = std::process::Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Ok(bytes) = read_clipboard_child(child) {
                return decode_clipboard(bytes);
            }
        }
    }
    Err(std::io::Error::other(
        "Native clipboard adapter unavailable",
    ))
}
#[cfg(all(unix, not(target_os = "macos")))]
fn clipboard_write(text: &str) -> Result<(), std::io::Error> {
    for (cmd, args) in [
        ("wl-copy", &[][..]),
        ("xclip", &["-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--input"][..]),
    ] {
        if let Ok(mut child) = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if write_clipboard_child(&mut child, text).is_ok() {
                return Ok(());
            }
        }
    }
    Err(std::io::Error::other(
        "Native clipboard adapter unavailable",
    ))
}
#[cfg(not(any(unix, target_os = "windows")))]
fn clipboard_read() -> Result<String, std::io::Error> {
    Err(std::io::Error::other(
        "Native clipboard adapter unavailable",
    ))
}
#[cfg(not(any(unix, target_os = "windows")))]
fn clipboard_write(_: &str) -> Result<(), std::io::Error> {
    Err(std::io::Error::other(
        "Native clipboard adapter unavailable",
    ))
}

#[cfg(test)]
mod tests;
