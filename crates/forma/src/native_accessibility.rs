//! Native AccessKit tree translation. Window adapter integration owns delivery.
use crate::runtime::{A11yRole, A11yTree};
use accesskit::{Action, Node, NodeId, Rect, Role, TreeId, TreeInfo, TreeUpdate};

fn id(value: usize) -> NodeId {
    NodeId(value as u64)
}
struct TextRun<'a> {
    id: NodeId,
    value: &'a str,
    lengths: Vec<u8>,
    offsets: Vec<u32>,
}
fn text_runs(parent: usize, value: &str) -> Option<Vec<TextRun<'_>>> {
    if parent > 0x7fff_ffff {
        return None;
    }
    let mut result = Vec::new();
    let mut offset = 0;
    let mut lines: Vec<&str> = value.split_inclusive('\n').collect();
    if value.is_empty() || value.ends_with('\n') {
        lines.push("");
    }
    for (line, value) in lines.into_iter().enumerate() {
        let line = u32::try_from(line).ok()?;
        let id = NodeId((1u64 << 63) | ((parent as u64) << 32) | u64::from(line));
        let mut lengths = Vec::new();
        let mut offsets = vec![offset];
        for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(value, true) {
            lengths.push(u8::try_from(grapheme.len()).ok()?);
            offset += grapheme.encode_utf16().count() as u32;
            offsets.push(offset);
        }
        result.push(TextRun {
            id,
            value,
            lengths,
            offsets,
        });
    }
    Some(result)
}
fn text_position(runs: &[TextRun<'_>], offset: u32) -> accesskit::TextPosition {
    let run = runs
        .iter()
        .find(|run| offset < *run.offsets.last().unwrap())
        .unwrap_or_else(|| runs.last().unwrap());
    let index = run
        .offsets
        .partition_point(|at| *at < offset)
        .min(run.lengths.len());
    accesskit::TextPosition {
        node: run.id,
        character_index: index,
    }
}

/// Translate a complete semantic snapshot. Coordinates remain in Runtime units;
/// the window host must supply its logical-to-window transform when delivering.
/// Text input children expose grapheme units through per-line AccessKit text runs.
/// A grapheme exceeding the u8 byte-length format disables text-run selection.
pub fn tree_update(tree: &A11yTree) -> TreeUpdate {
    let mut text_nodes = Vec::new();
    let mut nodes: Vec<_> = tree
        .nodes
        .iter()
        .map(|source| {
            let role = match source.role {
                A11yRole::Window => Role::Window,
                A11yRole::ScrollView => Role::ScrollView,
                A11yRole::Dialog => Role::Dialog,
                A11yRole::Button => Role::Button,
                A11yRole::TextInput => {
                    if source.multiline {
                        Role::MultilineTextInput
                    } else {
                        Role::TextInput
                    }
                }
                A11yRole::Slider => Role::Slider,
                A11yRole::StaticText => Role::Label,
            };
            let mut node = Node::new(role);
            node.set_children(source.children.iter().copied().map(id).collect::<Vec<_>>());
            if source.role == A11yRole::StaticText {
                node.set_value(source.label.clone());
            } else {
                node.set_label(source.label.clone());
            }
            if let Some(value) = &source.value {
                node.set_value(value.clone());
            }
            if let Some(value) = source.numeric_value {
                node.set_numeric_value(f64::from(value));
            }
            if source.disabled || source.modal_blocked {
                node.set_disabled();
            }
            if source.read_only {
                node.set_read_only();
            }
            let [x, y, w, h] = source.bounds.map(f64::from);
            node.set_bounds(Rect::new(x, y, x + w, y + h));
            if source.actions.contains(&"focus") {
                node.add_action(Action::Focus);
            }
            if source.actions.contains(&"click") {
                node.add_action(Action::Click);
            }
            if source.role == A11yRole::TextInput && source.actions.contains(&"set_value") {
                node.add_action(Action::SetValue);
            }
            if source.role == A11yRole::Slider {
                node.set_min_numeric_value(0.);
                node.set_max_numeric_value(1.);
                node.set_numeric_value_step(0.01);
                node.set_numeric_value_jump(0.1);
                if source.actions.contains(&"increment") {
                    node.add_action(Action::Increment);
                }
                if source.actions.contains(&"decrement") {
                    node.add_action(Action::Decrement);
                }
                if source.actions.contains(&"set_value") {
                    node.add_action(Action::SetValue);
                }
            }
            if source.role == A11yRole::ScrollView
                && source.actions.contains(&"scroll")
                && !source.modal_blocked
            {
                for action in [
                    Action::ScrollUp,
                    Action::ScrollDown,
                    Action::ScrollLeft,
                    Action::ScrollRight,
                ] {
                    node.add_action(action);
                }
            }
            if source.role == A11yRole::Dialog {
                if !source.disabled {
                    node.set_live(accesskit::Live::Polite);
                }
                if source.actions.contains(&"dismiss") && !source.modal_blocked {
                    node.add_action(Action::Collapse);
                }
            }
            if source.role == A11yRole::TextInput {
                if let Some(runs) = source
                    .value
                    .as_deref()
                    .and_then(|value| text_runs(source.id, value))
                {
                    node.set_children(runs.iter().map(|run| run.id).collect::<Vec<_>>());
                    if let Some([anchor, focus]) = source.selection_utf16 {
                        node.set_text_selection(accesskit::TextSelection {
                            anchor: text_position(&runs, anchor),
                            focus: text_position(&runs, focus),
                        });
                    }
                    if source.actions.contains(&"set_text_selection") {
                        node.add_action(Action::SetTextSelection);
                    }
                    for run in runs {
                        let mut text = Node::new(Role::TextRun);
                        text.set_value(run.value);
                        text.set_character_lengths(run.lengths);
                        text_nodes.push((run.id, text));
                    }
                }
            }
            (id(source.id), node)
        })
        .collect();
    nodes.extend(text_nodes);
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(id(tree.root_id))),
        tree_id: TreeId::ROOT,
        focus: id(tree.focused_id.unwrap_or(tree.root_id)),
    }
}

