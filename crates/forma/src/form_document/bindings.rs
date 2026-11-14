use super::WidgetState;
use super::{
    Document, Properties, RangeBounds, SceneControl, Value, collect_preorder_nodes,
    doc_props_to_json, enum_value, eval, json_to_doc_props, key_part, lookup, number, scope_mut,
};
use std::collections::BTreeMap;

mod lookup;
mod resolve;
mod state_push;
mod write;

pub(super) use lookup::*;
pub(super) use resolve::*;
pub(crate) use state_push::*;
pub(super) use write::*;

impl Document {
    /// The scale of a range control in the scene: its own `min`, `max` and `step`, or the
    /// defaults of its component.
    fn range_bounds(&self, node: &super::Node, state: &Properties) -> RangeBounds {
        let mut props = Properties::new();
        let defaults = self
            .definitions
            .get(node.kind.as_str())
            .map(|definition| &definition.defaults);
        for name in ["min", "max", "step"] {
            let declared = node
                .props
                .get(name)
                .or_else(|| defaults.and_then(|d| d.get(name)));
            if let Some(value) = declared {
                if let Ok(value) = eval(value, &Properties::new(), state, &mut Vec::new()) {
                    props.insert(name.into(), value);
                }
            }
        }
        RangeBounds::of(|name| props.get(name))
    }

    /// Synchronizes two-way `<->` bindings (`value`, `checked`, `selected`) from the live widgets
    /// back into `self.states` (including nested `state.a.b` paths).
    /// Returns the number of bound properties that changed. An error leaves
    /// all state scopes unchanged; successful writes commit as one batch.
    pub fn sync_runtime_bindings(
        &mut self,
        runtime: &impl WidgetState,
        activated_index: Option<usize>,
    ) -> Result<usize, String> {
        let controls = self.binding_scene_controls()?;
        self.sync_runtime_bindings_from_scene(runtime, activated_index, &controls)
    }

    /// Use the last displayed binding map when current state cannot render.
    /// The host must supply controls belonging to this document's live scene.
    pub(crate) fn sync_runtime_bindings_from_scene(
        &mut self,
        runtime: &impl WidgetState,
        activated_index: Option<usize>,
        controls: &[SceneControl],
    ) -> Result<usize, String> {
        self.sync_runtime_binding_events(runtime, activated_index.as_slice(), controls)
    }

