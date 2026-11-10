//! Scene expansion: the root document with per-node state, node ids, keys and binding metadata,
//! and positioning of the resulting controls.

use super::*;

pub(super) struct SceneWalk {
    pub(super) budget: usize,
    pub(super) node_index: usize,
}

impl SceneWalk {
    pub(super) fn root() -> Self {
        Self {
            budget: 16_384,
            node_index: 0,
        }
    }
}

impl Document {
    pub(super) fn scene_properties(
        &self,
        node: &Node,
        scope: &Properties,
        state: &Properties,
    ) -> Result<Properties, String> {
        let definition = self.definitions.get(&node.kind);
        // Most nodes only read defaults; materialize the merged scope only when
        // a missing-state fallback expression actually needs it for evaluation.
        let default_value = |key: &str| {
            definition
                .and_then(|d| d.defaults.get(key))
                .or_else(|| self.fallback.get(key))
        };
        let mut scope = std::borrow::Cow::Borrowed(scope);
        for meta_key in ["__enums", "__tokens", "__i18n", "__lets", "__styles"] {
            if let Some(Value::Object(meta_obj)) = default_value(meta_key) {
                let scope = scope_mut(&mut scope);
                let mut merged = match scope.remove(meta_key) {
                    Some(Value::Object(v)) => v
                        .small_scope_clone(4096)
                        .unwrap_or_else(|| v.scoped_clone()),
                    _ => Properties::new(),
                };
                merged.extend(meta_obj.clone());
                scope.insert(meta_key.into(), Value::Object(merged));
            }
        }
        let mut result = Properties::new();
        let values = property_values(node, &scope, state)?;
        for (key, value) in &values {
            if key == "__matches" || key == "__forward" || key == "__scope" || key == "__styles" {
                continue;
            }
            match eval(value, &scope, state, &mut Vec::new()) {
                Ok(value) => {
                    result.insert(key.clone(), value);
                }
                Err(error) if error.starts_with("Missing state.") => {
                    if let Some(default) = default_value(key) {
                        let mut defaults = self.fallback.clone();
                        if let Some(definition) = definition {
                            defaults.extend(definition.defaults.clone());
                        }
                        result.insert(
                            key.clone(),
                            eval(default, &defaults, state, &mut Vec::new())?,
                        );
                    }
                }
                Err(error) => return Err(error),
            }
        }
        if result.get("__range_normalized") == Some(&Value::Bool(true)) {
            if let Some(Value::Number(value)) = result.get("value") {
                let normalized = Value::Expr(format!("{}%", value * 100.));
                result.insert("value".into(), normalized);
            }
        }
        Ok(result)
    }
    fn scene_expand_sequence_step(
        &self,
        child: &Node,
        state: &mut std::borrow::Cow<'_, Properties>,
        prefix: &str,
        scope: &Properties,
        depth: usize,
        walk: &mut SceneWalk,
        out: &mut Vec<Node>,
    ) -> Result<(), String> {
        if child.kind == "Let" {
            let mut child_node_index = walk.node_index;
            count_scene_subtree(child, &mut child_node_index, depth)?;
            if walk.budget == 0 {
                return Err("Scene exceeds 16384 nodes".into());
            }
            walk.budget -= 1;
            let mut inner_walk = SceneWalk {
                budget: walk.budget,
                node_index: walk.node_index + 1,
            };
            walk.node_index = child_node_index;
            if let Some((name, value, expression)) =
                control_flow::evaluate_let_binding(child, scope, state)?
            {
                if child.children.is_empty() {
                    insert_scene_lexical(scope_mut(state), name, value, expression, scope);
                } else {
                    let mut block_state = state.as_ref().scoped_clone();
                    insert_scene_lexical(&mut block_state, name, value, expression, scope);
                    out.extend(self.scene_expand_sequence(
                        &child.children,
                        &block_state,
                        prefix,
                        scope,
                        depth + 1,
                        &mut inner_walk,
                    )?);
                }
            } else if !child.children.is_empty() {
                out.extend(self.scene_expand_sequence(
                    &child.children,
                    state,
                    prefix,
                    scope,
                    depth + 1,
                    &mut inner_walk,
                )?);
            }
            walk.budget = inner_walk.budget;
        } else {
            out.extend(self.scene_expand(child, state, prefix, scope, depth, walk)?);
        }
        Ok(())
    }

