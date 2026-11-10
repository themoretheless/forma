//! Semantic accessibility tree projection (`A11yRole`, `A11yNode`, `A11yTree`)
//! and semantic action dispatch helpers for [`Runtime`].

use super::{NodeKind, Runtime};

/// AccessKit-compatible semantic role for a node in the accessibility tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum A11yRole {
    Window,
    ScrollView,
    Dialog,
    Button,
    TextInput,
    Slider,
    StaticText,
}

impl A11yRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "Window",
            Self::ScrollView => "ScrollView",
            Self::Dialog => "Dialog",
            Self::Button => "Button",
            Self::TextInput => "TextInput",
            Self::Slider => "Slider",
            Self::StaticText => "StaticText",
        }
    }
}

/// AccessKit-compatible semantic accessibility node exported by [`Runtime::accessibility_tree`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A11yNode {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub role: A11yRole,
    pub key: String,
    pub label: String,
    pub value: Option<String>,
    #[serde(skip)]
    pub numeric_value: Option<f32>,
    #[serde(serialize_with = "serialize_a11y_bounds")]
    pub bounds: [f32; 4],
    pub focused: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub multiline: bool,
    /// Anchor/caret UTF-16 offsets in this node's original value (CRLF retained).
    pub selection_utf16: Option<[u32; 2]>,
    pub modal_blocked: bool,
    pub actions: Vec<&'static str>,
}

/// Snapshot of the scene's semantic accessibility tree and live region state.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A11yTree {
    pub root_id: usize,
    pub focused_id: Option<usize>,
    pub active_modal_id: Option<usize>,
    pub nodes: Vec<A11yNode>,
}

// Preserve the existing JSON numeric values, formerly materialized through Value.
fn serialize_a11y_bounds<S: serde::Serializer>(
    bounds: &[f32; 4],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&bounds.map(f64::from), serializer)
}

impl Runtime {
    /// Builds an AccessKit-compatible semantic accessibility tree for native and Web assistive technologies.
    pub fn accessibility_tree(&self) -> A11yTree {
        let mut a11y_nodes = Vec::with_capacity(self.nodes.len());
        let mut focused_node_id = None;
        let active_modal = self.active_modal_index();
        let mut active_modal_node_id = None;

        for node in &self.nodes {
            match node.kind {
                NodeKind::Frame => {
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: node.children.clone(),
                        role: A11yRole::Window,
                        key: self.scene.name.clone(),
                        label: self.scene.name.clone(),
                        value: None,
                        numeric_value: None,
                        bounds: [0.0, 0.0, self.render_width(), self.render_height()],
                        focused: false,
                        disabled: false,
                        multiline: false,
                        selection_utf16: None,
                        read_only: false,
                        modal_blocked: false,
                        actions: Vec::new(),
                    });
                }
                NodeKind::Scroll => {
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: node.children.clone(),
                        role: A11yRole::ScrollView,
                        key: "scroll".into(),
                        label: "Scrollable Region".into(),
                        value: Some(format!("{:.0},{:.0}", self.scroll[0], self.scroll[1])),
                        numeric_value: Some(self.scroll[1]),
                        bounds: self.viewport_rect(),
                        focused: false,
                        disabled: false,
                        multiline: false,
                        selection_utf16: None,
                        read_only: false,
                        modal_blocked: active_modal.is_some(),
                        actions: vec!["scroll"],
                    });
                }
                NodeKind::Modal => {
                    let modal_idx = self.modal_index_for_node(node);
                    let open = self.modal_open.get(modal_idx).copied().unwrap_or(false);
                    if active_modal == Some(modal_idx) {
                        active_modal_node_id = Some(node.id);
                    }
                    let m_spec = &self.scene.modals[modal_idx];
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: if open {
                            node.children.clone()
                        } else {
                            Vec::new()
                        },
                        role: A11yRole::Dialog,
                        key: m_spec.key.clone(),
                        label: m_spec.key.clone(),
                        value: Some(if open { "open" } else { "closed" }.into()),
                        numeric_value: None,
                        bounds: [m_spec.x, m_spec.y, m_spec.width, m_spec.height],
                        focused: false,
                        disabled: !open,
                        multiline: false,
                        selection_utf16: None,
                        read_only: false,
                        modal_blocked: active_modal.is_some() && active_modal != Some(modal_idx),
                        actions: if open && m_spec.dismiss {
                            vec!["dismiss"]
                        } else {
                            Vec::new()
                        },
                    });
                }
                NodeKind::Control => {
                    let idx = node.control.unwrap_or(0);
                    let b = self.control_bounds(idx);
                    let bounds = [b[0], b[1], b[2], b[3]];
                    let disabled = self.control_disabled(idx);
                    let blocked = self.control_blocked(idx);
                    let focused = self.focused == Some(idx);
                    if focused {
                        focused_node_id = Some(node.id);
                    }
                    let is_ranged = self
                        .controls
                        .get(idx)
                        .is_some_and(|c| c.template.range.is_some());
                    let role = if self.control_editable(idx) {
                        A11yRole::TextInput
                    } else if is_ranged {
                        A11yRole::Slider
                    } else if self.control_interactive(idx) {
                        A11yRole::Button
                    } else {
                        A11yRole::StaticText
                    };
                    let mut label = self.controls[idx].scene.button.text.clone();
                    if label.is_empty() {
                        for item in &self.controls[idx].template.content {
                            if let crate::template::Content::Text { text, .. } = item {
                                if !text.text.is_empty() {
                                    label = text.text.clone();
                                    break;
                                }
                            }
                        }
                    }
                    if self.control_editable(idx) {
                        label = self.control_input_label(idx);
                    }
                    let value = if self.control_editable(idx) {
                        Some(self.text_value(idx))
                    } else if is_ranged {
                        Some(format!("{:.0}%", self.range_value(idx) * 100.0))
                    } else {
                        None
                    };
                    let numeric_value = if is_ranged {
                        Some(self.range_value(idx))
                    } else {
                        None
                    };
                    let mut actions = Vec::new();
                    if !disabled && !blocked {
                        match role {
                            A11yRole::Button => actions.extend(["focus", "click"]),
                            A11yRole::TextInput => {
                                actions.extend(["focus", "set_text_selection"]);
                                if !self.control_readonly(idx) {
                                    actions.push("set_value");
                                }
                            }
                            A11yRole::Slider => {
                                actions.extend(["focus", "increment", "decrement", "set_value"])
                            }
                            _ => {}
                        }
                    }
                    a11y_nodes.push(A11yNode {
                        id: node.id,
                        parent: node.parent,
                        children: Vec::new(),
                        role,
                        key: self.controls[idx].key(),
                        label,
                        value,
                        numeric_value,
                        bounds,
                        focused,
                        disabled,
                        multiline: self.control_multiline(idx),
                        selection_utf16: self.editors[idx].as_ref().map(|editor| {
                            [editor.model.anchor(), editor.model.caret()]
                                .map(|at| editor.model.value()[..at].encode_utf16().count() as u32)
                        }),
                        read_only: self.control_readonly(idx),
                        modal_blocked: blocked,
                        actions,
                    });
                }
            }
        }

        A11yTree {
            root_id: 0,
            focused_id: focused_node_id,
            active_modal_id: active_modal_node_id,
            nodes: a11y_nodes,
        }
    }
}
