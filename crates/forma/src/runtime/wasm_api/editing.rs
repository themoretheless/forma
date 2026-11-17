//! Text and range editing.

use super::*;

#[wasm_bindgen]
impl Runtime {
    pub fn range_value(&self, i: usize) -> f32 {
        self.controls
            .get(i)
            .and_then(|c| c.template.range.as_ref())
            .map_or(f32::NAN, |r| r.value)
    }

    pub fn range_key(&mut self, key: &str) -> bool {
        let Some(i) = self.focused else { return false };
        if self.group_key(i, key) {
            return true;
        }
        if self.controls[i].disabled() || self.controls[i].template.range.is_none() {
            return false;
        }
        let Some(range) = Arc::make_mut(&mut self.controls[i].template).range.as_mut() else {
            return false;
        };
        let (arrow, page) = range.key_steps();
        let moved = match key {
            "Home" => 0.,
            "End" => 1.,
            "ArrowLeft" | "ArrowDown" => range.value - arrow,
            "ArrowRight" | "ArrowUp" => range.value + arrow,
            "PageDown" => range.value - page,
            "PageUp" => range.value + page,
            _ => return false,
        };
        range.value = range.snapped(moved);
        self.refresh_editors();
        true
    }

    /// An arrow key on a radio button of a `group <->` binding moves focus to the next (Down,
    /// Right) or previous (Up, Left) enabled button of the same group, wrapping around, and
    /// selects it, as radio groups do.
    fn group_key(&mut self, i: usize, key: &str) -> bool {
        let Some(group) = self.groups.get(i).cloned().flatten() else {
            return false;
        };
        let forward = match key {
            "ArrowDown" | "ArrowRight" => true,
            "ArrowUp" | "ArrowLeft" => false,
            _ => return false,
        };
        let members: Vec<usize> = (0..self.controls.len())
            .filter(|&j| self.groups[j].as_deref() == Some(group.as_str()) && self.enabled(j))
            .collect();
        let Some(at) = members.iter().position(|&j| j == i) else {
            return false;
        };
        let next = if forward {
            members[(at + 1) % members.len()]
        } else {
            members[(at + members.len() - 1) % members.len()]
        };
        self.set_focus(Some(next));
        self.activate_control(next);
        true
    }

    pub fn text_editing(&self) -> bool {
        self.focused
            .is_some_and(|i| self.editors[i].is_some() && self.enabled(i))
    }

    /// Whether this control accepts multiline editing; false for noneditors.
    pub fn control_readonly(&self, i: usize) -> bool {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|editor| editor.spec.read_only)
    }

    pub fn control_multiline(&self, i: usize) -> bool {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|editor| editor.spec.multiline)
    }

    pub fn control_editable(&self, i: usize) -> bool {
        self.editors.get(i).is_some_and(Option::is_some)
    }

    /// Semantic editor epoch; visual caret animation does not change it.
    /// Zero means unavailable/exhausted: consumers must read the model without caching.
    pub fn text_model_revision(&self) -> u32 {
        self.focused
            .and_then(|i| self.editors[i].as_ref())
            .map_or(0, |editor| editor.semantic_revision)
    }

    pub fn text_value(&self, i: usize) -> String {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .map_or_else(String::new, |e| e.model.value().into())
    }

    /// Focused anchor/caret in textarea UTF-16 units, with CRLF normalized to LF.
    /// The model keeps its original bytes; empty without a focused editor.
    pub fn text_selection_utf16(&self) -> Vec<u32> {
        self.focused
            .and_then(|i| self.editors[i].as_ref())
            .map_or_else(Vec::new, |editor| {
                [editor.model.anchor(), editor.model.caret()]
                    .into_iter()
                    .map(|at| {
                        let prefix = &editor.model.value()[..at];
                        (prefix.encode_utf16().count()
                            - prefix
                                .as_bytes()
                                .windows(2)
                                .filter(|pair| *pair == b"\r\n")
                                .count()) as u32
                    })
                    .collect()
            })
    }

    /// Apply normalized textarea UTF-16 selection, preserving grapheme boundaries.
    pub fn text_set_selection_utf16(&mut self, anchor: u32, caret: u32) -> bool {
        if !self.text_editing() {
            return false;
        }
        let changed = self.editors[self.focused.unwrap()]
            .as_mut()
            .unwrap()
            .select_utf16(anchor, caret);
        self.refresh_editors();
        changed
    }

    pub fn text_selected(&self) -> String {
        self.focused
            .and_then(|i| self.editors[i].as_ref())
            .map_or_else(String::new, |e| e.model.selected_text().into())
    }

    pub fn text_insert(&mut self, text: &str) -> bool {
        if !self.text_editing() || self.control_readonly(self.focused.unwrap()) {
            return false;
        }
        self.editors[self.focused.unwrap()]
            .as_mut()
            .unwrap()
            .insert(text);
        self.refresh_editors();
        true
    }

    pub fn text_preedit(&mut self, text: &str) {
        if self.text_editing() {
            self.editors[self.focused.unwrap()]
                .as_mut()
                .unwrap()
                .set_preedit(text);
            self.refresh_editors();
        }
    }

    /// Host-normalized word modifier (Option on macOS, Ctrl on Windows/Linux).
    pub fn text_word_key(&mut self, key: &str, shift: bool) -> bool {
        if !self.text_editing() {
            return false;
        }
        let handled = self.editors[self.focused.unwrap()]
            .as_mut()
            .unwrap()
            .word_key(key, shift);
        self.refresh_editors();
        handled
    }

    pub fn text_key(&mut self, key: &str, shift: bool, command: bool) -> bool {
        if !self.text_editing() {
            return false;
        }
        let index = self.focused.unwrap();
        let multiline = self.control_multiline(index);
        let handled = self.editors[index]
            .as_mut()
            .unwrap()
            .key(key, shift, command);
        self.refresh_editors();
        if handled && key == "Enter" && !multiline {
            self.submissions[index] = self.submissions[index].saturating_add(1);
            self.last_submitted = Some(index);
        }
        handled
    }

    pub fn text_caret_bounds(&self) -> Vec<f32> {
        let Some(i) = self.focused else { return vec![] };
        let Some(e) = &self.editors[i] else {
            return vec![];
        };
        let mut rect = e.caret_rect();
        let b = self.controls[i].bounds_rect();
        rect[0] += b[0];
        rect[1] += b[1];
        rect.to_vec()
    }

    /// Number of visual lines produced by soft-wrapping the control's text editor.
    pub fn text_soft_line_count(&self, i: usize) -> usize {
        self.editors
            .get(i)
            .and_then(Option::as_ref)
            .map_or(0, |e| e.soft_wrapped_lines().len())
    }

    /// Hit-test a point inside the focused editor using visual soft-wrapping.
    pub fn text_hit_soft_wrapped(&mut self, x: f32, y: f32, extend: bool) -> bool {
        let Some(i) = self.focused else {
            return false;
        };
        if !self.text_editing() {
            return false;
        }
        let b = self.controls[i].bounds_rect();
        self.editors[i]
            .as_mut()
            .unwrap()
            .hit_soft_wrapped(x - b[0], y - b[1], extend);
        self.refresh_editors();
        true
    }
}