/// Route basic native actions through the same Runtime admission as keyboard.
/// Range actions need additional metadata before admission.
pub fn apply_action(runtime: &mut crate::Runtime, target: NodeId, action: Action) -> bool {
    apply_action_data(runtime, target, action, None)
}

pub fn apply_action_data(
    runtime: &mut crate::Runtime,
    target: NodeId,
    action: Action,
    data: Option<&accesskit::ActionData>,
) -> bool {
    let Ok(node_id) = usize::try_from(target.0) else {
        return false;
    };
    if action == Action::SetTextSelection {
        let Some(accesskit::ActionData::SetTextSelection(selection)) = data else {
            return false;
        };
        let tree = runtime.accessibility_tree();
        let Some(node) = tree
            .nodes
            .iter()
            .find(|node| node.id == node_id && node.actions.contains(&"set_text_selection"))
        else {
            return false;
        };
        let Some(runs) = node
            .value
            .as_deref()
            .and_then(|value| text_runs(node_id, value))
        else {
            return false;
        };
        let offset = |position: accesskit::TextPosition| {
            runs.iter()
                .find(|run| run.id == position.node)
                .and_then(|run| run.offsets.get(position.character_index))
                .copied()
        };
        let (Some(anchor), Some(focus)) = (offset(selection.anchor), offset(selection.focus))
        else {
            return false;
        };
        return runtime.accessibility_set_text_selection(node_id, anchor, focus);
    }
    if action == Action::SetValue {
        let tree = runtime.accessibility_tree();
        let Some(node) = tree
            .nodes
            .iter()
            .find(|node| node.id == node_id && node.actions.contains(&"set_value"))
        else {
            return false;
        };
        return match (node.role, data) {
            (A11yRole::TextInput, Some(accesskit::ActionData::Value(value))) => {
                runtime.accessibility_set_text_value(node_id, value)
            }
            (A11yRole::Slider, Some(accesskit::ActionData::NumericValue(value))) => {
                runtime.accessibility_set_range_value(node_id, *value)
            }
            (A11yRole::Slider, Some(accesskit::ActionData::Value(value))) => {
                let trimmed = value.trim().trim_end_matches('%').trim();
                let Ok(parsed) = trimmed.parse::<f64>() else {
                    return false;
                };
                runtime.accessibility_set_range_value(node_id, parsed)
            }
            _ => false,
        };
    }
    if matches!(action, Action::Increment | Action::Decrement) {
        let name = if action == Action::Increment {
            "increment"
        } else {
            "decrement"
        };
        let tree = runtime.accessibility_tree();
        if !tree.nodes.iter().any(|node| {
            node.id == node_id && node.role == A11yRole::Slider && node.actions.contains(&name)
        }) {
            return false;
        }
        return runtime.accessibility_step_range(
            node_id,
            if action == Action::Increment {
                0.05
            } else {
                -0.05
            },
        );
    }
    if matches!(
        action,
        Action::ScrollUp | Action::ScrollDown | Action::ScrollLeft | Action::ScrollRight
    ) {
        let tree = runtime.accessibility_tree();
        if !tree.nodes.iter().any(|node| {
            node.id == node_id
                && node.role == A11yRole::ScrollView
                && node.actions.contains(&"scroll")
                && !node.modal_blocked
        }) {
            return false;
        }
        let (dx, dy) = match action {
            Action::ScrollUp => (0., -48.),
            Action::ScrollDown => (0., 48.),
            Action::ScrollLeft => (-48., 0.),
            Action::ScrollRight => (48., 0.),
            _ => unreachable!(),
        };
        return runtime.accessibility_scroll(node_id, dx, dy);
    }
    if action == Action::Collapse {
        let tree = runtime.accessibility_tree();
        if !tree.nodes.iter().any(|node| {
            node.id == node_id
                && node.role == A11yRole::Dialog
                && node.actions.contains(&"dismiss")
                && !node.modal_blocked
        }) {
            return false;
        }
        return runtime.accessibility_dismiss_modal(node_id);
    }
    let name = match action {
        Action::Focus => "focus",
        Action::Click => "click",
        _ => return false,
    };
    if !runtime
        .accessibility_tree()
        .nodes
        .iter()
        .any(|node| node.id == node_id && node.actions.contains(&name))
    {
        return false;
    }

    let Some(index) = runtime.nodes().get(node_id).and_then(|node| node.control) else {
        return false;
    };
    match action {
        Action::Focus => runtime.focus_control(index),
        Action::Click => {
            if !runtime.focus_control(index) {
                return false;
            }
            runtime.activate_control(index);
            true
        }
        _ => false,
    }
}