    /// Read current widget values once, then replay buffered activations in order.
    /// All writes share a single atomic state transaction.
    pub(crate) fn sync_runtime_binding_events(
        &mut self,
        runtime: &impl WidgetState,
        activations: &[usize],
        controls: &[SceneControl],
    ) -> Result<usize, String> {
        let mut preorder_nodes = Vec::with_capacity(64);
        collect_preorder_nodes(&self.root, &mut preorder_nodes, 0)?;
        // Scene expansion may have reordered/removed controls since the live
        // widgets captured their events. Match stable keys before using indices.
        let mut runtime_keys = BTreeMap::new();
        for index in 0..runtime.control_count() {
            let key = runtime.control_key(index);
            if runtime_keys.insert(key.clone(), Some(index)).is_some() {
                runtime_keys.insert(key, None);
            }
        }
        let mut bound_controls = vec![None; runtime.control_count()];
        let mut scene_keys = std::collections::BTreeSet::new();
        for control in controls {
            if !scene_keys.insert(&control.key) {
                return Err(format!("Ambiguous binding control key: {}", control.key));
            }
            match runtime_keys.get(&control.key) {
                Some(Some(index)) => bound_controls[*index] = Some(control),
                Some(None) => {
                    return Err(format!("Ambiguous runtime binding key: {}", control.key));
                }
                None => {} // Newly inserted controls do not own a live widget yet.
            }
        }
        // Commit one root scope only after every binding has validated. The
        // staged values also preserve sequential/alias semantics within a batch.
        let empty_state = Properties::new();
        let mut state = std::borrow::Cow::Borrowed(self.states.first().unwrap_or(&empty_state));
        let mut changed = 0usize;
        let mut key_indexes = BindingKeyIndexes::new();
        for activated_index in std::iter::once(None).chain(activations.iter().copied().map(Some)) {
            let start = activated_index.unwrap_or(0);
            let end = activated_index.map_or(bound_controls.len(), |index| {
                index.saturating_add(1).min(bound_controls.len())
            });
            for (idx, ctrl) in bound_controls
                .iter()
                .enumerate()
                .take(end)
                .skip(start)
                .filter_map(|(idx, slot)| slot.as_ref().map(|ctrl| (idx, ctrl)))
            {
                let Some(&src_node) = preorder_nodes.get(ctrl.node).or_else(|| {
                    preorder_nodes.iter().find(|n| {
                        n.props.get("__node").and_then(|v| v.number().ok())
                            == Some(ctrl.node as f64)
                    })
                }) else {
                    continue;
                };
                let Some(Value::Object(bindings)) = src_node.props.get("__bindings") else {
                    continue;
                };
                for (prop_name, path_val) in bindings {
                    let path = path_val.text()?;
                    let applies = match prop_name.as_str() {
                        "value" => activated_index.is_none(),
                        "checked" | "selected" | "group" => activated_index == Some(idx),
                        _ => false,
                    };
                    if !applies {
                        continue;
                    }
                    let collection_target = if path.starts_with("state.") {
                        None
                    } else if let Some((item, tail)) = path.split_once('.') {
                        let Some((collection, index)) =
                            resolve_binding_item_cached(ctrl, item, &state, &mut key_indexes)?
                        else {
                            continue;
                        };
                        Some((collection, index, tail.to_owned()))
                    } else {
                        None
                    };
                    if prop_name == "value" && !runtime.binding_widget_changed(idx) {
                        let present = if let Some(path) = path.strip_prefix("state.") {
                            lookup_binding_ref(&state, path).is_some()
                                || lookup(&state, path).is_some()
                        } else if let Some((collection, index, tail)) = &collection_target {
                            match lookup_binding_ref(&state, collection) {
                                Some(Value::List(items)) => match items.get(*index) {
                                    Some(Value::Object(item)) => {
                                        lookup_binding_ref(item, tail).is_some()
                                            || lookup(item, tail).is_some()
                                    }
                                    _ => false,
                                },
                                _ => false,
                            }
                        } else {
                            false
                        };
                        if present {
                            continue;
                        }
                    }
                    let new_val = match prop_name.as_str() {
                        "value" if activated_index.is_none() => {
                            if runtime.control_editable(idx) {
                                Some(Value::Text(runtime.text_value(idx)))
                            } else {
                                let r = runtime.range_value(idx);
                                if r.is_finite() {
                                    let bounds = self.range_bounds(src_node, &state);
                                    Some(number(bounds.value(r as f64)))
                                } else {
                                    None
                                }
                            }
                        }
                        "checked" if activated_index == Some(idx) => {
                            let current =
                                if let Some((collection, index, tail)) = &collection_target {
                                    match lookup_binding_ref(&state, collection) {
                                        Some(Value::List(items)) => match items.get(*index) {
                                            Some(Value::Object(item)) => {
                                                lookup_binding_ref(item, tail).cloned()
                                            }
                                            _ => None,
                                        },
                                        _ => None,
                                    }
                                } else {
                                    eval(
                                        &Value::Expr(path.clone()),
                                        &Properties::new(),
                                        &state,
                                        &mut Vec::new(),
                                    )
                                    .ok()
                                };
                            let checked = current
                                .and_then(|value| value.boolean().ok())
                                .unwrap_or(false);
                            Some(Value::Bool(!checked))
                        }
                        "selected" if activated_index == Some(idx) => Some(Value::Bool(true)),
                        "group" if activated_index == Some(idx) => {
                            src_node.props.get("__group_value").and_then(|value| {
                                eval(value, &Properties::new(), &state, &mut Vec::new()).ok()
                            })
                        }
                        _ => None,
                    };
                    let Some(val) = new_val else {
                        continue;
                    };
                    if let Some(state_path) = path.strip_prefix("state.") {
                        if write_state_path(state.to_mut(), state_path, val) {
                            changed += 1;
                            key_indexes.clear();
                        }
                    } else if let Some((collection, index, tail)) = collection_target {
                        if write_collection_item_path(
                            state.to_mut(),
                            &collection,
                            index,
                            &tail,
                            val,
                        ) {
                            changed += 1;
                            key_indexes.clear();
                        }
                    }
                }
            }
        }
        match state {
            std::borrow::Cow::Owned(state) => {
                if let Some(root) = self.states.first_mut() {
                    *root = state;
                } else {
                    self.states.push(state);
                }
            }
            std::borrow::Cow::Borrowed(_) => {
                if self.states.is_empty() {
                    self.states.push(Properties::new());
                }
            }
        }
        runtime.acknowledge_binding_widgets();
        Ok(changed)
    }

    /// Exports the root state map (`self.states[0]`) as a JSON object string.
    pub fn state_json(&self) -> Result<String, String> {
        let empty = Properties::new();
        let root_state = self.states.first().unwrap_or(&empty);
        let json_val = doc_props_to_json(root_state)?;
        serde_json::to_string(&json_val).map_err(|e| e.to_string())
    }
}
