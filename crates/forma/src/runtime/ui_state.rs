//! The retained document: its state and the bindings it drives.

use super::*;

impl Runtime {
    /// Select owned Svet scheduling for retained native document updates.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_svet_document_scheduling(&mut self, enabled: bool) {
        if enabled && self.svet_document.is_none() {
            let states = self
                .ui_document
                .as_ref()
                .map_or(&[][..], |doc| doc.states.as_slice());
            self.svet_document = Some(crate::native_document_graph::DocumentGraph::new(states));
        } else if !enabled {
            self.svet_document = None;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn svet_document_telemetry(&self) -> Option<svet_graph::context::Telemetry> {
        self.svet_document.as_ref().map(|graph| graph.telemetry())
    }

    /// Synchronizes two-way `<->` bindings (`value`, `checked`, `selected`) from the live runtime
    /// back into the retained Rust `.ui` `Document`, re-rendering the scene while preserving focus
    /// and active text/slider interaction. Returns `Ok(true)` when the scene is rebuilt,
    /// including a successful retry of an earlier failed render.
    pub fn sync_ui_bindings_native(&mut self) -> Result<bool, String> {
        if self.ui_activations.overflow {
            return Err("Pending UI activation limit exceeded; cancel pending activations or reload the document".into());
        }
        let Some(mut doc) = self.ui_document.take() else {
            return Ok(false);
        };
        let activated = self.last_event.take();
        let model_changed = doc.states != self.ui_binding_states;
        if self.ui_binding_idle_ready
            && !model_changed
            && !self.ui_render_pending
            && activated.is_none()
            && self.ui_activations.events.is_empty()
            && (0..self.control_count()).all(|index| !self.binding_widget_changed(index))
        {
            self.ui_document = Some(doc);
            return Ok(false);
        }
        // First synchronization, rebuilds and failed attempts must still validate
        // binding targets and initialize missing model fields from live widgets.
        self.ui_binding_idle_ready = false;
        let controls = if !model_changed {
            std::borrow::Cow::Borrowed(self.ui_binding_scene.as_slice())
        } else {
            match doc.binding_scene_controls() {
                Ok(controls) => std::borrow::Cow::Owned(controls),
                Err(error) => {
                    self.ui_render_pending = true;
                    self.last_event = activated;
                    self.ui_document = Some(doc);
                    return Err(error);
                }
            }
        };
        let changed =
            match doc.sync_runtime_binding_events(self, &self.ui_activations.events, &controls) {
                Ok(c) => c,
                Err(e) => {
                    // Binding writes are atomic; an uncommitted activation must
                    // remain available for retry after the metadata is repaired.
                    self.last_event = activated;
                    self.ui_document = Some(doc);
                    return Err(e);
                }
            };
        self.ui_activations.clear();
        #[cfg(not(target_arch = "wasm32"))]
        let needs_render = self
            .svet_document
            .as_mut()
            .map_or(changed > 0, |graph| graph.reconcile(&doc.states))
            || model_changed
            || self.ui_render_pending;
        #[cfg(target_arch = "wasm32")]
        let needs_render = changed > 0 || model_changed || self.ui_render_pending;
        self.ui_render_pending = needs_render;
        if needs_render {
            let (scene_src, templates, controls, logical_end) =
                match doc.render_scene_with_extents() {
                    Ok(res) => res,
                    Err(e) => {
                        self.ui_document = Some(doc);
                        return Err(e);
                    }
                };
            let mut next = match Self::from_sources(&scene_src, &templates) {
                Ok(rt) => rt,
                Err(e) => {
                    self.ui_document = Some(doc);
                    return Err(e);
                }
            };
            next.apply_logical_content_end(logical_end);
            next.preserve_binding_interaction(self);
            next.ui_binding_scene = controls;
            next.ui_binding_states = doc.states.clone();
            next.ui_document = Some(doc);
            #[cfg(not(target_arch = "wasm32"))]
            {
                if let Some(graph) = &mut self.svet_document {
                    graph.complete();
                }
                next.svet_document = self.svet_document.take();
            }
            *self = next;
            Ok(true)
        } else {
            self.ui_binding_idle_ready = true;
            self.ui_document = Some(doc);
            Ok(false)
        }
    }

    /// Replaces root state fields of the retained `.ui` document with application values, then
    /// synchronizes like [`Self::sync_ui_bindings_native`]: one rebuild at most, with focus, text,
    /// undo history and scroll offset kept. Fields that are not named keep their value, so state
    /// owned by the screen itself survives. Text the user typed since the last synchronization
    /// still wins for the field it is bound to. Returns `Ok(false)` when nothing was rebuilt.
    /// A refused push changes nothing: state, pending input and the scene stay as they were.
    pub fn set_ui_state_properties(
        &mut self,
        state: &crate::form_document::Properties,
    ) -> Result<bool, String> {
        self.push_ui_state(crate::form_document::StatePush::fields(state.clone()))
    }

    /// [`Self::set_ui_state_properties`] for a JSON object, converted like the initial state of
    /// [`Self::load_ui_files_json_native`].
    pub fn set_ui_state_json_native(&mut self, state_json: &str) -> Result<bool, String> {
        self.push_ui_state(crate::form_document::StatePush::from_json(
            state_json, false,
        )?)
    }

    /// Sets individual state paths from a JSON object such as
    /// `{"user.name":"Ada","items.2.done":true}`, with the guarantees of
    /// [`Self::set_ui_state_properties`]. A path steps through objects by field name and through
    /// lists by index; missing objects are created, a missing list item is an error.
    pub fn patch_ui_state_json_native(&mut self, patch_json: &str) -> Result<bool, String> {
        self.push_ui_state(crate::form_document::StatePush::from_json(
            patch_json, true,
        )?)
    }

    pub(super) fn push_ui_state(
        &mut self,
        push: crate::form_document::StatePush,
    ) -> Result<bool, String> {
        let Some(doc) = self.ui_document.as_mut() else {
            return Err("UI state requires a document loaded from .ui files".into());
        };
        if doc.states.is_empty() {
            doc.states.push(Default::default());
        }
        let mut pushed = doc.states[0].clone();
        push.apply(&mut pushed)?;
        // Synchronization commits input before it renders. Everything it may
        // consume is kept, so a scene that refuses the push loses nothing.
        let state = std::mem::replace(&mut doc.states[0], pushed);
        let baseline = self.binding_widget_baseline.borrow().clone();
        let (events, last_event) = (self.ui_activations.events.clone(), self.last_event);
        let (pending, idle) = (self.ui_render_pending, self.ui_binding_idle_ready);
        let result = self.sync_ui_bindings_native();
        if result.is_err() {
            // A failed synchronization never replaces this Runtime.
            if let Some(doc) = self.ui_document.as_mut() {
                doc.states[0] = state;
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(graph) = &mut self.svet_document {
                    graph.reconcile(&doc.states);
                    if !pending {
                        graph.complete();
                    }
                }
            }
            *self.binding_widget_baseline.borrow_mut() = baseline;
            self.ui_activations.events = events;
            self.last_event = last_event;
            self.ui_render_pending = pending;
            self.ui_binding_idle_ready = idle;
        }
        result
    }

    /// Returns the retained `.ui` `Document` state map (if loaded via `from_ui_files`), or empty map.
    pub fn ui_state_properties(&self) -> crate::form_document::Properties {
        self.ui_document
            .as_ref()
            .and_then(|d| d.states.first().cloned())
            .unwrap_or_default()
    }
}
