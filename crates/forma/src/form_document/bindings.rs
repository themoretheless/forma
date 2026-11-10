use std::collections::BTreeMap;

use super::WidgetState;

use super::{
    Document, Properties, SceneControl, Value, collect_preorder_nodes, doc_props_to_json,
    enum_value, eval, json_to_doc_props, key_part, lookup, number, scope_mut,
};

/// Application state on its way into a retained document's root scope.
/// Values use the same JSON conversion as the initial state of a UI project.
pub(crate) struct StatePush {
    entries: Properties,
    paths: bool,
}
// A path is walked recursively, so its depth is bounded like scene composition.
const MAX_STATE_PATH_SEGMENTS: usize = 64;
impl StatePush {
    /// Every entry replaces one root state field as a whole.
    pub(crate) fn fields(entries: Properties) -> Self {
        Self {
            entries,
            paths: false,
        }
    }
    /// `paths` reads each key as a dotted path (`user.name`, `items.2.done`).
    pub(crate) fn from_json(json: &str, paths: bool) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("Invalid state JSON: {e}"))?;
        if !value.is_object() {
            return Err("State must be a JSON object".into());
        }
        let entries = json_to_doc_props(&value)?;
        if paths {
            for path in entries.keys() {
                let mut segments = 0usize;
                for segment in path.split('.') {
                    segments += 1;
                    if segment.is_empty() {
                        return Err(format!("Invalid state path '{path}'"));
                    }
                }
                if segments > MAX_STATE_PATH_SEGMENTS {
                    return Err(format!(
                        "State path exceeds {MAX_STATE_PATH_SEGMENTS} segments"
                    ));
                }
            }
        }
        Ok(Self { entries, paths })
    }
    /// The caller applies to a copy: a refused path may follow accepted ones.
    pub(crate) fn apply(self, root: &mut Properties) -> Result<(), String> {
        for (key, value) in self.entries {
            if self.paths {
                push_state_path(root, &key, &key, value)?;
            } else {
                root.insert(key, value);
            }
        }
        Ok(())
    }
}

// Unlike binding writes, a pushed path that cannot be reached is an error:
// the application named it, so a silent miss would hide a stale screen.
fn push_state_path(
    target: &mut Properties,
    path: &str,
    full: &str,
    value: Value,
) -> Result<(), String> {
    let Some((head, tail)) = path.split_once('.') else {
        target.insert(path.into(), value);
        return Ok(());
    };
    match target.get_mut(head) {
        Some(Value::Object(object)) => push_state_path(object, tail, full, value),
        Some(Value::List(items)) => push_state_item(items, tail, full, value),
        Some(Value::Null) | None => {
            let mut nested = Properties::new();
            push_state_path(&mut nested, tail, full, value)?;
            target.insert(head.into(), Value::Object(nested));
            Ok(())
        }
        Some(_) => Err(format!(
            "State path '{full}': '{head}' is not an object or list"
        )),
    }
}

fn push_state_item(
    items: &mut [Value],
    path: &str,
    full: &str,
    value: Value,
) -> Result<(), String> {
    let (index, tail) = path
        .split_once('.')
        .map_or((path, None), |(index, tail)| (index, Some(tail)));
    let Some(target) = index
        .parse::<usize>()
        .ok()
        .and_then(|index| items.get_mut(index))
    else {
        return Err(format!("State path '{full}': no list item '{index}'"));
    };
    match (tail, target) {
        (None, target) => {
            *target = value;
            Ok(())
        }
        (Some(tail), Value::Object(object)) => push_state_path(object, tail, full, value),
        (Some(tail), Value::List(items)) => push_state_item(items, tail, full, value),
        (Some(_), _) => Err(format!(
            "State path '{full}': item '{index}' is not an object or list"
        )),
    }
}

impl Document {
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
                        "checked" | "selected" => activated_index == Some(idx),
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
                                    Some(number(r as f64))
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

// Borrow real model fields instead of cloning a whole collection to inspect a
// member. Synthetic length properties are intentionally not collection targets.
pub(super) fn lookup_binding_ref<'a>(values: &'a Properties, path: &str) -> Option<&'a Value> {
    if let Some(value) = values.get(path) {
        return Some(value);
    }
    let (head, tail) = path.split_once('.')?;
    lookup_binding_value_ref(values.get(head)?, tail)
}

