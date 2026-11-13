//! Keyboard rules shared by the native window hosts.

use winit::keyboard::{Key, NamedKey, PhysicalKey};

/// Runtime key code of the keys that activate or dismiss a control; 0 for any other key.
pub fn activation_key(key: &Key) -> u8 {
    match key {
        Key::Named(NamedKey::Space) => 1,
        Key::Named(NamedKey::Enter) => 2,
        Key::Named(NamedKey::Escape) => 3,
        _ => 0,
    }
}

/// Move focus with Tab, wrapping to the other end once the last control is passed.
pub fn focus_traversal(runtime: &mut crate::Runtime, shift: bool) {
    if !runtime.focus_next(shift) {
        runtime.focus(false);
        runtime.focus_next(shift);
    }
}

/// A key press delivered to the focused text editor.
pub struct TextKey<'a> {
    pub logical_key: &'a Key,
    pub physical_key: &'a PhysicalKey,
    /// Text the key produces, when it produces any.
    pub text: Option<&'a str>,
    pub shift: bool,
    /// The platform shortcut modifier is held (and the press is not AltGr).
    pub command: bool,
    /// The platform modifier for moving by words is held.
    pub word_modifier: bool,
}

/// Route a key press to word movement, the clipboard, editing commands or text insertion.
pub fn text_edit_key(runtime: &mut crate::Runtime, press: &TextKey) {
    let key = match press.logical_key {
        Key::Character(text) => text.to_string(),
        Key::Named(name) => format!("{name:?}"),
        _ => String::new(),
    };
    let key = if press.command {
        crate::native_clipboard::command_key(press.physical_key, &key)
    } else {
        &key
    };
    let word_handled = press.word_modifier && runtime.text_word_key(key, press.shift);
    let clipboard_handled =
        !word_handled && press.command && crate::native_clipboard::handle_key(runtime, key);
    if !word_handled
        && !clipboard_handled
        && !runtime.text_key(key, press.shift, press.command)
        && !press.command
    {
        if let Some(text) = press.text {
            if !text.chars().any(char::is_control) {
                runtime.text_insert(text);
            }
        }
    }
}
