//! Input method composition.

use super::*;

impl<A: NativeApp + 'static> NativeHost<A> {
    /// Observe the retained runtime after application updates have completed.
    pub(super) fn ime_presentation(&mut self) -> Result<(bool, Vec<f32>), String> {
        let mut ime = self.ime;
        let mut presentation = (false, Vec::new());
        let result = self.app.inspect(&mut |runtime| {
            ime.observe(runtime);
            presentation = ime_target(runtime);
        });
        // The old preedit belongs to the retired runtime; never clear text in
        // its replacement while observing presentation.
        self.ime = ime;
        result?;
        Ok(presentation)
    }

    pub(super) fn update_ime(&mut self) -> Result<(), String> {
        if self.shell.chooser.is_some() {
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
        place_ime_caret(window, &caret);
        Ok(())
    }

    pub(super) fn ime_event(&mut self, event: Ime) -> Result<(), String> {
        self.app.on_ime_event(&event)?;
        match event {
            Ime::Preedit(text, cursor) => self.preedit_ime_cursor(&text, cursor),
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

    #[cfg(test)]
    pub(super) fn preedit_ime(&mut self, text: &str) -> Result<(), String> {
        self.preedit_ime_cursor(text, None)
    }

    pub(super) fn preedit_ime_cursor(
        &mut self,
        text: &str,
        cursor: Option<(usize, usize)>,
    ) -> Result<(), String> {
        let ime = &mut self.ime;
        self.app
            .update(&mut |runtime| ime.preedit_with_cursor(runtime, text, cursor))
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
