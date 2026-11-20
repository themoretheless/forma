//! Loading scenes and pushing application state.

use super::*;

#[wasm_bindgen]
impl Runtime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::from_source(crate::EXAMPLE).expect("valid built-in scene")
    }

    pub fn load_component(&mut self, source: &str, component: &str) -> Result<(), JsValue> {
        self.load_component_native(source, component)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Load a complete artifact atomically through the Rust parser/runtime.
    pub fn load_scene_artifact(&mut self, json: &str) -> Result<(), JsValue> {
        self.load_scene_artifact_native(json)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Compile a UI project to a versioned transport in Rust for a later runtime load.
    pub fn compile_ui_scene_artifact(
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<String, JsValue> {
        Self::compile_ui_scene_artifact_native(files_json, entry_file, state_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Exports the authoritative `.ui` property and binding registry as JSON.
    pub fn property_registry_json() -> String {
        crate::form_document::property_registry_json()
    }

    pub fn load(&mut self, source: &str) -> Result<(), JsValue> {
        self.load_component(source, crate::BUTTON_COMPONENT)
    }

    /// Compiles a JSON map of `.ui`/`.svg` project files directly in Rust and loads the rendered scene.
    pub fn load_ui_files_json(
        &mut self,
        files_json: &str,
        entry_file: &str,
        state_json: &str,
    ) -> Result<(), JsValue> {
        self.load_ui_files_json_native(files_json, entry_file, state_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Synchronizes two-way `<->` bindings (`value`, `checked`, `selected`) from the live runtime
    /// back into the retained Rust `.ui` `Document`, re-rendering the scene while preserving focus
    /// and active text/slider interaction. Returns `true` after a successful scene rebuild.
    pub fn sync_ui_bindings(&mut self) -> Result<bool, JsValue> {
        self.sync_ui_bindings_native()
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Replaces the named root state fields of the retained `.ui` document from a JSON object and
    /// synchronizes once, keeping focus, text and scroll. Returns `true` after a scene rebuild.
    pub fn set_ui_state_json(&mut self, state_json: &str) -> Result<bool, JsValue> {
        self.set_ui_state_json_native(state_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Sets individual state paths (`{"user.name":"Ada","items.2.done":true}`) of the retained
    /// `.ui` document and synchronizes once. Returns `true` after a scene rebuild.
    pub fn patch_ui_state_json(&mut self, patch_json: &str) -> Result<bool, JsValue> {
        self.patch_ui_state_json_native(patch_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Number of accepted activations waiting for a successful binding commit.
    pub fn pending_ui_binding_activations(&self) -> usize {
        self.ui_activations.events.len()
    }

    /// Explicitly discard buffered activations, clearing an overflow refusal.
    /// Widget values remain available for the next binding synchronization.
    pub fn cancel_ui_binding_activations(&mut self) -> usize {
        let count = self.ui_activations.events.len();
        self.ui_activations.clear();
        self.last_event = None;
        count
    }

    /// Exports the current state of the retained Rust `.ui` `Document` as a JSON string.
    pub fn ui_state_json(&self) -> String {
        self.ui_document
            .as_ref()
            .and_then(|doc| doc.state_json().ok())
            .unwrap_or_else(|| "{}".to_string())
    }
}
