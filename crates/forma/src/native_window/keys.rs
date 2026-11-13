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
    pub modifiers: super::NativeModifiers,
}

/// Route a key press to word movement, the clipboard, editing commands or text insertion.
pub fn text_edit_key(runtime: &mut crate::Runtime, press: &TextKey) {
    let shift = press.modifiers.shift;
    let command = press.modifiers.shortcut(press.text.is_some());
    let word_modifier = press.modifiers.word_modifier();
    let key = match press.logical_key {
        Key::Character(text) => text.to_string(),
        Key::Named(name) => format!("{name:?}"),
        _ => String::new(),
    };
    let key = if command {
        crate::native_clipboard::command_key(press.physical_key, &key)
    } else {
        &key
    };
    let word_handled = word_modifier && runtime.text_word_key(key, shift);
    let clipboard_handled =
        !word_handled && command && crate::native_clipboard::handle_key(runtime, key);
    if !word_handled && !clipboard_handled && !runtime.text_key(key, shift, command) && !command {
        if let Some(text) = press.text {
            if !text.chars().any(char::is_control) {
                runtime.text_insert(text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::NativeModifiers;

    fn held(control: bool, alt: bool, command: bool) -> NativeModifiers {
        NativeModifiers {
            shift: false,
            control,
            alt,
            command,
        }
    }

    #[test]
    fn alt_gr_is_not_a_shortcut_where_control_is_the_shortcut_modifier() {
        let alt_gr = held(true, true, true);
        if cfg!(target_os = "macos") {
            assert!(!alt_gr.alt_gr(true));
            assert!(alt_gr.shortcut(true));
            assert!(alt_gr.accessibility_chord());
        } else {
            assert!(alt_gr.alt_gr(true));
            assert!(!alt_gr.shortcut(true));
            // Without text the same chord is an ordinary shortcut.
            assert!(alt_gr.shortcut(false));
            assert!(!alt_gr.accessibility_chord());
        }
    }

    #[test]
    fn word_movement_uses_option_on_macos_and_control_elsewhere() {
        let option = held(false, true, false);
        let shortcut = held(!cfg!(target_os = "macos"), false, true);
        assert_eq!(option.word_modifier(), cfg!(target_os = "macos"));
        assert_eq!(shortcut.word_modifier(), cfg!(not(target_os = "macos")));
        assert!(!held(true, true, true).word_modifier());
    }
}
