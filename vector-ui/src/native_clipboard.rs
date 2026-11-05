//! Native clipboard adapters shared by both window hosts.
use crate::Runtime;

/// Preserve Latin-layout shortcuts; use the physical Latin key only when a
/// command arrives as a non-ASCII layout character (for example Cyrillic).
pub fn command_key<'a>(physical: &winit::keyboard::PhysicalKey, logical: &'a str) -> &'a str {
    if logical.is_ascii() { return logical; }
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
pub fn handle_key(runtime: &mut Runtime, key: &str) -> bool {
    handle_with(runtime, key, clipboard_read, clipboard_write)
}

fn handle_with(
    runtime: &mut Runtime,
    key: &str,
    mut read: impl FnMut() -> Result<String, std::io::Error>,
    mut write: impl FnMut(&str) -> Result<(), std::io::Error>,
) -> bool {
    if !runtime.text_editing() { return false; }
    match key.to_ascii_lowercase().as_str() {
        "v" => { if let Ok(text) = read() { if !text.is_empty() { runtime.text_insert(&text); } } }
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

fn decode_clipboard(bytes: Vec<u8>) -> Result<String,std::io::Error> {
    String::from_utf8(bytes).map_err(|error|std::io::Error::new(std::io::ErrorKind::InvalidData,error))
}

const CLIPBOARD_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(450);

fn wait_with_timeout(child: &mut std::process::Child) -> Result<std::process::ExitStatus, std::io::Error> {
    let deadline = std::time::Instant::now() + CLIPBOARD_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Clipboard helper timed out"));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn read_clipboard_child(mut child: std::process::Child) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    let mut stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("Clipboard stdout unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).map(|_| buf)
    });
    let status = wait_with_timeout(&mut child)?;
    if !status.success() {
        return Err(std::io::Error::other("Clipboard read failed"));
    }
    reader.join().map_err(|_| std::io::Error::other("Clipboard reader thread panicked"))?
}

// Always close stdin and reap the child, including after a broken pipe or timeout.
fn write_clipboard_child(child: &mut std::process::Child, text: &str) -> Result<(),std::io::Error> {
    use std::io::Write;
    let written=match child.stdin.take() {
        Some(mut stdin)=>stdin.write_all(text.as_bytes()),
        None=>Err(std::io::Error::other("Clipboard stdin unavailable")),
    };
    let status=wait_with_timeout(child);
    written?;
    if status?.success(){Ok(())}else{Err(std::io::Error::other("Clipboard write failed"))}
}