    pub(super) fn scene_expand_sequence<'a>(
        &self,
        children: impl IntoIterator<Item = &'a Node>,
        inherited: &Properties,
        prefix: &str,
        scope: &Properties,
        depth: usize,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let mut state = std::borrow::Cow::Borrowed(inherited);
        let mut out = Vec::new();
        for child in children {
            self.scene_expand_sequence_step(
                child, &mut state, prefix, scope, depth, walk, &mut out,
            )?;
        }
        Ok(out)
    }
    pub(super) fn scene_expand(
        &self,
        node: &Node,
        inherited: &Properties,
        prefix: &str,
        inherited_props: &Properties,
        depth: usize,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let start_node_index = walk.node_index;
        let mut subtree_end = start_node_index;
        count_scene_subtree(node, &mut subtree_end, depth)?;
        walk.node_index = subtree_end;
        if walk.budget == 0 {
            return Err("Scene exceeds 16384 nodes".into());
        }
        walk.budget -= 1;
        let id = match node.props.get("__node") {
            Some(v) => v.number()? as usize,
            None => start_node_index,
        };
        let mut state = std::borrow::Cow::Borrowed(inherited);
        if let Some(local) = self.states.get(id) {
            if !local.is_empty() {
                scope_mut(&mut state).extend(local.clone());
            }
        }
        let mut scope = std::borrow::Cow::Borrowed(inherited_props);
        if let Some(Value::Object(values)) = node.props.get("__scope") {
            scope_mut(&mut scope).extend(values.clone());
            let selected = select_groups(values.get("__matches"), &scope, &state)?;
            scope_mut(&mut scope).extend(selected);
            let resolved = scope
                .iter()
                .filter(|(key, _)| !key.starts_with("__"))
                .map(|(key, value)| Ok((key.clone(), value.evaluate(&scope, &state)?)))
                .collect::<Result<Properties, String>>()?;
            let mut contract_values = std::borrow::Cow::Borrowed(&resolved);
            if let Some(Value::Object(contracts)) = values.get("__contracts") {
                for (key, contract) in contracts {
                    if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                        && !values.contains_key(key)
                        && contract_values.contains_key(key)
                    {
                        contract_values.to_mut().remove(key);
                    }
                }
            }
            check_contracts(
                &self.name,
                scope.get("__contracts"),
                scope.get("__enums"),
                &contract_values,
            )?;
            scope_mut(&mut scope).extend(resolved);
        }
        match node.kind.as_str() {
            "If" => {
                let condition = control_flow::evaluate_if_condition(node, &scope, &state)?;
                let mut child_walk = SceneWalk {
                    budget: walk.budget,
                    node_index: start_node_index + 1,
                };
                let res = if condition {
                    let mut seq_state = std::borrow::Cow::Borrowed(state.as_ref());
                    let mut out = Vec::new();
                    for child in &node.children {
                        if child.kind == "Else" {
                            count_scene_subtree(child, &mut child_walk.node_index, depth + 1)?;
                        } else {
                            self.scene_expand_sequence_step(
                                child,
                                &mut seq_state,
                                prefix,
                                &scope,
                                depth + 1,
                                &mut child_walk,
                                &mut out,
                            )?;
                        }
                    }
                    Ok(out)
                } else {
                    let mut out = Vec::new();
                    for child in &node.children {
                        if child.kind == "Else" {
                            let mut else_walk = SceneWalk {
                                budget: child_walk.budget,
                                node_index: child_walk.node_index + 1,
                            };
                            out = self.scene_expand_sequence(
                                &child.children,
                                &state,
                                prefix,
                                &scope,
                                depth + 1,
                                &mut else_walk,
                            )?;
                            child_walk.budget = else_walk.budget;
                            break;
                        }
                        count_scene_subtree(child, &mut child_walk.node_index, depth + 1)?;
                    }
                    Ok(out)
                };
                walk.budget = child_walk.budget;
                res
            }
            "For" => {
                let (items, item) = control_flow::evaluate_for_header(node, &scope, &state)?;
                let collection_expr = node.props.get("collection").and_then(|v| v.text().ok());
                let mut out = Vec::new();
                let mut keys = std::collections::BTreeSet::new();
                if items.is_empty() {
                    let mut child_idx = start_node_index + 1;
                    for child in &node.children {
                        if child.kind == "Empty" {
                            let mut empty_walk = SceneWalk {
                                budget: walk.budget,
                                node_index: child_idx + 1,
                            };
                            out.extend(self.scene_expand_sequence(
                                &child.children,
                                &state,
                                prefix,
                                &scope,
                                depth + 1,
                                &mut empty_walk,
                            )?);
                            walk.budget = empty_walk.budget;
                            break;
                        }
                        count_scene_subtree(child, &mut child_idx, depth + 1)?;
                    }
                }
                // Small loops with little owned data keep the cheaper flat
                // map path. Large loops/payloads retain shared-root overlays.
                let mut local = std::borrow::Cow::Owned(
                    if items.len() <= 32 {
                        state.small_scope_clone(4096)
                    } else {
                        None
                    }
                    .unwrap_or_else(|| state.as_ref().scoped_clone()),
                );
                // These captured outer locals are identical for every item.
                // Materialize them only after the first valid key, then share
                // the immutable map across retained binding frames.
                let mut captured_locals: Option<Properties> = None;
                let idx_key = format!("__index.{item}");
                let col_key = format!("__collection.{item}");
                let binding_metadata = [
                    format!("__binding_key.{item}"),
                    format!("__binding_key_expr.{item}"),
                    format!("__binding_key_scope.{item}"),
                ];
                for (index, value) in items.into_iter().enumerate() {
                    let local = scope_mut(&mut local);
                    // Key evaluation sees the inherited metadata, never the
                    // previous iteration's key (also when nested For shadows item).
                    for name in &binding_metadata {
                        match state.get(name) {
                            Some(value) => {
                                local.set(name.clone(), value.clone());
                            }
                            None => {
                                local.unset(name);
                            }
                        }
                    }
                    local.set(item.clone(), value);
                    if let Some(Value::Object(lexicals)) = local.get_mut("__binding_lexicals") {
                        if lexicals.contains_key(&item) {
                            let mut masked = lexicals.scoped_clone();
                            masked.unset(&item);
                            *lexicals = masked;
                        }
                    }
                    local.set(idx_key.clone(), number(index as f64));
                    if let Some(col_expr) = &collection_expr {
                        local.set(col_key.clone(), Value::Text(col_expr.clone()));
                    }
                    let key = key_part(
                        &node
                            .props
                            .get("key")
                            .ok_or("For requires a key")?
                            .evaluate(&scope, local)?,
                    )?;
                    if !keys.insert(key.clone()) {
                        return Err("Duplicate collection key".into());
                    }
                    // Retain key semantics, not just the old positional index,
                    // so a recovery binding map can resolve the same live item.
                    local.set(binding_metadata[0].clone(), Value::Text(key.clone()));
                    local.set(binding_metadata[1].clone(), node.props["key"].clone());
                    local.set(
                        binding_metadata[2].clone(),
                        Value::Object(scope.as_ref().clone()),
                    );
                    let mut frames = match state.get("__binding_frames") {
                        Some(Value::List(frames)) => frames.clone(),
                        _ => Vec::new(),
                    };
                    let mut active = match state.get("__binding_active") {
                        Some(Value::Object(active)) => active.clone(),
                        _ => Properties::new(),
                    };
                    // Lexicals are retained in __lexicals and replayed over locals.
                    // Avoid a second owned copy of each lexical collection here.
                    let locals = captured_locals.get_or_insert_with(|| state.iter().filter(|(name, _)| {
                        !name.starts_with("__")
                            && !self.states.first().is_some_and(|root| root.contains_key(*name))
                            && !matches!(state.get("__binding_lexicals"), Some(Value::Object(lexicals)) if lexicals.contains_key(*name))
                    }).map(|(name, value)| (name.clone(), value.clone())).collect()).clone();
                    let mut frame = Properties::from([
                        ("__item".into(), Value::Text(item.clone())),
                        ("__parents".into(), Value::Object(active.clone())),
                        ("__locals".into(), Value::Object(locals)),
                    ]);
                    if let Some(Value::Object(lexicals)) = state.get("__binding_lexicals") {
                        frame.insert("__lexicals".into(), Value::Object(lexicals.clone()));
                        if let Some(steps) = state.get("__binding_let_steps") {
                            frame.insert("__let_steps".into(), steps.clone());
                        }
                    }
                    if let Some(value) = local.get(&col_key) {
                        frame.insert(col_key.clone(), value.clone());
                    }
                    for name in &binding_metadata {
                        if let Some(value) = local.get(name) {
                            frame.insert(name.clone(), value.clone());
                        }
                    }
                    active.insert(item.clone(), number(frames.len() as f64));
                    frames.push(Value::Object(frame));
                    local.set("__binding_frames".into(), Value::List(frames));
                    local.set("__binding_active".into(), Value::Object(active));
                    let prefix = format!("{prefix}@{id}:{key}/");
                    let mut iter_walk = SceneWalk {
                        budget: walk.budget,
                        node_index: start_node_index + 1,
                    };
                    let mut iter_state = std::borrow::Cow::Borrowed(&*local);
                    for body_child in &node.children {
                        if body_child.kind == "Empty" {
                            count_scene_subtree(body_child, &mut iter_walk.node_index, depth + 1)?;
                        } else {
                            self.scene_expand_sequence_step(
                                body_child,
                                &mut iter_state,
                                &prefix,
                                &scope,
                                depth + 1,
                                &mut iter_walk,
                                &mut out,
                            )?;
                        }
                    }
                    walk.budget = iter_walk.budget;
                }
                Ok(out)
            }
            "Else" | "Empty" => Ok(Vec::new()),
            "Let" => {
                let mut out = Vec::new();
                let mut child_walk = SceneWalk {
                    budget: walk.budget,
                    node_index: start_node_index + 1,
                };
                if let Some((name, val, val_expr)) =
                    control_flow::evaluate_let_binding(node, &scope, &state)?
                {
                    let mut next_state = state.as_ref().scoped_clone();
                    insert_scene_lexical(&mut next_state, name, val, val_expr, &scope);
                    out.extend(self.scene_expand_sequence(
                        &node.children,
                        &next_state,
                        prefix,
                        &scope,
                        depth + 1,
                        &mut child_walk,
                    )?);
                } else if !node.children.is_empty() {
                    out.extend(self.scene_expand_sequence(
                        &node.children,
                        &state,
                        prefix,
                        &scope,
                        depth + 1,
                        &mut child_walk,
                    )?);
                }
                walk.budget = child_walk.budget;
                Ok(out)
            }
            "Match" => {
                let subject = node
                    .props
                    .get("subject")
                    .ok_or("Match requires a subject")?
                    .evaluate(&scope, &state)?;
                let mut out = Vec::new();
                let mut child_idx = start_node_index + 1;
                for branch in &node.children {
                    let branch_start = child_idx;
                    count_scene_subtree(branch, &mut child_idx, depth + 1)?;
                    if control_flow::branch_matches(branch, &subject, &scope, &state)? {
                        let mut branch_walk = SceneWalk {
                            budget: walk.budget,
                            node_index: branch_start + 1,
                        };
                        out.extend(self.scene_expand_sequence(
                            &branch.children,
                            &state,
                            prefix,
                            &scope,
                            depth + 1,
                            &mut branch_walk,
                        )?);
                        walk.budget = branch_walk.budget;
                        break;
                    }
                }
                Ok(out)
            }
            _ => {
                let mut props = self.scene_properties(node, &scope, &state)?;
                props.entry("__node".into()).or_insert(number(id as f64));
                let mut output = Node {
                    kind: node.kind.clone(),
                    props,
                    children: Vec::new(),
                    action: node.action.clone(),
                };
                if self.definitions.contains_key(&node.kind) {
                    let explicit = output
                        .props
                        .get("key")
                        .map(Value::text)
                        .transpose()?
                        .unwrap_or_default();
                    let key = if prefix.is_empty() && !explicit.is_empty() {
                        explicit
                    } else {
                        format!("{prefix}@{id}:{}:{explicit}", explicit.len())
                    };
                    output.props.insert("key".into(), Value::Text(key));
                }
                if self.definitions.contains_key(&node.kind) {
                    // Supplied content resolves props in the receiving component's scope.
                    let mut child_idx = start_node_index + 1;
                    let mut annotated_children = Vec::with_capacity(node.children.len());
                    for child in &node.children {
                        annotated_children.push(annotated_scene_source(
                            child,
                            &mut child_idx,
                            depth + 1,
                        )?);
                    }
                    output.children = annotated_children;
                } else {
                    let mut child_walk = SceneWalk {
                        budget: walk.budget,
                        node_index: start_node_index + 1,
                    };
                    output.children = self.scene_expand_sequence(
                        &node.children,
                        &state,
                        prefix,
                        &scope,
                        depth + 1,
                        &mut child_walk,
                    )?;
                    walk.budget = child_walk.budget;
                }
                // Children and resolved properties no longer borrow this scope.
                // Move a local owned state into its public control snapshot;
                // inherited shared state is cloned only when ownership is needed.
                // Layout-only nodes do not consume snapshots.
                if !is_layout(&node.kind) {
                    output
                        .props
                        .insert("__state".into(), Value::Object(state.into_owned()));
                }
                Ok(vec![output])
            }
        }
    }
    pub(super) fn scene_visual(&self, node: &mut Node) -> Result<Node, String> {
        if is_layout(&node.kind) {
            let visual = Node {
                kind: node.kind.clone(),
                props: node.props.clone(),
                children: node
                    .children
                    .iter_mut()
                    .map(|n| self.scene_visual(n))
                    .collect::<Result<_, _>>()?,
                action: node.action.clone(),
            };
            normalize_layout_owned(visual)
        } else {
            self.instantiate_scene(node).map(|(mut root, p)| {
                let orig = root.props.clone();
                root.props.extend(p);
                root.props
                    .insert("__rect_props".into(), Value::Object(orig));
                root
            })
        }
    }
    pub(super) fn position_scene(
        &self,
        mut node: Node,
        precomputed_visual: Option<Node>,
        bounds: [f64; 4],
        instances: &mut Vec<(Node, Option<Node>)>,
        root_legacy: bool,
        logical_end: &mut [f64; 2],
        scroll_content: bool,
    ) -> Result<(), String> {
        // Oversized scroll content must overflow toward positive offsets; centering
        // it at a negative origin makes the leading half unreachable by scrolling.
        let align = |available: f64, size: f64| {
            let offset = (available - size) / 2.;
            if scroll_content {
                offset.max(0.)
            } else {
                offset
            }
        };
        if is_layout(&node.kind) {
            logical_end[0] = logical_end[0].max(bounds[0] + bounds[2]);
            logical_end[1] = logical_end[1].max(bounds[1] + bounds[3]);
        }
        if !is_layout(&node.kind) {
            let visual = match precomputed_visual {
                Some(v) => v,
                None => self.scene_visual(&mut node)?,
            };
            let width = dimension(&visual, 0, bounds[2])?;
            let height = dimension(&visual, 1, bounds[3])?;
            for (key, value) in [
                ("x", bounds[0] + align(bounds[2], width)),
                ("y", bounds[1] + align(bounds[3], height)),
                ("width", width),
                ("height", height),
            ] {
                node.props.insert(key.into(), number(value));
            }
            for key in ["cell", "row", "column"] {
                node.props.remove(key);
            }
            instances.push((node, Some(visual)));
            return Ok(());
        }
        if !root_legacy && boolean(&node.props, "clip", false)? {
            return Err("Nested scene clipping is unsupported; use the root Frame clip".into());
        }
        if root_legacy
            && ["Frame", "Scroll"].contains(&node.kind.as_str())
            && !node.props.contains_key("columns")
            && !node.props.contains_key("rows")
        {
            node.kind = "Column".into();
        }
        let mut node = normalize_layout_owned(node)?;
        let visual = match precomputed_visual {
            Some(v) if v.kind == node.kind && v.children.len() == node.children.len() => v,
            _ => self.scene_visual(&mut node)?,
        };
        let [mut x, mut y, mut width, mut height] = bounds;
        // A container consumes its allocated cell; its explicit dimensions are resolved by its parent.
        let pad = insets(node.props.get("padding"))?;
        x += pad[3];
        y += pad[0];
        width = (width - pad[1] - pad[3]).max(0.);
        height = (height - pad[0] - pad[2]).max(0.);
        let cols = sizes(&visual, 0, width)?;
        let rows = sizes(&visual, 1, height)?;
        let gap = gaps(&node.props)?;
        let mut col_offsets = Vec::with_capacity(cols.len());
        let mut col_acc = 0.0;
        for (i, &cw) in cols.iter().enumerate() {
            col_offsets.push(col_acc + i as f64 * gap[1]);
            col_acc += cw;
        }
        let mut row_offsets = Vec::with_capacity(rows.len());
        let mut row_acc = 0.0;
        for (i, &rh) in rows.iter().enumerate() {
            row_offsets.push(row_acc + i as f64 * gap[0]);
            row_acc += rh;
        }
        for (child, child_visual) in node.children.into_iter().zip(visual.children) {
            let ci = cell(&child, 0, cols.len())?;
            let ri = cell(&child, 1, rows.len())?;
            let cw = ci.map_or(width, |i| cols[i]);
            let ch = ri.map_or(height, |i| rows[i]);
            let mut cx = x + ci.map_or(0., |i| col_offsets[i]);
            let mut cy = y + ri.map_or(0., |i| row_offsets[i]);
            if is_layout(&child.kind) {
                let w = dimension(&child_visual, 0, cw)?;
                let h = dimension(&child_visual, 1, ch)?;
                cx += align(cw, w);
                cy += align(ch, h);
                self.position_scene(
                    child,
                    Some(child_visual),
                    [cx, cy, w, h],
                    instances,
                    false,
                    logical_end,
                    scroll_content,
                )?;
            } else {
                self.position_scene(
                    child,
                    Some(child_visual),
                    [cx, cy, cw, ch],
                    instances,
                    false,
                    logical_end,
                    scroll_content,
                )?;
            }
        }
        Ok(())
    }
}