fn lookup_binding_value_ref<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => lookup_binding_ref(object, path),
        Value::List(items) => {
            let (index, tail) = path
                .split_once('.')
                .map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get(index.parse::<usize>().ok()?)?;
            match tail {
                Some(tail) => lookup_binding_value_ref(value, tail),
                None => Some(value),
            }
        }
        _ => None,
    }
}

pub(super) fn lookup_binding_mut<'a>(
    values: &'a mut Properties,
    path: &str,
) -> Option<&'a mut Value> {
    if values.contains_key(path) {
        return values.get_mut(path);
    }
    let (head, tail) = path.split_once('.')?;
    lookup_binding_value_mut(values.get_mut(head)?, tail)
}

fn lookup_binding_value_mut<'a>(value: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    match value {
        Value::Object(object) => lookup_binding_mut(object, path),
        Value::List(items) => {
            let (index, tail) = path
                .split_once('.')
                .map_or((path, None), |(index, tail)| (index, Some(tail)));
            let value = items.get_mut(index.parse::<usize>().ok()?)?;
            match tail {
                Some(tail) => lookup_binding_value_mut(value, tail),
                None => Some(value),
            }
        }
        _ => None,
    }
}

/// Resolve the retained item's stable key against the current root collection.
/// Never route a stale positional index into a different object.
pub(super) type BindingKeyIndexes = BTreeMap<String, BTreeMap<String, BTreeMap<String, usize>>>;

#[cfg(test)]
pub(super) fn resolve_binding_item(
    control: &SceneControl,
    item: &str,
    state: &Properties,
) -> Result<Option<(String, usize)>, String> {
    resolve_binding_item_cached(control, item, state, &mut BindingKeyIndexes::new())
}

fn resolve_binding_item_cached(
    control: &SceneControl,
    item: &str,
    state: &Properties,
    indexes: &mut BindingKeyIndexes,
) -> Result<Option<(String, usize)>, String> {
    match (
        control.state.get("__binding_frames"),
        control.state.get("__binding_active"),
    ) {
        (Some(Value::List(frames)), Some(Value::Object(active))) => {
            if frames.len() > 64 {
                return Err("Binding frames exceed lexical depth limit".into());
            }
            let Some(index) = active.get(item) else {
                return Ok(None);
            };
            let Value::Number(index) = index else {
                return Err("Invalid active binding frame".into());
            };
            if !index.is_finite() || *index < 0. || index.fract() != 0. {
                return Err("Invalid active binding frame".into());
            }
            let Some(Value::Object(frame)) = frames.get(*index as usize) else {
                return Err("Invalid active binding frame".into());
            };
            if !matches!(frame.get("__item"), Some(Value::Text(name)) if name == item) {
                return Err("Binding frame item mismatch".into());
            }
            let Some(Value::Object(parents)) = frame.get("__parents") else {
                return Err("Invalid binding frame parents".into());
            };
            if !parents.is_empty()
                || matches!(frame.get("__lexicals"), Some(Value::Object(locals)) if !locals.is_empty())
            {
                return resolve_binding_frame(
                    frames,
                    *index as usize,
                    state,
                    indexes,
                    &mut BTreeMap::new(),
                    0,
                );
            }
        }
        (None, None) => {} // Legacy maps without lexical frames remain readable.
        _ => return Err("Invalid lexical binding metadata".into()),
    }
    resolve_binding_item_inner(
        &control.state,
        item,
        state,
        indexes,
        &mut Properties::new(),
        0,
        None,
    )
}

// Track only direct model/member aliases. Computed collections do not acquire
// a writable origin merely because their current values resemble a model list.
fn binding_model_path(
    expression: &str,
    parents: &BTreeMap<String, (String, usize)>,
    aliases: &BTreeMap<String, Option<String>>,
    lexical_names: &Properties,
    state: &Properties,
) -> Option<String> {
    let path = expression.strip_prefix("state.").unwrap_or(expression);
    if path.is_empty()
        || !path.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    {
        return None;
    }
    let (head, tail) = path
        .split_once('.')
        .map_or((path, ""), |(head, tail)| (head, tail));
    if !expression.starts_with("state.") && ["props", "tokens", "i18n"].contains(&head) {
        return None;
    }
    let append = |base: String| {
        if tail.is_empty() {
            base
        } else {
            format!("{base}.{tail}")
        }
    };
    if path.contains('.') && state.contains_key(path) {
        return Some(path.to_owned());
    }
    if let Some((collection, index)) = parents.get(head) {
        return Some(append(format!("{collection}.{index}")));
    }
    if let Some(alias) = aliases.get(head) {
        return alias.as_ref().map(|alias| append(alias.clone()));
    }
    if lexical_names.contains_key(head) {
        return None;
    }
    state.contains_key(head).then(|| path.to_owned())
}

