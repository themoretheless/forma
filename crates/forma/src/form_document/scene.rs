//! Scene expansion: the root document with per-node state, node ids, keys and binding metadata,
//! and positioning of the resulting controls.

use super::*;

mod branches;
mod placement;

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

struct SceneSequenceEnv<'a> {
    prefix: &'a str,
    scope: &'a Properties,
    depth: usize,
}

/// The node being expanded and where it sits in the walk.
#[derive(Clone, Copy)]
struct SceneAt<'a> {
    node: &'a Node,
    /// The node's stable id: its `__node` property, or its preorder index.
    id: usize,
    /// The node's preorder index; its children start right after it.
    start: usize,
    prefix: &'a str,
    scope: &'a Properties,
    depth: usize,
}

/// Properties a positioned control carries from the layout to the scene mapping: its clip
/// rectangle and the scroll areas around it. Written by placement, read once by the emitter.
pub(in crate::form_document) const CLIP_PROPERTY: &str = "__clip";
pub(in crate::form_document) const SCROLL_PROPERTY: &str = "__scroll";
pub(in crate::form_document) const ORIGIN_PROPERTY: &str = "__origin";
/// The built-in component a container with a `clicked` handler lays down as its pressable area.
pub(in crate::form_document) const CONTAINER_AREA: &str = "ContainerArea";

pub(super) struct ScenePositionTarget<'a> {
    pub(super) instances: &'a mut Vec<(Node, Option<Node>)>,
    pub(super) logical_end: &'a mut [f64; 2],
    pub(super) scroll_content: bool,
    /// What the clipping containers around the node being positioned leave visible.
    pub(super) clip: Option<[f64; 4]>,
    /// The scroll areas around the node being positioned, outermost first: number and viewport.
    pub(super) scroll_chain: Vec<(usize, [f64; 4])>,
    /// The `clicked` handler of the nearest container around the node, with its event arguments:
    /// a control without its own handler answers a click with this one.
    pub(super) bubble: Option<(String, Option<Value>)>,
    /// The content size of every scroll area below the root, by number.
    pub(super) scroll_content_sizes: &'a mut Vec<[f64; 2]>,
}

impl Document {
    pub(in crate::form_document) fn scene_properties(
        &self,
        node: &Node,
        scope: &Properties,
        state: &Properties,
    ) -> Result<Properties, String> {
        let definition = self.definitions.get(node.kind.as_str());
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
        let mut result = BTreeMap::new();
        let values = property_values(node, &scope, state)?;
        let mut stack = Vec::new();
        let mut merged_defaults: Option<Properties> = None;
        for (key, value) in &values {
            if key == "__matches" || key == "__forward" || key == "__scope" || key == "__styles" {
                continue;
            }
            stack.clear();
            match eval_typed(value, &scope, state, &mut stack) {
                Ok(value) => {
                    result.insert(key.clone(), value);
                }
                Err(error) if error.is_missing_state() => {
                    if let Some(default) = default_value(key) {
                        let defaults = merged_defaults.get_or_insert_with(|| {
                            let mut d = self.fallback.clone();
                            if let Some(definition) = definition {
                                d.extend(definition.defaults.clone());
                            }
                            d
                        });
                        result.insert(
                            key.clone(),
                            eval(default, defaults, state, &mut Vec::new())?,
                        );
                    }
                }
                Err(error) => return Err(error.into_message()),
            }
        }
        if result.get("__range_normalized") == Some(&Value::Bool(true)) {
            if let Some(Value::Number(value)) = result.get("value") {
                let bounds = RangeBounds::of(|name| result.get(name));
                let normalized = Value::Expr(format!("{}%", bounds.fraction(*value) * 100.));
                result.insert("value".into(), normalized);
            }
        }
        Ok(result.into())
    }

    fn scene_expand_sequence_step(
        &self,
        child: &Node,
        state: &mut std::borrow::Cow<'_, Properties>,
        env: SceneSequenceEnv<'_>,
        walk: &mut SceneWalk,
        out: &mut Vec<Node>,
    ) -> Result<(), String> {
        if child.kind == BuiltinKind::Let {
            let mut child_node_index = walk.node_index;
            count_scene_subtree(child, &mut child_node_index, env.depth)?;
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
                control_flow::evaluate_let_binding(child, env.scope, state)?
            {
                if child.children.is_empty() {
                    insert_scene_lexical(scope_mut(state), name, value, expression, env.scope);
                } else {
                    let mut block_state = state.as_ref().scoped_clone();
                    insert_scene_lexical(&mut block_state, name, value, expression, env.scope);
                    out.extend(self.scene_expand_sequence(
                        &child.children,
                        &block_state,
                        env.prefix,
                        env.scope,
                        env.depth + 1,
                        &mut inner_walk,
                    )?);
                }
            } else if !child.children.is_empty() {
                out.extend(self.scene_expand_sequence(
                    &child.children,
                    state,
                    env.prefix,
                    env.scope,
                    env.depth + 1,
                    &mut inner_walk,
                )?);
            }
            walk.budget = inner_walk.budget;
        } else {
            out.extend(self.scene_expand(child, state, env.prefix, env.scope, env.depth, walk)?);
        }
        Ok(())
    }