#[cfg(target_os="macos")]
fn clipboard_read()->Result<String,std::io::Error> {
    let child=std::process::Command::new("/usr/bin/pbpaste").stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()?;
    decode_clipboard(read_clipboard_child(child)?)
}
#[cfg(target_os="macos")]
fn clipboard_write(text:&str)->Result<(),std::io::Error> {
    let mut child=std::process::Command::new("/usr/bin/pbcopy").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()?;
    write_clipboard_child(&mut child,text)
}
#[cfg(target_os="windows")]
fn clipboard_read()->Result<String,std::io::Error> {
    let child=std::process::Command::new("powershell")
        .args(["-NoProfile","-NonInteractive","-Command","[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Get-Clipboard -Raw"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let mut s=decode_clipboard(read_clipboard_child(child)?)?;
    if s.ends_with("\r\n"){s.truncate(s.len()-2);}else if s.ends_with('\n'){s.pop();}
    Ok(s)
}
#[cfg(target_os="windows")]
fn clipboard_write(text:&str)->Result<(),std::io::Error> {
    let mut child=std::process::Command::new("clip").stdin(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()?;
    write_clipboard_child(&mut child,text)
}
#[cfg(all(unix,not(target_os="macos")))]
fn clipboard_read()->Result<String,std::io::Error> {
    for (cmd,args) in [("wl-paste",&["--no-newline"][..]),("xclip",&["-selection","clipboard","-o"][..]),("xsel",&["--clipboard","--output"][..])] {
        if let Ok(child)=std::process::Command::new(cmd).args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn() {
            if let Ok(bytes)=read_clipboard_child(child) {
                return decode_clipboard(bytes);
            }
        }
    }
    Err(std::io::Error::other("Native clipboard adapter unavailable"))
}
#[cfg(all(unix,not(target_os="macos")))]
fn clipboard_write(text:&str)->Result<(),std::io::Error> {
    for (cmd,args) in [("wl-copy",&[][..]),("xclip",&["-selection","clipboard"][..]),("xsel",&["--clipboard","--input"][..])] {
        if let Ok(mut child)=std::process::Command::new(cmd).args(args).stdin(std::process::Stdio::piped()).spawn() {
            if write_clipboard_child(&mut child,text).is_ok(){return Ok(());}
        }
    }
    Err(std::io::Error::other("Native clipboard adapter unavailable"))
}
#[cfg(not(any(unix,target_os="windows")))]
fn clipboard_read()->Result<String,std::io::Error> {Err(std::io::Error::other("Native clipboard adapter unavailable"))}
#[cfg(not(any(unix,target_os="windows")))]
fn clipboard_write(_: &str)->Result<(),std::io::Error> {Err(std::io::Error::other("Native clipboard adapter unavailable"))}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Runtime {
        Runtime::from_ui_files(&std::collections::BTreeMap::from([("App.ui".into(),
            "component App { state query: String = 'Привет'; state other: String = 'untouched'; Column { width:260; height:120; TextInput { key:'editor'; width:200; height:32; value <-> state.query; } TextInput { key:'other'; width:200; height:32; value <-> state.other; } } }".into())]), "App.ui", &Default::default()).unwrap()
    }
    #[test]
    fn non_latin_commands_use_physical_fallback_without_remapping_latin_layouts() {
        use winit::keyboard::{KeyCode, PhysicalKey};
        for (code,logical,expected) in [(KeyCode::KeyA,"ф","a"),(KeyCode::KeyC,"с","c"),(KeyCode::KeyX,"ч","x"),(KeyCode::KeyV,"м","v"),(KeyCode::KeyZ,"я","z"),(KeyCode::KeyY,"н","y")] {
            assert_eq!(command_key(&PhysicalKey::Code(code),logical),expected);
        }
        assert_eq!(command_key(&PhysicalKey::Code(KeyCode::KeyA),"q"),"q","Latin AZERTY-like logical key wins");
        assert_eq!(command_key(&PhysicalKey::Code(KeyCode::KeyQ),"a"),"a");
        assert_eq!(command_key(&PhysicalKey::Code(KeyCode::KeyQ),"й"),"й","unknown shortcuts stay unknown");
    }

    #[test]
    fn copy_and_failed_cut_preserve_selection_and_model() {
        let mut runtime=editor();runtime.focus_control(0);runtime.text_key("a",false,true);
        let mut writes=Vec::new();
        assert!(handle_with(&mut runtime,"C",||panic!("copy must not read"),|text| {writes.push(text.to_owned());Ok(())}));
        assert_eq!(writes,vec!["Привет"]);assert_eq!(runtime.text_selected(),"Привет");
        assert!(handle_with(&mut runtime,"x",||panic!("cut must not read"),|_|Err(std::io::Error::other("refused"))));
        assert_eq!(runtime.text_value(0),"Привет");assert_eq!(runtime.text_selected(),"Привет");
        assert!(handle_with(&mut runtime,"v",||Err(std::io::Error::other("refused")),|_|panic!("paste must not write")));
        assert_eq!(runtime.text_value(0),"Привет");assert_eq!(runtime.text_selected(),"Привет");
        runtime.sync_ui_bindings_native().unwrap();assert_eq!(runtime.text_value(1),"untouched");
    }
    #[cfg(unix)]
    #[test]
    fn clipboard_writer_reaps_children_on_success_failure_and_missing_pipe() {
        for (script,piped,large,success) in [("cat >/dev/null",true,false,true),("exit 7",true,true,false),("exit 0",false,false,false)] {
            let mut command=std::process::Command::new("/bin/sh");command.args(["-c",script]);
            if piped{command.stdin(std::process::Stdio::piped());}
            let mut child=command.spawn().unwrap();let pid=child.id() as libc::pid_t;
            let text=if large{"x".repeat(1_000_000)}else{"你好🙂".into()};
            assert_eq!(write_clipboard_child(&mut child,&text).is_ok(),success);
            assert!(child.stdin.is_none());
            let mut status=0;
            assert_eq!(unsafe{libc::waitpid(pid,&mut status,libc::WNOHANG)},-1,"child must already be reaped");
            assert_eq!(std::io::Error::last_os_error().raw_os_error(),Some(libc::ECHILD));
        }
    }
    #[test]
    fn malformed_clipboard_is_rejected_without_replacing_selection() {
        assert_eq!(decode_clipboard("你好🙂".as_bytes().to_vec()).unwrap(),"你好🙂");
        for bytes in [vec![0xff],vec![0xe2,0x82],vec![b'a',0xc0,0xaf]] {
            let mut runtime=editor();runtime.focus_control(0);runtime.text_key("a",false,true);
            let selection=runtime.text_selection_utf16();
            assert!(handle_with(&mut runtime,"v",||decode_clipboard(bytes.clone()),|_|panic!()));
            assert_eq!(runtime.text_value(0),"Привет");
            assert_eq!(runtime.text_selection_utf16(),selection);
            assert_eq!(decode_clipboard(bytes).unwrap_err().kind(),std::io::ErrorKind::InvalidData);
        }
    }
    #[test]
    fn empty_paste_preserves_selection_bindings_and_undo_history() {
        let mut runtime=editor();runtime.focus_control(0);
        runtime.text_insert("!");runtime.text_key("a",false,true);
        let original=runtime.text_value(0);let selection=runtime.text_selection_utf16();
        assert!(handle_with(&mut runtime,"v",||Ok(String::new()),|_|panic!("paste must not write")));
        assert_eq!(runtime.text_value(0),original);
        assert_eq!(runtime.text_selection_utf16(),selection);
        runtime.sync_ui_bindings_native().unwrap();
        let state:serde_json::Value=serde_json::from_str(&runtime.ui_state_json()).unwrap();
        assert_eq!(state["query"],original);assert_eq!(state["other"],"untouched");
        runtime.text_key("z",false,true);assert_eq!(runtime.text_value(0),"Привет");
        runtime.text_key("z",true,true);assert_eq!(runtime.text_value(0),original);
    }
    #[test]
    fn cut_is_undoable_and_unicode_paste_commits_only_selected_editor() {
        let mut runtime=editor();runtime.focus_control(0);runtime.text_key("a",false,true);
        let mut copied=String::new();
        assert!(handle_with(&mut runtime,"x",||panic!(),|text| {copied=text.to_owned();Ok(())}));
        assert_eq!(copied,"Привет");assert_eq!(runtime.text_value(0),"");
        runtime.text_key("z",false,true);assert_eq!(runtime.text_value(0),"Привет");
        runtime.text_key("z",true,true);assert_eq!(runtime.text_value(0),"");
        assert!(handle_with(&mut runtime,"V",||Ok("你好\r\nмир🙂".into()),|_|panic!()));
        assert_eq!(runtime.text_value(0),"你好 мир🙂");runtime.sync_ui_bindings_native().unwrap();
        let model:serde_json::Value=serde_json::from_str(&runtime.ui_state_json()).unwrap();assert_eq!(model["query"],"你好 мир🙂");assert_eq!(model["other"],"untouched");
    }
    #[test]
    fn unavailable_editor_and_empty_copy_do_not_access_clipboard() {
        let mut runtime=editor();
        assert!(!handle_with(&mut runtime,"v",||panic!(),|_|panic!()));
        runtime.focus_control(0);
        assert!(handle_with(&mut runtime,"c",||panic!(),|_|panic!("empty selection must not clear clipboard")));
        assert!(!handle_with(&mut runtime,"a",||panic!(),|_|panic!()));
        runtime.set_binding_disabled(0,true).unwrap();assert!(!handle_with(&mut runtime,"v",||panic!(),|_|panic!()));
    }
}