/// Window-lifetime IDs prevent queued OS actions from using recycled scene IDs.
#[derive(Default)]
pub struct PublishedTree {
    scene: Option<u64>,
    next: u64,
    last_snapshot: Option<A11yTree>,
    last_scale: f64,
    ids: std::collections::BTreeMap<NodeId, NodeId>,
    reverse: std::collections::BTreeMap<NodeId, NodeId>,
    text_values: std::collections::BTreeMap<usize, String>,
}
impl PublishedTree {
    pub fn invalidate(&mut self) {
        self.last_snapshot = None;
    }
    pub fn update_if_changed(
        &mut self,
        runtime: &crate::Runtime,
        scale: f64,
    ) -> Result<Option<TreeUpdate>, String> {
        if !scale.is_finite() || scale <= 0. {
            return Err("Invalid accessibility scale".into());
        }
        let snapshot = runtime.accessibility_tree();
        if self.scene == Some(runtime.native_scene_identity())
            && self.last_scale == scale
            && self.last_snapshot.as_ref() == Some(&snapshot)
        {
            return Ok(None);
        }
        let mut update = self.update_snapshot(runtime.native_scene_identity(), &snapshot)?;
        let root = update.tree.as_ref().unwrap().root;
        update
            .nodes
            .iter_mut()
            .find(|(id, _)| *id == root)
            .unwrap()
            .1
            .set_transform(accesskit::Affine::scale(scale));
        self.last_snapshot = Some(snapshot);
        self.last_scale = scale;
        Ok(Some(update))
    }
    pub fn update(&mut self, runtime: &crate::Runtime) -> Result<TreeUpdate, String> {
        self.invalidate();
        self.update_snapshot(
            runtime.native_scene_identity(),
            &runtime.accessibility_tree(),
        )
    }
    fn update_snapshot(&mut self, scene: u64, snapshot: &A11yTree) -> Result<TreeUpdate, String> {
        if self.scene != Some(scene) {
            self.ids.clear();
            self.reverse.clear();
            self.text_values.clear();
        }
        // Text-position IDs describe one value, not merely one line number.
        for node in &snapshot.nodes {
            if node.role == A11yRole::TextInput
                && self.text_values.get(&node.id) != node.value.as_ref()
            {
                self.ids.retain(|key, _| {
                    key.0 >> 63 == 0 || ((key.0 & !(1u64 << 63)) >> 32) != node.id as u64
                });
            }
        }
        self.text_values = snapshot
            .nodes
            .iter()
            .filter(|node| node.role == A11yRole::TextInput)
            .filter_map(|node| node.value.clone().map(|value| (node.id, value)))
            .collect();
        let mut update = tree_update(snapshot);
        let live: std::collections::BTreeSet<_> =
            update.nodes.iter().map(|(key, _)| *key).collect();
        self.ids.retain(|key, _| live.contains(key));
        for (key, _) in &update.nodes {
            if !self.ids.contains_key(key) {
                let native = if key.0 == snapshot.root_id as u64 {
                    NodeId(0)
                } else {
                    self.next = self
                        .next
                        .checked_add(1)
                        .ok_or_else(|| "AccessKit node IDs exhausted".to_owned())?;
                    NodeId(self.next)
                };
                self.ids.insert(*key, native);
            }
        }
        self.reverse = self
            .ids
            .iter()
            .map(|(local, native)| (*native, *local))
            .collect();
        for (key, node) in &mut update.nodes {
            *key = self.ids[key];
            node.set_children(
                node.children()
                    .iter()
                    .map(|key| self.ids[key])
                    .collect::<Vec<_>>(),
            );
            if let Some(selection) = node.text_selection().copied() {
                node.set_text_selection(accesskit::TextSelection {
                    anchor: accesskit::TextPosition {
                        node: self.ids[&selection.anchor.node],
                        ..selection.anchor
                    },
                    focus: accesskit::TextPosition {
                        node: self.ids[&selection.focus.node],
                        ..selection.focus
                    },
                });
            }
        }
        update.focus = self.ids[&update.focus];
        if let Some(tree) = &mut update.tree {
            tree.root = self.ids[&tree.root];
        }
        self.scene = Some(scene);
        Ok(update)
    }
    pub fn apply(&self, runtime: &mut crate::Runtime, request: &accesskit::ActionRequest) -> bool {
        if self.scene != Some(runtime.native_scene_identity())
            || request.target_tree != TreeId::ROOT
        {
            return false;
        }
        let Some(target) = self.reverse.get(&request.target_node).copied() else {
            return false;
        };
        let mut data = request.data.clone();
        if let Some(accesskit::ActionData::SetTextSelection(selection)) = &mut data {
            let snapshot = runtime.accessibility_tree();
            let Some(node) = snapshot
                .nodes
                .iter()
                .find(|node| node.id as u64 == target.0)
            else {
                return false;
            };
            if self.text_values.get(&node.id) != node.value.as_ref() {
                return false;
            }
            let (Some(anchor), Some(focus)) = (
                self.reverse.get(&selection.anchor.node),
                self.reverse.get(&selection.focus.node),
            ) else {
                return false;
            };
            selection.anchor.node = *anchor;
            selection.focus.node = *focus;
        }
        apply_action_data(runtime, target, request.action, data.as_ref())
    }
}

#[cfg(test)]
mod tests;