// A direct model alias and scalar item key need no owned lexical values.
// Unsupported expressions/parents continue through the general replay below.
fn resolve_direct_alias_frame(
    frame: &Properties,
    state: &Properties,
    indexes: &mut BindingKeyIndexes,
    depth: usize,
) -> Option<Result<Option<(String, usize)>, String>> {
    let empty = Properties::new();
    let Value::Object(parents) = frame.get("__parents")? else {
        return None;
    };
    if !parents.is_empty() {
        return None;
    }
    let Value::Object(lexicals) = frame.get("__lexicals")? else {
        return None;
    };
    let Value::List(steps) = frame.get("__let_steps")? else {
        return None;
    };
    if steps.len() > 16_384 {
        return None;
    }
    let item = frame.get("__item")?.text().ok()?;
    let expression = frame.get(&format!("__binding_key_expr.{item}"))?;
    let scope = match frame.get(&format!("__binding_key_scope.{item}")) {
        Some(Value::Object(scope)) => scope,
        _ => &empty,
    };
    let locals = match frame.get("__locals") {
        Some(Value::Object(locals)) => locals,
        _ => &empty,
    };
    let member = direct_binding_key_member(expression, scope, frame, &item, state, locals)?;
    let Value::Expr(key_path) = expression else {
        return None;
    };
    if lexicals.contains_key(key_path) {
        return None;
    }
    let mut aliases = BTreeMap::new();
    for step in steps {
        let Value::Object(step) = step else {
            return None;
        };
        let name = step.get("name")?.text().ok()?;
        let Value::Expr(expression) = step.get("expression")? else {
            return None;
        };
        if matches!(step.get("parents"), Some(Value::Object(parents)) if !parents.is_empty()) {
            return None;
        }
        let scope = match step.get("scope") {
            Some(Value::Object(scope)) => scope,
            _ => &empty,
        };
        let head = expression.split('.').next()?;
        if !expression.starts_with("state.")
            && (!aliases.contains_key(head) || enum_value(expression, scope).is_some())
        {
            return None;
        }
        let origin = binding_model_path(expression, &BTreeMap::new(), &aliases, &empty, state)?;
        let value = lookup_binding_ref(state, &origin)?;
        if matches!(
            value,
            Value::Expr(_) | Value::Expression(_) | Value::Interpolation(_) | Value::Match(..)
        ) {
            return None;
        }
        aliases.insert(name, Some(origin));
    }
    let collection = frame.get(&format!("__collection.{item}"))?.text().ok()?;
    let path = binding_model_path(&collection, &BTreeMap::new(), &aliases, lexicals, state)?;
    let Some(Value::List(items)) = lookup_binding_ref(state, &path) else {
        return None;
    };
    if items.len() > 16_384 {
        return None;
    }
    // Cached indexes are installed only after every key was a pure scalar read.
    // Otherwise replay is required for computed key values and their lexicals.
    if indexes
        .get(&path)
        .and_then(|members| members.get(member))
        .is_none()
        && items.iter().any(|item| {
            !matches!(item, Value::Object(object)
                if matches!(lookup_binding_ref(object, member), Some(Value::Text(_) | Value::Number(_))))
        })
    {
        return None;
    }
    Some(resolve_binding_item_inner(
        frame,
        &item,
        state,
        indexes,
        &mut Properties::new(),
        depth + 1,
        Some(&format!("state.{path}")),
    ))
}

