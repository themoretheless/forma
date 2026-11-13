//! What an application implements to run in a native window, and the input events it receives.

use super::*;

/// Presentation path that successfully submitted a native frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeRenderer {
    Cpu,
    Gpu,
}

/// Application interface consumed by the native window host.
pub trait NativeApp {
    fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String>;

    /// Inspect the current runtime without dispatching input or updating the app.
    /// Invoke the observer exactly once on the runtime retained after update.
    fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String>;

    /// Optional observer after a frame was actually presented.
    fn on_frame_presented(&self, _renderer: NativeRenderer) -> Result<(), String> {
        Ok(())
    }

    /// Optional observer after completion health service, without a new frame.
    /// `idle` means no queue work remains and completion wakeups will stop.
    fn on_gpu_health_checked(&self, _idle: bool) -> Result<(), String> {
        Ok(())
    }

    /// Optional observer before a failed GPU path falls back to CPU presentation.
    fn on_gpu_fallback(&self, _error: &str) -> Result<(), String> {
        Ok(())
    }

    /// Optional observer for OS window visibility/occlusion notifications.
    fn on_occlusion_changed(&self, _occluded: bool) -> Result<(), String> {
        Ok(())
    }

    /// Optional diagnostic observer for IME events before host dispatch.
    /// Applications should avoid collecting user text unless explicitly needed.
    fn on_ime_event(&self, _event: &Ime) -> Result<(), String> {
        Ok(())
    }

    fn on_dirty(&self, callback: Box<dyn Fn()>) -> Result<(), String> {
        let _ = callback;
        Ok(())
    }

    /// Receives the window's waker once, when the window is created. Keep a clone in
    /// worker threads and call [`NativeWaker::wake`] after publishing a result: the
    /// loop then runs `update` on its own thread and redraws.
    fn on_waker(&self, _waker: NativeWaker) -> Result<(), String> {
        Ok(())
    }

    /// Sees a key before the runtime does. Return `true` to mark it handled: the
    /// runtime then receives nothing for this event. Keys the demo host captures
    /// (see [`HostMode`]) and keys typed on its backend chooser are not offered.
    fn on_key_event(&mut self, _event: &NativeKeyEvent<'_>) -> Result<bool, String> {
        Ok(false)
    }

    /// The window's content area changed. `width` and `height` are logical pixels,
    /// `scale` is the display scale factor. Called once when the window is created and
    /// then on every resize or scale change; a minimized window may report zero.
    fn on_resized(&mut self, _width: f32, _height: f32, _scale: f32) -> Result<(), String> {
        Ok(())
    }

    /// The user asked to close the window. Return `false` to keep it open.
    fn on_close_requested(&mut self) -> Result<bool, String> {
        Ok(true)
    }

    /// Pointer input the runtime has no notion of: secondary and middle buttons, and a
    /// double click of the primary button (reported after the runtime received the
    /// second press). Not called while the demo's backend chooser is shown.
    fn on_pointer_event(&mut self, _event: &NativePointerEvent) -> Result<(), String> {
        Ok(())
    }
}

/// Modifier keys held during an input event. `command` is the platform's shortcut
/// modifier: Command on macOS, Control elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

impl NativeModifiers {
    pub fn from_state(state: winit::keyboard::ModifiersState) -> Self {
        Self {
            shift: state.shift_key(),
            control: state.control_key(),
            alt: state.alt_key(),
            command: if cfg!(target_os = "macos") {
                state.super_key()
            } else {
                state.control_key()
            },
        }
    }

    /// Ctrl+Option belongs to macOS accessibility, not text editing.
    pub fn accessibility_chord(self) -> bool {
        cfg!(target_os = "macos") && self.control && self.alt
    }

    /// Ctrl+Alt is AltGr for text-producing keys on Windows and Linux.
    pub fn alt_gr(self, produces_text: bool) -> bool {
        cfg!(not(target_os = "macos")) && self.command && self.alt && produces_text
    }

    /// The shortcut modifier applies to this key press.
    pub fn shortcut(self, produces_text: bool) -> bool {
        self.command && !self.alt_gr(produces_text)
    }

    /// The modifier for moving by words: Option on macOS, Control elsewhere.
    pub fn word_modifier(self) -> bool {
        if cfg!(target_os = "macos") {
            self.alt && !self.command
        } else {
            self.command && !self.alt
        }
    }
}

/// Key event offered to [`NativeApp::on_key_event`].
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct NativeKeyEvent<'a> {
    pub logical_key: &'a Key,
    pub physical_key: &'a PhysicalKey,
    /// Text the key produces, when it produces any.
    pub text: Option<&'a str>,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: NativeModifiers,
    /// An input-method composition is in progress; the key usually belongs to it.
    pub composing: bool,
}

/// What [`NativeApp::on_pointer_event`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativePointerKind {
    Secondary { pressed: bool },
    Middle { pressed: bool },
    DoubleClick,
}

/// Pointer event in logical pixels of the window's content area.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct NativePointerEvent {
    pub kind: NativePointerKind,
    pub x: f32,
    pub y: f32,
    pub modifiers: NativeModifiers,
}

/// Maps a mouse button to an application pointer event. The primary button belongs to
/// the runtime; back, forward and other buttons stay unreported.
pub(super) fn button_pointer_kind(button: MouseButton, pressed: bool) -> Option<NativePointerKind> {
    match button {
        MouseButton::Right => Some(NativePointerKind::Secondary { pressed }),
        MouseButton::Middle => Some(NativePointerKind::Middle { pressed }),
        _ => None,
    }
}

impl NativeApp for crate::Runtime {
    fn inspect(&self, observer: &mut dyn FnMut(&crate::Runtime)) -> Result<(), String> {
        observer(self);
        Ok(())
    }

    fn update(&mut self, input: &mut dyn FnMut(&mut crate::Runtime)) -> Result<(), String> {
        input(self);
        self.sync_ui_bindings_native().map(|_| ())
    }
}
