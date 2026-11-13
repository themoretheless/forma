//! Input method composition.

use super::*;

impl<A: NativeApp + 'static> NativeHost<A> {
    /// Observe the retained runtime after application updates have completed.
    pub(super) fn ime_presentation(&mut self) -> Result<(bool, Vec<f32>), String> {
        let mut ime = self.ime;
        let mut presentation = (false, Vec::new());
        let result = self.app.inspect(&mut |runtime| {
            ime.observe(runtime);
            presentation = (
                runtime.text_editing()
                    && !runtime.control_readonly(runtime.focused_index().max(0) as usize),
                runtime.text_caret_bounds(),
            );
        });
        // The old preedit belongs to the retired runtime; never clear text in
        // its replacement while observing presentation.
        self.ime = ime;
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
                self.ime.enabled();
                Ok(())
            }
            Ime::Disabled => {
                self.ime.retire();
                self.app.update(&mut |runtime| runtime.text_preedit(""))
            }
        }
    }

    pub(super) fn preedit_ime(&mut self, text: &str) -> Result<(), String> {
        let ime = &mut self.ime;
        self.app.update(&mut |runtime| ime.preedit(runtime, text))
    }

    pub(super) fn commit_ime(&mut self, text: &str) -> Result<(), String> {
        if self.ime.cancelled {
            // Nothing reaches the runtime, so the application is not updated either.
            self.ime.owner = None;
            self.ime.composing = false;
            return Ok(());
        }
        let ime = &mut self.ime;
        self.app.update(&mut |runtime| ime.commit(runtime, text))
    }
}