    pub(in crate::form_document) fn scene_expand_sequence<'a>(
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
                child,
                &mut state,
                SceneSequenceEnv {
                    prefix,
                    scope,
                    depth,
                },
                walk,
                &mut out,
            )?;
        }
        Ok(out)
    }

    pub(in crate::form_document) fn scene_expand(
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
        let at = SceneAt {
            node,
            id,
            start: start_node_index,
            prefix,
            scope: &scope,
            depth,
        };
        match node.kind.builtin() {
            Some(BuiltinKind::If) => self.expand_if(&at, &state, walk),
            Some(BuiltinKind::For) => self.expand_for(&at, &state, walk),
            Some(BuiltinKind::Else | BuiltinKind::Empty) => Ok(Vec::new()),
            Some(BuiltinKind::Let) => self.expand_let(&at, &state, walk),
            Some(BuiltinKind::Match) => self.expand_match(&at, &state, walk),
            _ => self.expand_element(&at, state, walk),
        }
    }

    /// A layout or control node with its resolved properties, key and children.
    fn expand_element(
        &self,
        at: &SceneAt<'_>,
        state: std::borrow::Cow<'_, Properties>,
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
        let mut props = self.scene_properties(node, scope, &state)?;
        props.entry("__node".into()).or_insert(number(id as f64));
        let mut output = Node {
            kind: node.kind.clone(),
            props,
            children: Vec::new(),
            action: node.action.clone(),
        };
        if self.definitions.contains_key(node.kind.as_str()) {
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
        if self.definitions.contains_key(node.kind.as_str()) {
            // Supplied content resolves props in the receiving component's scope.
            let mut child_idx = start_node_index + 1;
            let mut annotated_children = Vec::with_capacity(node.children.len());
            for child in &node.children {
                annotated_children.push(annotated_scene_source(child, &mut child_idx, depth + 1)?);
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
                scope,
                depth + 1,
                &mut child_walk,
            )?;
            walk.budget = child_walk.budget;
        }
        // Children and resolved properties no longer borrow this scope.
        // Move a local owned state into its public control snapshot;
        // inherited shared state is cloned only when ownership is needed.
        // Layout-only nodes do not consume snapshots.
        if !node.kind.is_layout() {
            output
                .props
                .insert("__state".into(), Value::Object(state.into_owned()));
        }
        Ok(vec![output])
    }
}

pub(in crate::form_document) fn assign_scene_origins(root: &mut Node) {
    fn parse_leaf_explicit(leaf: &str) -> Option<&str> {
        let tail = leaf.strip_prefix('@')?.split_once(':')?.1;
        let (len_str, explicit) = tail.split_once(':')?;
        let len = len_str.parse::<usize>().ok()?;
        if len > 0 && explicit.len() == len {
            Some(explicit)
        } else {
            None
        }
    }

    fn walk(
        node: &mut Node,
        path: &str,
        sibling_counts: &mut std::collections::BTreeMap<String, usize>,
    ) {
        if node.kind.is_layout() {
            let count = sibling_counts
                .entry(node.kind.as_str().to_owned())
                .or_insert(0);
            let ordinal = *count;
            *count += 1;
            let next_path = if path.is_empty() {
                format!("{}#{ordinal}", node.kind.as_str())
            } else {
                format!("{path}/{}#{ordinal}", node.kind.as_str())
            };
            let mut child_counts = std::collections::BTreeMap::new();
            for child in &mut node.children {
                walk(child, &next_path, &mut child_counts);
            }
            return;
        }
        let key = node.props.get("key").and_then(|v| match v {
            Value::Text(s) => Some(s.as_str()),
            _ => None,
        });
        let origin = match key.filter(|k| !k.is_empty()) {
            Some(k) if !k.starts_with('@') => format!("key:{k}"),
            Some(k) => {
                if let Some((loop_prefix, leaf_part)) = k.rsplit_once('/') {
                    let clean_prefix: String = loop_prefix
                        .split('/')
                        .filter_map(|seg| {
                            seg.strip_prefix('@')
                                .and_then(|s| s.split_once(':'))
                                .map(|(_, val)| format!("For[{val}]"))
                        })
                        .collect::<Vec<_>>()
                        .join("/");
                    if let Some(exp) = parse_leaf_explicit(leaf_part) {
                        format!("{path}/{clean_prefix}/{}[key={exp}]", node.kind.as_str())
                    } else {
                        let bucket = format!("{clean_prefix}/{}", node.kind.as_str());
                        let count = sibling_counts.entry(bucket).or_insert(0);
                        let ordinal = *count;
                        *count += 1;
                        format!("{path}/{clean_prefix}/{}#{ordinal}", node.kind.as_str())
                    }
                } else {
                    let count = sibling_counts
                        .entry(node.kind.as_str().to_owned())
                        .or_insert(0);
                    let ordinal = *count;
                    *count += 1;
                    format!("{path}/{}#{ordinal}", node.kind.as_str())
                }
            }
            None => {
                let count = sibling_counts
                    .entry(node.kind.as_str().to_owned())
                    .or_insert(0);
                let ordinal = *count;
                *count += 1;
                if path.is_empty() {
                    format!("{}#{ordinal}", node.kind.as_str())
                } else {
                    format!("{path}/{}#{ordinal}", node.kind.as_str())
                }
            }
        };
        node.props
            .insert(ORIGIN_PROPERTY.into(), Value::Text(origin));
    }

    let mut root_counts = std::collections::BTreeMap::new();
    walk(root, "", &mut root_counts);
}
