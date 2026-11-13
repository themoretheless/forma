//! The accessibility tree and the actions assistive technology requests.

use super::*;

impl<A: NativeApp + 'static> NativeHost<A> {
    pub(super) fn accessibility_action(
        &mut self,
        request: &accesskit::ActionRequest,
    ) -> Result<bool, String> {
        if request.target_tree != accesskit::TreeId::ROOT {
            return Ok(false);
        }
        let pending_ime = self.composing || self.composition_owner.is_some();
        let mut accepted = false;
        let published = std::mem::take(&mut self.accessibility_ids);
        let result = if let Some(chooser) = &mut self.chooser {
            accepted = published.apply(chooser, request);
            Ok(())
        } else {
            self.focus_sensitive_input(&mut |runtime| {
                accepted = published.apply(runtime, request);
            })
        };
        self.accessibility_ids = published;
        result?;
        if accepted
            && pending_ime
            && matches!(
                request.action,
                accesskit::Action::SetValue | accesskit::Action::SetTextSelection
            )
        {
            self.composing = false;
            self.composition_owner = None;
            self.ime_cancelled = true;
        }
        Ok(accepted)
    }

    pub(super) fn update_accessibility(&mut self) -> Result<(), String> {
        if !self.accessibility_active {
            return Ok(());
        }
        let scale = self
            .window
            .as_ref()
            .map_or(1., |window| window.scale_factor());
        let mut update = None;
        let published = &mut self.accessibility_ids;
        if let Some(chooser) = &self.chooser {
            update = Some(published.update_if_changed(chooser, scale));
        } else {
            self.app.inspect(&mut |runtime| {
                update = Some(published.update_if_changed(runtime, scale))
            })?;
        }
        let update = update.ok_or_else(|| {
            "NativeApp.inspect did not observe accessibility runtime".to_owned()
        })??;
        if let (Some(adapter), Some(update)) = (&mut self.accessibility, update) {
            adapter.update_if_active(|| update);
        }
        Ok(())
    }
}
