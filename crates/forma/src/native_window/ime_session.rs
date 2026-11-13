//! Input method composition state shared by the native window hosts.

#[cfg(test)]
use winit::event::Ime;

/// Which editor a composition belongs to, and whether its commit may still land.
///
/// A composition is owned by the editor session that received its first marked text. Once
/// that session is gone (focus moved, the scene was rebuilt), the commit which the platform
/// still delivers must not enter whatever replaced it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImeSession {
    /// Marked text is on screen.
    pub composing: bool,
    /// The composition was abandoned; its commit is dropped.
    pub cancelled: bool,
    /// Editor session the composition started in.
    pub owner: Option<(u64, i32)>,
}

impl ImeSession {
    /// A composition is in progress or its commit is still expected.
    pub fn pending(&self) -> bool {
        self.composing || self.owner.is_some()
    }

    /// Abandon the composition without touching the runtime.
    pub fn retire(&mut self) {
        self.composing = false;
        self.cancelled = true;
        self.owner = None;
    }

    /// Abandon the composition and remove its marked text.
    #[cfg(test)]
    pub fn cancel(&mut self, runtime: &mut crate::Runtime) {
        self.retire();
        runtime.text_preedit("");
    }

    /// Drop the composition when the editor that owns it no longer exists. The owner is kept
    /// so the commit that follows is recognised as belonging to the retired editor.
    pub fn observe(&mut self, runtime: &crate::Runtime) {
        if self.owner.is_some() && self.owner != runtime.native_editor_session() {
            self.composing = false;
            self.cancelled = true;
        }
    }

    /// The platform enabled input method editing; earlier cancellations no longer apply.
    pub fn enabled(&mut self) {
        self.composing = false;
        self.cancelled = false;
        self.owner = None;
    }

    pub fn preedit(&mut self, runtime: &mut crate::Runtime, text: &str) {
        let previous = self.owner;
        self.composing = !text.is_empty();
        if !text.is_empty() {
            self.cancelled = false;
            self.owner = runtime.native_editor_session();
            runtime.text_preedit(text);
        } else if previous.is_none() || runtime.native_editor_session() == previous {
            // An empty preedit removes marked pixels, but does not end the ownership of the
            // commit which commonly follows it.
            runtime.text_preedit("");
        }
    }

    pub fn commit(&mut self, runtime: &mut crate::Runtime, text: &str) {
        let owner = self.owner.take();
        self.composing = false;
        if self.cancelled {
            return;
        }
        // A commit queued for an old editor must not enter a replacement document or a newly
        // focused field. Direct commits without a preceding preedit remain supported.
        if owner.is_none() || runtime.native_editor_session() == owner {
            if text.is_empty() {
                runtime.text_preedit("");
            } else {
                runtime.text_insert(text);
            }
        } else {
            self.cancelled = true;
        }
    }

    /// Apply one platform event to a runtime the caller owns directly. The host applies events
    /// through its application instead; the tests drive the session with this.
    #[cfg(test)]
    pub fn event(&mut self, runtime: &mut crate::Runtime, event: Ime) {
        self.observe(runtime);
        match event {
            Ime::Enabled => self.enabled(),
            Ime::Disabled => self.cancel(runtime),
            Ime::Preedit(text, _) => self.preedit(runtime, &text),
            Ime::Commit(text) => self.commit(runtime, &text),
        }
    }
}

/// Whether the focused control accepts composed text, and its caret rectangle (x, y, width,
/// height in logical pixels; empty when there is no caret).
pub fn ime_target(runtime: &crate::Runtime) -> (bool, Vec<f32>) {
    (
        runtime.text_editing()
            && !runtime.control_readonly(runtime.focused_index().max(0) as usize),
        runtime.text_caret_bounds(),
    )
}

/// Tell the platform where the caret is, so the candidate window opens next to it.
pub fn place_ime_caret(window: &winit::window::Window, caret: &[f32]) {
    if let [x, y, width, height] = *caret {
        window.set_ime_cursor_area(
            winit::dpi::LogicalPosition::new(x as f64, y as f64),
            winit::dpi::LogicalSize::new(width as f64, height as f64),
        );
    }
}

#[cfg(test)]
#[path = "ime_session_tests.rs"]
mod tests;
