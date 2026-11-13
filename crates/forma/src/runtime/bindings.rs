//! Values a form pushes into controls and the interaction it must preserve.

use super::*;

impl Runtime {
    pub(crate) fn binding_widget_changed(&self, index: usize) -> bool {
        let baseline = self.binding_widget_baseline.borrow();
        match baseline.get(&index) {
            Some(BindingWidgetValue::Text(value)) => self
                .editors
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|editor| editor.model.value() != value),
            Some(BindingWidgetValue::Range(value)) => self.range_value(index) != *value,
            None => false,
        }
    }

    pub(crate) fn acknowledge_binding_widgets(&self) {
        let mut baseline = self.binding_widget_baseline.borrow_mut();
        for (index, control) in self.controls.iter().enumerate() {
            if let Some(editor) = &self.editors[index] {
                match baseline.get_mut(&index) {
                    Some(BindingWidgetValue::Text(value)) => {
                        if value != editor.model.value() {
                            value.clear();
                            value.push_str(editor.model.value());
                        }
                    }
                    _ => {
                        baseline.insert(
                            index,
                            BindingWidgetValue::Text(editor.model.value().to_owned()),
                        );
                    }
                }
            } else if let Some(range) = &control.template.range {
                match baseline.get_mut(&index) {
                    Some(BindingWidgetValue::Range(value)) => *value = range.value,
                    _ => {
                        baseline.insert(index, BindingWidgetValue::Range(range.value));
                    }
                }
            }
        }
    }

    /// Re-evaluation keeps gestures attached to the same uniquely keyed control.
    /// Context replacement is cancelled separately by the binding owner.
    pub(crate) fn preserve_binding_interaction(&mut self, previous: &Runtime) {
        let matches = self.preserve_interaction_matches(previous);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let same_structure = self.nodes.len() == previous.nodes.len()
                && self.nodes.iter().zip(&previous.nodes).all(|(a, b)| {
                    a.parent == b.parent && a.kind == b.kind && a.control == b.control
                });
            let mut keys = std::collections::BTreeSet::new();
            let same_controls = self.controls.len() == previous.controls.len()
                && matches.iter().enumerate().all(|(i, old)| {
                    let key = self.control_key(i);
                    !key.is_empty() && keys.insert(key) && *old == Some(i)
                });
            if same_structure && same_controls {
                self.native_accessibility_id = Some(previous.native_scene_identity());
            }
        }
        for (i, c) in self.controls.iter_mut().enumerate() {
            let Some(old_index) = matches[i] else {
                continue;
            };
            let old = &previous.controls[old_index];
            if !c.disabled() && c.template.clickable {
                c.down = old.down;
                c.keyboard = old.keyboard;
                c.update_colors();
                if previous.captured == Some(old_index) {
                    self.captured = Some(i);
                }
            }
        }
        let layout_changed = self.width() != previous.width()
            || self.height() != previous.height()
            || self.scene.scroll != previous.scene.scroll
            || self.viewport_rect() != previous.viewport_rect()
            || self.content_size() != previous.content_size()
            || self.controls.len() != previous.controls.len()
            || self
                .controls
                .iter()
                .zip(&previous.controls)
                .any(|(a, b)| a.bounds() != b.bounds());
        self.revision = previous.revision.wrapping_add(1);
        self.geometry_revision = previous.geometry_revision.wrapping_add(1);
        self.layout_revision = previous
            .layout_revision
            .wrapping_add(u32::from(layout_changed));
    }

    /// Apply model text without creating an undo entry. Replacing a binding or
    /// context resets editing history even when the two records contain equal text.
    pub fn set_binding_value(
        &mut self,
        index: usize,
        value: &str,
        reset: bool,
    ) -> Result<(), String> {
        let editor = self
            .editors
            .get_mut(index)
            .and_then(Option::as_mut)
            .ok_or_else(|| format!("Control {index} has no editable value"))?;
        if !reset && editor.model.value() == value {
            return Ok(());
        }
        let mut spec = editor.spec.clone();
        spec.value = value.into();
        let mut next = crate::text_input::Editor::new(spec, editor.base.clone())?;
        next.reduced_motion(self.controls[index].reduced_motion);
        if !reset {
            let mut caret = editor.model.caret().min(value.len());
            while !value.is_char_boundary(caret) {
                caret -= 1;
            }
            next.model
                .set_selection(caret, caret)
                .map_err(|e| e.to_string())?;
            let caret = next.model.grapheme_selection().start;
            next.model
                .set_selection(caret, caret)
                .map_err(|e| e.to_string())?;
        }
        *editor = next;
        self.refresh_editors();
        Ok(())
    }

    /// Text bindings target a single text primitive; ambiguous rich content is
    /// rejected instead of silently replacing captions or icons as well.
    pub fn set_binding_text(&mut self, index: usize, value: &str) -> Result<(), String> {
        let control = self
            .controls
            .get_mut(index)
            .ok_or_else(|| format!("Unknown control {index}"))?;
        let count = usize::from(control.template.text.is_some())
            + control
                .template
                .content
                .iter()
                .filter(|c| matches!(c, crate::template::Content::Text { .. }))
                .count();
        if count != 1 || self.editors[index].is_some() {
            return Err(format!(
                "Control {index}: text binding requires exactly one non-editable text primitive"
            ));
        }
        // The template is shared with the parse cache, so the first write takes a copy.
        // Only the edited control pays, and only once per keystroke it changes.
        let template = Arc::make_mut(&mut control.template);
        let text = if let Some(text) = &mut template.text {
            text
        } else {
            template
                .content
                .iter_mut()
                .find_map(|c| match c {
                    crate::template::Content::Text { text, .. } => Some(text),
                    _ => None,
                })
                .unwrap()
        };
        if text.text == value {
            return Ok(());
        }
        text.text = value.into();
        control.scene.button.text = value.into();
        *control.raster_cache.borrow_mut() = None;
        *control.display_cache.borrow_mut() = None;
        control.visual_revision = control.visual_revision.wrapping_add(1);
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn set_binding_disabled(&mut self, index: usize, disabled: bool) -> Result<(), String> {
        let control = self
            .controls
            .get_mut(index)
            .ok_or_else(|| format!("Unknown control {index}"))?;
        if control.scene.button.disabled == disabled {
            return Ok(());
        }
        control.scene.button.disabled = disabled;
        Arc::make_mut(&mut control.template).props.disabled = disabled;
        if disabled {
            control.down = false;
            control.keyboard = None;
        }
        control.update_colors();
        if disabled && self.captured == Some(index) {
            self.captured = None;
        }
        if disabled && self.focused == Some(index) {
            self.set_focus(None);
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn cancel_binding_interaction(&mut self, index: usize) {
        let before = self.child_revision();
        if let Some(control) = self.controls.get_mut(index) {
            control.down = false;
            control.keyboard = None;
            control.update_colors();
        }
        if self.captured == Some(index) {
            self.captured = None;
        }
        if let Some(Some(editor)) = self.editors.get_mut(index) {
            editor.set_preedit("");
        }
        self.refresh_editors();
        self.update_revision(before);
    }
}
