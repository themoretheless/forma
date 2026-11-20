//! Expanding the control-flow nodes: `If`, `For`, `Let` and `Match`.

use super::*;

impl Document {
    /// An `If`: the taken branch, with the node indices of the other one skipped.
    pub(super) fn expand_if(
        &self,
        at: &SceneAt<'_>,
        state: &Properties,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let SceneAt {
            node,
            start: start_node_index,
            prefix,
            scope,
            depth,
            ..
        } = *at;
        let condition = control_flow::evaluate_if_condition(node, scope, state)?;
        let mut child_walk = SceneWalk {
            budget: walk.budget,
            node_index: start_node_index + 1,
        };
        let res = if condition {
            let mut seq_state = std::borrow::Cow::Borrowed(state);
            let mut out = Vec::new();
            for child in &node.children {
                if child.kind == BuiltinKind::Else {
                    count_scene_subtree(child, &mut child_walk.node_index, depth + 1)?;
                } else {
                    self.scene_expand_sequence_step(
                        child,
                        &mut seq_state,
                        SceneSequenceEnv {
                            prefix,
                            scope,
                            depth: depth + 1,
                        },
                        &mut child_walk,
                        &mut out,
                    )?;
                }
            }
            Ok(out)
        } else {
            let mut out = Vec::new();
            for child in &node.children {
                if child.kind == BuiltinKind::Else {
                    let mut else_walk = SceneWalk {
                        budget: child_walk.budget,
                        node_index: child_walk.node_index + 1,
                    };
                    out = self.scene_expand_sequence(
                        &child.children,
                        state,
                        prefix,
                        scope,
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

    /// A `For`: its body once per item under a keyed prefix, or its `Empty` block.
    pub(super) fn expand_for(
        &self,
        at: &SceneAt<'_>,
        state: &Properties,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let SceneAt {
            node,
            id,
            start: start_node_index,
            prefix,
            scope,
            depth,
        } = *at;
        let (items, item) = control_flow::evaluate_for_header(node, scope, state)?;
        let collection_expr = node.props.get("collection").and_then(|v| v.text().ok());
        let mut out = Vec::new();
        let mut keys = std::collections::BTreeSet::new();
        if items.is_empty() {
            out.extend(self.expand_for_empty(at, state, walk)?);
        }
        // Small loops with little owned data keep the cheaper flat
        // map path. Large loops/payloads retain shared-root overlays.
        let mut local = std::borrow::Cow::Owned(
            if items.len() <= 32 {
                state.small_scope_clone(4096)
            } else {
                None
            }
            .unwrap_or_else(|| state.scoped_clone()),
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
                    .evaluate(scope, local)?,
            )?;
            if !keys.insert(key.clone()) {
                return Err("Duplicate collection key".into());
            }
            // Retain key semantics, not just the old positional index,
            // so a recovery binding map can resolve the same live item.
            local.set(binding_metadata[0].clone(), Value::Text(key.clone()));
            local.set(binding_metadata[1].clone(), node.props["key"].clone());
            local.set(binding_metadata[2].clone(), Value::Object(scope.clone()));
            self.retain_binding_frame(
                state,
                local,
                &item,
                &col_key,
                &binding_metadata,
                &mut captured_locals,
            );
            let prefix = format!("{prefix}@{id}:{key}/");
            let mut iter_walk = SceneWalk {
                budget: walk.budget,
                node_index: start_node_index + 1,
            };
            let mut iter_state = std::borrow::Cow::Borrowed(&*local);
            for body_child in &node.children {
                if body_child.kind == BuiltinKind::Empty {
                    count_scene_subtree(body_child, &mut iter_walk.node_index, depth + 1)?;
                } else {
                    self.scene_expand_sequence_step(
                        body_child,
                        &mut iter_state,
                        SceneSequenceEnv {
                            prefix: &prefix,
                            scope,
                            depth: depth + 1,
                        },
                        &mut iter_walk,
                        &mut out,
                    )?;
                }
            }
            walk.budget = iter_walk.budget;
        }
        Ok(out)
    }

    /// The `Empty` block of a `For` over no items; nothing when it has none.
    pub(super) fn expand_for_empty(
        &self,
        at: &SceneAt<'_>,
        state: &Properties,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let SceneAt {
            node,
            start: start_node_index,
            prefix,
            scope,
            depth,
            ..
        } = *at;
        let mut child_idx = start_node_index + 1;
        for child in &node.children {
            if child.kind == BuiltinKind::Empty {
                let mut empty_walk = SceneWalk {
                    budget: walk.budget,
                    node_index: child_idx + 1,
                };
                let expanded = self.scene_expand_sequence(
                    &child.children,
                    state,
                    prefix,
                    scope,
                    depth + 1,
                    &mut empty_walk,
                )?;
                walk.budget = empty_walk.budget;
                return Ok(expanded);
            }
            count_scene_subtree(child, &mut child_idx, depth + 1)?;
        }
        Ok(Vec::new())
    }

    /// Adds this iteration to the binding frames in `local`, so a write through a binding inside
    /// the loop can find the same live item again.
    pub(super) fn retain_binding_frame(
        &self,
        state: &Properties,
        local: &mut Properties,
        item: &str,
        col_key: &str,
        binding_metadata: &[String; 3],
        captured_locals: &mut Option<Properties>,
    ) {
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
            ("__item".into(), Value::Text(item.to_string())),
            ("__parents".into(), Value::Object(active.clone())),
            ("__locals".into(), Value::Object(locals)),
        ]);
        if let Some(Value::Object(lexicals)) = state.get("__binding_lexicals") {
            frame.insert("__lexicals".into(), Value::Object(lexicals.clone()));
            if let Some(steps) = state.get("__binding_let_steps") {
                frame.insert("__let_steps".into(), steps.clone());
            }
        }
        if let Some(value) = local.get(col_key) {
            frame.insert(col_key.to_string(), value.clone());
        }
        for name in binding_metadata {
            if let Some(value) = local.get(name) {
                frame.insert(name.clone(), value.clone());
            }
        }
        active.insert(item.to_string(), number(frames.len() as f64));
        frames.push(Value::Object(frame));
        local.set("__binding_frames".into(), Value::List(frames));
        local.set("__binding_active".into(), Value::Object(active));
    }

    /// A `Let` used as a node: its children with the binding in scope.
    pub(super) fn expand_let(
        &self,
        at: &SceneAt<'_>,
        state: &Properties,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let SceneAt {
            node,
            start: start_node_index,
            prefix,
            scope,
            depth,
            ..
        } = *at;
        let mut out = Vec::new();
        let mut child_walk = SceneWalk {
            budget: walk.budget,
            node_index: start_node_index + 1,
        };
        if let Some((name, val, val_expr)) = control_flow::evaluate_let_binding(node, scope, state)?
        {
            let mut next_state = state.scoped_clone();
            insert_scene_lexical(&mut next_state, name, val, val_expr, scope);
            out.extend(self.scene_expand_sequence(
                &node.children,
                &next_state,
                prefix,
                scope,
                depth + 1,
                &mut child_walk,
            )?);
        } else if !node.children.is_empty() {
            out.extend(self.scene_expand_sequence(
                &node.children,
                state,
                prefix,
                scope,
                depth + 1,
                &mut child_walk,
            )?);
        }
        walk.budget = child_walk.budget;
        Ok(out)
    }

    /// A `Match`: the children of the first branch that matches the subject.
    pub(super) fn expand_match(
        &self,
        at: &SceneAt<'_>,
        state: &Properties,
        walk: &mut SceneWalk,
    ) -> Result<Vec<Node>, String> {
        let SceneAt {
            node,
            start: start_node_index,
            prefix,
            scope,
            depth,
            ..
        } = *at;
        let subject = node
            .props
            .get("subject")
            .ok_or("Match requires a subject")?
            .evaluate(scope, state)?;
        let mut out = Vec::new();
        let mut child_idx = start_node_index + 1;
        for branch in &node.children {
            let branch_start = child_idx;
            count_scene_subtree(branch, &mut child_idx, depth + 1)?;
            if control_flow::branch_matches(branch, &subject, scope, state)? {
                let mut branch_walk = SceneWalk {
                    budget: walk.budget,
                    node_index: branch_start + 1,
                };
                out.extend(self.scene_expand_sequence(
                    &branch.children,
                    state,
                    prefix,
                    scope,
                    depth + 1,
                    &mut branch_walk,
                )?);
                walk.budget = branch_walk.budget;
                break;
            }
        }
        Ok(out)
    }
}