fn direct_binding_key_member<'a>(
    expression: &'a Value,
    scope: &Properties,
    metadata: &Properties,
    item: &str,
    state: &Properties,
    live_locals: &Properties,
) -> Option<&'a str> {
    let Value::Expr(expression) = expression else {
        return None;
    };
    if ["state", "props", "tokens", "i18n"].contains(&item) {
        return None;
    }
    let tail = expression
        .strip_prefix(item)
        .and_then(|rest| rest.strip_prefix('.'))?;
    (!tail.is_empty()
        && tail.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        && !state.contains_key(expression)
        && !metadata.contains_key(expression)
        && !live_locals.contains_key(expression)
        && enum_value(expression, scope).is_none())
    .then_some(tail)
}

pub(super) fn resolve_binding_frame(
    frames: &[Value],
    index: usize,
    state: &Properties,
    indexes: &mut BindingKeyIndexes,
    resolved: &mut BTreeMap<usize, Option<(String, usize)>>,
    depth: usize,
) -> Result<Option<(String, usize)>, String> {
    if depth >= 64 {
        return Err("Collection binding lexical depth exceeded".into());
    }
    if let Some(result) = resolved.get(&index) {
        return Ok(result.clone());
    }
    let Some(Value::Object(frame)) = frames.get(index) else {
        return Err("Invalid collection binding frame".into());
    };
    if let Some(result) = resolve_direct_alias_frame(frame, state, indexes, depth) {
        let result = result?;
        resolved.insert(index, result.clone());
        return Ok(result);
    }
    let item = frame
        .get("__item")
        .ok_or("Missing binding frame item")?
        .text()?;
    let collection_name = format!("__collection.{item}");
    let Some(collection) = frame.get(&collection_name) else {
        return Ok(None);
    };
    let collection = collection.text()?;
    let empty = Properties::new();
    let parents = match frame.get("__parents") {
        Some(Value::Object(parents)) => parents,
        _ => &empty,
    };
    let lexical_names = match frame.get("__lexicals") {
        Some(Value::Object(locals)) => locals,
        _ => &empty,
    };
    // Replayed lexicals replace these names below. Do not deep-clone captured
    // values that would only be discarded before key resolution.
    let mut locals = match frame.get("__locals") {
        Some(Value::Object(locals)) if lexical_names.is_empty() => locals.clone(),
        Some(Value::Object(locals)) => locals
            .iter()
            .filter(|(name, _)| !lexical_names.contains_key(*name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        _ => Properties::new(),
    };
    let mut parent_paths = BTreeMap::new();
    for (name, value) in parents {
        let parent_index = value.number()?;
        if !parent_index.is_finite()
            || parent_index < 0.
            || parent_index.fract() != 0.
            || parent_index as usize >= index
        {
            return Err("Invalid collection binding parent frame".into());
        }
        let Some((path, item_index)) = resolve_binding_frame(
            frames,
            parent_index as usize,
            state,
            indexes,
            resolved,
            depth + 1,
        )?
        else {
            return Ok(None);
        };
        let Some(Value::List(items)) = lookup_binding_ref(state, &path) else {
            return Err("Binding parent expects a list".into());
        };
        let Some(value) = items.get(item_index) else {
            return Ok(None);
        };
        if !lexical_names.contains_key(name) {
            locals.insert(name.clone(), value.clone());
        }
        parent_paths.insert(name.clone(), (path, item_index));
    }
    let mut aliases = BTreeMap::new();
    if let Some(Value::Object(lexicals)) = frame.get("__lexicals") {
        let mut evaluated = Properties::new();
        if let Some(Value::List(steps)) = frame.get("__let_steps") {
            if steps.len() > 16_384 {
                return Err("Binding Let steps exceed 16384".into());
            }
            let mut environment = std::borrow::Cow::Borrowed(state);
            let mut overlay_names: Vec<String> = Vec::new();
            for step in steps {
                let Value::Object(step) = step else {
                    return Err("Invalid binding Let step".into());
                };
                let name = step.get("name").ok_or("Missing binding Let name")?.text()?;
                let expression = step
                    .get("expression")
                    .ok_or("Missing binding Let expression")?;
                let scope = match step.get("scope") {
                    Some(Value::Object(scope)) => scope,
                    _ => &empty,
                };
                // The first root Let can evaluate against borrowed state. Own
                // an environment only when a previous lexical/parent is overlaid.
                if !overlay_names.is_empty() || !evaluated.is_empty() {
                    let environment = scope_mut(&mut environment);
                    for name in overlay_names.drain(..) {
                        match state.get(&name) {
                            Some(value) => {
                                environment.set(name, value.clone());
                            }
                            None => {
                                environment.unset(&name);
                            }
                        }
                    }
                    environment.extend(evaluated.clone());
                    overlay_names.extend(evaluated.keys().cloned());
                }
                let mut step_parent_paths = BTreeMap::new();
                if let Some(Value::Object(parents)) = step.get("parents") {
                    for (name, value) in parents {
                        let parent_index = value.number()?;
                        if !parent_index.is_finite()
                            || parent_index < 0.
                            || parent_index.fract() != 0.
                            || parent_index as usize >= index
                        {
                            return Err("Invalid Let parent frame".into());
                        }
                        let Some((path, item_index)) = resolve_binding_frame(
                            frames,
                            parent_index as usize,
                            state,
                            indexes,
                            resolved,
                            depth + 1,
                        )?
                        else {
                            return Ok(None);
                        };
                        let Some(Value::List(items)) = lookup_binding_ref(state, &path) else {
                            return Err("Let parent expects a list".into());
                        };
                        let Some(value) = items.get(item_index) else {
                            return Ok(None);
                        };
                        scope_mut(&mut environment).set(name.clone(), value.clone());
                        overlay_names.push(name.clone());
                        step_parent_paths.insert(name.clone(), (path, item_index));
                    }
                }
                let origin = match expression {
                    Value::Expr(path) => {
                        binding_model_path(path, &step_parent_paths, &aliases, &evaluated, state)
                    }
                    _ => None,
                };
                let value = expression.evaluate(scope, &environment)?;
                aliases.insert(name.clone(), origin);
                evaluated.insert(name, value);
            }
        }
        for (name, value) in lexicals {
            locals.insert(
                name.clone(),
                evaluated.remove(name).unwrap_or_else(|| value.clone()),
            );
        }
    }
    let Some(path) = binding_model_path(&collection, &parent_paths, &aliases, lexical_names, state)
    else {
        return Ok(None);
    };
    // Override only the resolved collection path; keep the retained lexical
    // frame and captured props scope borrowed, including computed key values.
    let resolved_collection = format!("state.{path}");
    let result = resolve_binding_item_inner(
        frame,
        &item,
        state,
        indexes,
        &mut locals,
        depth + 1,
        Some(&resolved_collection),
    )?;
    resolved.insert(index, result.clone());
    Ok(result)
}

fn resolve_binding_item_inner(
    metadata: &Properties,
    item: &str,
    state: &Properties,
    indexes: &mut BindingKeyIndexes,
    live_locals: &mut Properties,
    depth: usize,
    collection_override: Option<&str>,
) -> Result<Option<(String, usize)>, String> {
    if depth >= 64 {
        return Err("Collection binding parent depth exceeded".into());
    }
    let collection = match collection_override {
        Some(collection) => std::borrow::Cow::Borrowed(collection),
        None => {
            let Some(collection) = metadata.get(&format!("__collection.{item}")) else {
                return Ok(None);
            };
            std::borrow::Cow::Owned(collection.text()?)
        }
    };
    let nested_path;
    let path = if let Some(path) = collection.strip_prefix("state.") {
        path
    } else {
        let Some((parent, tail)) = collection.split_once('.') else {
            return Ok(None);
        };
        let parent_metadata = match metadata.get(&format!("__binding_parent.{item}")) {
            Some(Value::Object(parent)) => parent,
            _ => metadata,
        };
        let Some((parent_path, parent_index)) = resolve_binding_item_inner(
            parent_metadata,
            parent,
            state,
            indexes,
            live_locals,
            depth + 1,
            None,
        )?
        else {
            return Ok(None);
        };
        let Some(Value::List(parents)) = lookup_binding_ref(state, &parent_path) else {
            return Ok(None);
        };
        let Some(parent_value) = parents.get(parent_index) else {
            return Ok(None);
        };
        live_locals.insert(parent.to_owned(), parent_value.clone());
        nested_path = format!("{parent_path}.{parent_index}.{tail}");
        &nested_path
    };
    let key = metadata
        .get(&format!("__binding_key.{item}"))
        .ok_or("Missing stable collection binding key")?
        .text()?;
    let key_expression = metadata
        .get(&format!("__binding_key_expr.{item}"))
        .ok_or("Missing collection binding key expression")?;
    let empty_scope = Properties::new();
    let key_scope = match metadata.get(&format!("__binding_key_scope.{item}")) {
        Some(Value::Object(scope)) => scope,
        _ => &empty_scope,
    };
    let Some(Value::List(items)) = lookup_binding_ref(state, path) else {
        return Err("Binding collection expects a list".into());
    };
    if items.len() > 16_384 {
        return Err("Binding collection exceeds 16384 items".into());
    }
    // Plain item-member keys can be read directly. Preserve evaluator precedence
    // for exact dotted fields, enums, reserved namespaces and computed values.
    let direct_path = direct_binding_key_member(
        key_expression,
        key_scope,
        metadata,
        item,
        state,
        live_locals,
    );
    if let Some(member) = direct_path {
        if let Some(keys) = indexes.get(path).and_then(|members| members.get(member)) {
            return Ok(keys.get(&key).map(|index| (path.to_owned(), *index)));
        }
    }
    let mut local = None;
    let mut keys = BTreeMap::new();
    let mut found = None;
    for (index, value) in items.iter().enumerate() {
        let direct = direct_path
            .and_then(|path| match value {
                Value::Object(object) => lookup_binding_ref(object, path),
                _ => None,
            })
            .filter(|value| matches!(value, Value::Text(_) | Value::Number(_)));
        let current_key = if let Some(value) = direct {
            key_part(value)?
        } else {
            let local = local.get_or_insert_with(|| {
                let mut local = state.scoped_clone();
                for (name, value) in metadata {
                    if name != item && !name.starts_with("__") && !local.contains_key(name) {
                        local.insert(name.clone(), value.clone());
                    }
                }
                for (name, value) in live_locals.iter() {
                    local.insert(name.clone(), value.clone());
                }
                local
            });
            local.insert(item.into(), value.clone());
            key_part(&key_expression.evaluate(key_scope, local)?)?
        };
        if current_key == key {
            found = Some((path.to_owned(), index));
        }
        if keys.insert(current_key, index).is_some() {
            return Err("Duplicate collection binding key".into());
        }
    }
    // Only pure member reads are independent of the retained control's scope.
    // The transaction discards these indexes after every successful model write.
    if local.is_none() {
        if let Some(member) = direct_path {
            indexes
                .entry(path.to_owned())
                .or_default()
                .insert(member.to_owned(), keys);
        }
    }
    Ok(found)
}

pub(super) fn write_collection_item_path(
    target: &mut Properties,
    col_path: &str,
    item_idx: usize,
    item_tail: &str,
    value: Value,
) -> bool {
    match lookup_binding_mut(target, col_path) {
        Some(Value::List(items)) => match items.get_mut(item_idx) {
            Some(Value::Object(item_obj)) => write_state_path(item_obj, item_tail, value),
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn write_state_path(target: &mut Properties, path: &str, value: Value) -> bool {
    if let Some((head, tail)) = path.split_once('.') {
        match target.get_mut(head) {
            Some(Value::Object(obj)) => write_state_path(obj, tail, value),
            Some(Value::List(items)) => write_list_path(items, tail, value),
            _ => {
                let mut nested = Properties::new();
                write_state_path(&mut nested, tail, value);
                target.insert(head.into(), Value::Object(nested));
                true
            }
        }
    } else {
        if target.get(path) == Some(&value) {
            return false;
        }
        target.insert(path.into(), value);
        true
    }
}

fn write_list_path(items: &mut [Value], path: &str, value: Value) -> bool {
    let (index, tail) = path
        .split_once('.')
        .map_or((path, None), |(index, tail)| (index, Some(tail)));
    let Some(target) = index
        .parse::<usize>()
        .ok()
        .and_then(|index| items.get_mut(index))
    else {
        return false;
    };
    match tail {
        None => {
            if *target == value {
                return false;
            }
            *target = value;
            true
        }
        Some(tail) => match target {
            Value::Object(object) => write_state_path(object, tail, value),
            Value::List(items) => write_list_path(items, tail, value),
            _ => false,
        },
    }
}
