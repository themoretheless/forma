//! Input method composition.

use super::*;

impl<A: NativeApp + 'static> NativeHost<A> {
    /// Observe the retained runtime after application updates have completed.
    pub(super) fn ime_presentation(&mut self) -> Result<(bool, Vec<f32>), String> {
        let owner = self.composition_owner;
        let mut retired = false;
        let mut presentation = (false, Vec::new());
        let result = self.app.inspect(&mut |runtime| {
            retired = owner.is_some() && runtime.native_editor_session() != owner;
            presentation = (
                runtime.text_editing()
                    && !runtime.control_readonly(runtime.focused_index().max(0) as usize),
                runtime.text_caret_bounds(),
            );
        });
        // The old preedit belongs to the retired runtime; never clear text in
        // its replacement while observing presentation.
        if retired {
            self.composing = false;
            self.ime_cancelled = true;
        }
        result?;
        Ok(presentation)
    }

    pub(super) fn update_ime(&mut self) -> Result<(), String> {
        if self.chooser.is_some() {
            if let Some(window) = &self.window {
                window.set_ime_allowed(false);
            }
            return Ok(());
        }
        let (editing, caret) = self.ime_presentation()?;
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "Native window is not initialized".to_owned())?;
        window.set_ime_allowed(editing);
        if caret.len() == 4 {
            window.set_ime_cursor_area(
                LogicalPosition::new(caret[0] as f64, caret[1] as f64),
                LogicalSize::new(caret[2] as f64, caret[3] as f64),
            );
        }
        Ok(())
    }

    pub(super) fn ime_event(&mut self, event: Ime) -> Result<(), String> {
        self.app.on_ime_event(&event)?;
        match event {
            Ime::Preedit(text, _) => self.preedit_ime(&text),
            Ime::Commit(text) => self.commit_ime(&text),
            Ime::Enabled => {
                self.composing = false;
                self.ime_cancelled = false;
                self.composition_owner = None;
                Ok(())
            }
            Ime::Disabled => {
                self.composing = false;
                self.ime_cancelled = true;
                self.composition_owner = None;
                self.app.update(&mut |runtime| runtime.text_preedit(""))
            }
        }
    }

    pub(super) fn preedit_ime(&mut self, text: &str) -> Result<(), String> {
        let previous = self.composition_owner;
        let mut owner = previous;
        if !text.is_empty() {
            self.ime_cancelled = false;
        }
        self.composing = !text.is_empty();
        let result = self.app.update(&mut |runtime| {
            if !text.is_empty() {
                owner = runtime.native_editor_session();
                runtime.text_preedit(text);
            } else if previous.is_none() || runtime.native_editor_session() == previous {
                // An empty preedit removes marked pixels, but does not end the
                // ownership of the commit which commonly follows it.
                runtime.text_preedit("");
            }
        });
        self.composition_owner = owner;
        result
    }

    pub(super) fn commit_ime(&mut self, text: &str) -> Result<(), String> {
        let owner = self.composition_owner.take();
        self.composing = false;
        if self.ime_cancelled {
            return Ok(());
        }
        let mut retired = false;
        let result = self.app.update(&mut |runtime| {
            // A commit queued for an old editor must not enter a replacement
            // document or a newly focused field. Direct commits without a
            // preceding preedit remain supported.
            if owner.is_none() || runtime.native_editor_session() == owner {
                if text.is_empty() {
                    runtime.text_preedit("");
                } else {
                    runtime.text_insert(text);
                }
            } else {
                retired = true;
            }
        });
        if retired {
            self.ime_cancelled = true;
        }
        result
    }
}
