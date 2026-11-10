//! Component expansion into primitive visual trees and their flattening to template text.
//!
//! This is the lower level of document rendering: one component instance at a time, without
//! scene identity. The scene level in `scene.rs` calls into it per control.

use super::*;

impl Document {
    pub(super) fn expand_value(
        &self,
        value: &Value,
        props: &Properties,
        state: &Properties,
        parents: &[String],
        depth: usize,
    ) -> Result<Value, String> {
        let value = eval(value, props, state, &mut Vec::new())?;
        if let Value::Node(node) = value {
            Ok(Value::Node(Box::new(self.expand(
                &node,
                props,
                state,
                parents,
                depth + 1,
            )?)))
        } else {
            Ok(value)
        }
    }
    pub(super) fn expand_sequence<'a>(
        &self,
        nodes: impl IntoIterator<Item = &'a Node>,
        props: &Properties,
        state: &Properties,
        parents: &[String],
        depth: usize,
    ) -> Result<Vec<Node>, String> {
        if depth > 64 {
            return Err("Visual composition depth exceeded".into());
        }
        let mut result = Vec::new();
        // Ordinary visual expansion only reads inherited state; lexical writes own a copy.
        let mut current_state = std::borrow::Cow::Borrowed(state);
        for node in nodes {
            match node.kind.as_str() {
                "If" => {
                    if control_flow::evaluate_if_condition(node, props, &current_state)? {
                        result.extend(self.expand_sequence(
                            control_flow::if_then_children(node),
                            props,
                            &current_state,
                            parents,
                            depth + 1,
                        )?);
                    } else if let Some(else_node) = control_flow::if_else_node(node) {
                        result.extend(self.expand_sequence(
                            &else_node.children,
                            props,
                            &current_state,
                            parents,
                            depth + 1,
                        )?);
                    }
                }
                "For" => {
                    let (items, item) =
                        control_flow::evaluate_for_header(node, props, &current_state)?;
                    let idx_key = format!("__index.{item}");
                    let mut keys = std::collections::BTreeSet::new();
                    if items.is_empty() {
                        if let Some(empty) = control_flow::for_empty_node(node) {
                            result.extend(self.expand_sequence(
                                &empty.children,
                                props,
                                &current_state,
                                parents,
                                depth + 1,
                            )?);
                        }
                    }
                    for (index, value) in items.into_iter().enumerate() {
                        let mut state = current_state.as_ref().scoped_clone();
                        state.insert(item.clone(), value);
                        state.insert(idx_key.clone(), number(index as f64));
                        let key = key_part(
                            &node
                                .props
                                .get("key")
                                .ok_or("For requires a key")?
                                .evaluate(props, &state)?,
                        )?;
                        if !keys.insert(key) {
                            return Err("Duplicate collection key".into());
                        }
                        result.extend(self.expand_sequence(
                            control_flow::for_body_children(node),
                            props,
                            &state,
                            parents,
                            depth + 1,
                        )?);
                    }
                }
                "Else" | "Empty" => {}
                "Let" => {
                    if let Some((name, val, _)) =
                        control_flow::evaluate_let_binding(node, props, &current_state)?
                    {
                        if node.children.is_empty() {
                            scope_mut(&mut current_state).insert(name, val);
                        } else {
                            let mut scoped = current_state.as_ref().scoped_clone();
                            scoped.insert(name, val);
                            result.extend(self.expand_sequence(
                                &node.children,
                                props,
                                &scoped,
                                parents,
                                depth + 1,
                            )?);
                        }
                    }
                }
                "Match" => {
                    let subject = node
                        .props
                        .get("subject")
                        .ok_or("Match requires a subject")?
                        .evaluate(props, &current_state)?;
                    for branch in &node.children {
                        if control_flow::branch_matches(branch, &subject, props, &current_state)? {
                            result.extend(self.expand_sequence(
                                &branch.children,
                                props,
                                &current_state,
                                parents,
                                depth + 1,
                            )?);
                            break;
                        }
                    }
                }
                _ => result.push(self.expand(node, props, &current_state, parents, depth + 1)?),
            }
        }
        Ok(result)
    }
    pub(super) fn expand(
        &self,
        n: &Node,
        props: &Properties,
        state: &Properties,
        parents: &[String],
        depth: usize,
    ) -> Result<Node, String> {
        if depth > 64 {
            return Err("Visual composition depth exceeded".into());
        }
        if n.kind == "ContentPresenter" {
            let slot_name = n
                .props
                .get("key")
                .or_else(|| n.props.get("slot"))
                .and_then(|v| match v {
                    Value::Text(s) | Value::Expr(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("content");
            let value = n
                .props
                .get("content")
                .or_else(|| props.get(slot_name))
                .cloned()
                .unwrap_or_else(|| {
                    if n.children.len() == 1 {
                        Value::Node(Box::new(n.children[0].clone()))
                    } else {
                        Value::Node(Box::new(Node {
                            kind: "Frame".into(),
                            props: Properties::new(),
                            children: n.children.clone(),
                            action: None,
                        }))
                    }
                });
            return match self.expand_value(&value, props, state, parents, depth)? {
                Value::Node(n) => Ok(*n),
                _ => Err("ContentPresenter requires a node".into()),
            };
        }
        // Expanded properties/children replace the source tree; do not clone
        // the original subtree merely to overwrite both owned fields.
        let props_evaluated = properties(n, props, state)?;
        let expanded_props = if props_evaluated
            .values()
            .any(|v| matches!(v, Value::Node(_)))
        {
            props_evaluated
                .into_iter()
                .map(|(k, v)| match v {
                    Value::Node(child) => Ok((
                        k,
                        Value::Node(Box::new(self.expand(
                            &child,
                            props,
                            state,
                            parents,
                            depth + 1,
                        )?)),
                    )),
                    other => Ok((k, other)),
                })
                .collect::<Result<_, String>>()?
        } else {
            props_evaluated
        };
        let node = Node {
            kind: n.kind.clone(),
            props: expanded_props,
            children: self.expand_sequence(&n.children, props, state, parents, depth + 1)?,
            action: n.action.clone(),
        };
        // Scene definitions can wrap primitives such as Text and TextInput.
        // Inside their visual templates the same names retain primitive meaning.
        if !is_primitive(&n.kind) && self.definitions.contains_key(&n.kind) {
            return Ok(self.instantiate(&node, false, state, parents)?.0);
        }
        Ok(node)
    }
    pub(super) fn instantiate(
        &self,
        instance: &Node,
        top: bool,
        state: &Properties,
        parents: &[String],
    ) -> Result<(Node, Properties), String> {
        self.instantiate_scope(instance, top, Some(state), parents, &mut None)
    }
    pub(super) fn instantiate_scene(
        &self,
        instance: &mut Node,
    ) -> Result<(Node, Properties), String> {
        let mut snapshot = instance.props.remove("__state");
        let empty_state = Properties::new();
        let supplied_state = if snapshot.is_none() {
            Some(&empty_state)
        } else {
            None
        };
        let result = self.instantiate_scope(instance, true, supplied_state, &[], &mut snapshot);
        if let Some(snapshot) = snapshot {
            instance.props.insert("__state".into(), snapshot);
        }
        result
    }
    pub(super) fn instantiate_scope(
        &self,
        instance: &Node,
        top: bool,
        supplied_state: Option<&Properties>,
        parents: &[String],
        owned_snapshot: &mut Option<Value>,
    ) -> Result<(Node, Properties), String> {
        if parents.len() >= 32 || parents.contains(&instance.kind) {
            return Err(format!("Component cycle {}", instance.kind));
        }
        let definition = self
            .definitions
            .get(&instance.kind)
            .ok_or_else(|| format!("Missing component {}", instance.kind))?;
        for key in instance.props.keys() {
            if !definition.defaults.contains_key(key)
                && !group_declares(definition.defaults.get("__matches"), key)
                && !matches!(definition.defaults.get("__contracts"),Some(Value::Object(c)) if c.contains_key(key))
                && !key.starts_with("__")
                && !(top && self.fallback.contains_key(key))
                && ![
                    "key",
                    "x",
                    "y",
                    "cell",
                    "row",
                    "column",
                    "width",
                    "height",
                    "font.size",
                    "minWidth",
                    "maxWidth",
                    "minHeight",
                    "maxHeight",
                ]
                .contains(&key.as_str())
            {
                return Err(format!("Unknown property {}.{key}", instance.kind));
            }
        }
        let has_required_contracts = matches!(
            definition.defaults.get("__contracts"),
            Some(Value::Object(contracts))
                if contracts.values().any(|contract| {
                    matches!(contract, Value::Object(c) if c.get("required") == Some(&Value::Bool(true)))
                })
        );
        let has_transferred_snapshot = owned_snapshot.is_some();
        let mut props = if top && !self.fallback.is_empty() {
            let mut p = self.fallback.clone();
            p.extend(definition.defaults.clone());
            p
        } else {
            definition.defaults.clone()
        };
        props.extend(instance.props.clone());
        let transferred_snapshot = owned_snapshot.is_some();
        if let Some(snapshot) = owned_snapshot.take() {
            props.insert("__state".into(), snapshot);
        }
        let empty_state = Properties::new();
        let result = (|| {
            let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty_state,
            });
            if definition.defaults.contains_key("__matches") {
                let mut selected =
                    select_groups(definition.defaults.get("__matches"), &props, state)?;
                // Explicit scene metadata wins over group defaults without cloning its payload.
                if transferred_snapshot {
                    selected.remove("__state");
                }
                if !selected.is_empty() {
                    // Explicit instance values win over selected groups. Only keys actually
                    // overwritten by a group need to be restored; transport snapshots such
                    // as __state must not be cloned a second time on every instantiation.
                    let overrides: Properties = selected
                        .keys()
                        .filter_map(|key| {
                            instance
                                .props
                                .get(key)
                                .map(|value| (key.clone(), value.clone()))
                        })
                        .collect();
                    props.extend(selected);
                    props.extend(overrides);
                }
            }
            if let Some(value) = instance.props.get("font.size") {
                props.insert("fontSize".into(), value.clone());
            }
            let mut next_parents = Vec::with_capacity(parents.len() + 1);
            next_parents.extend_from_slice(parents);
            next_parents.push(instance.kind.clone());
            let parents = next_parents;
            let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty_state,
            });
            let mut eval_stack = Vec::new();
            let mut resolved_map = BTreeMap::new();
            for (k, v) in &props {
                if !k.starts_with("__") {
                    eval_stack.clear();
                    let v = eval(v, &props, state, &mut eval_stack)?;
                    let v = match v {
                        Value::Node(node) => {
                            Value::Node(Box::new(self.expand(&node, &props, state, &parents, 1)?))
                        }
                        other => other,
                    };
                    resolved_map.insert(k.clone(), v);
                }
            }
            let resolved: Properties = resolved_map.into();
            let mut contract_values = std::borrow::Cow::Borrowed(&resolved);
            if has_required_contracts {
                if let Some(Value::Object(contracts)) = definition.defaults.get("__contracts") {
                    for (key, contract) in contracts {
                        let is_supplied = definition.defaults.contains_key(key)
                            || instance.props.contains_key(key)
                            || (has_transferred_snapshot && key == "__state");
                        if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                            && !is_supplied
                            && contract_values.contains_key(key)
                        {
                            contract_values.to_mut().remove(key);
                        }
                    }
                }
            }
            check_contracts(
                &instance.kind,
                definition.defaults.get("__contracts"),
                definition.defaults.get("__enums"),
                &contract_values,
            )?;
            let mut nodes = std::borrow::Cow::Borrowed(definition.nodes.as_slice());
            if !instance.children.is_empty() {
                fn supply(nodes: &mut [Node], children: &[Node]) -> bool {
                    let mut found = false;
                    for node in nodes {
                        if node.kind == "ContentPresenter"
                            && node.props.get("key") == Some(&Value::Text("content".into()))
                        {
                            node.props.insert(
                                "content".into(),
                                Value::Node(Box::new(Node {
                                    kind: "Frame".into(),
                                    props: Properties::new(),
                                    children: children.to_vec(),
                                    action: None,
                                })),
                            );
                            found = true;
                        }
                        found |= supply(&mut node.children, children);
                    }
                    found
                }
                if !supply(nodes.to_mut(), &instance.children) {
                    return Err("Child content requires ContentPresenter".into());
                }
            }
            if nodes.len() != 1 {
                return Err("A component requires one root".into());
            }
            let mut root = self.expand(&nodes[0], &props, state, &parents, 0)?;
            if top && root.kind != "Rectangle" {
                return Err("A control requires a Rectangle root".into());
            }
            if !top {
                for key in [
                    "key",
                    "cell",
                    "row",
                    "column",
                    "width",
                    "height",
                    "minWidth",
                    "maxWidth",
                    "minHeight",
                    "maxHeight",
                ] {
                    if let Some(value) = resolved.get(key) {
                        root.props.insert(key.into(), value.clone());
                    }
                }
            }
            Ok((root, resolved))
        })();
        if transferred_snapshot {
            *owned_snapshot = props.remove("__state");
        }
        result
    }
    pub(super) fn flatten(
        &self,
        n: &Node,
        bounds: [f64; 4],
        out: &mut String,
    ) -> Result<(), String> {
        let [mut x, mut y, mut w, mut h] = bounds;
        let owned_normalized;
        let n = if matches!(n.kind.as_str(), "Row" | "Column" | "Grid" | "Stack") {
            owned_normalized = normalize_layout(n)?;
            &owned_normalized
        } else {
            n
        };
        let width = dimension(n, 0, w)?;
        let height = dimension(n, 1, h)?;
        x += (w - width) / 2.;
        y += (h - height) / 2.;
        w = width;
        h = height;
        let clip = |out: &mut String, radius: f64| {
            let _ = std::fmt::Write::write_fmt(
                out,
                format_args!(
                    "ContentClip {{ x:{x}; y:{y}; width:{w}; height:{h}; radius:{radius}; }} "
                ),
            );
        };
        match n.kind.as_str() {
            "Frame" => {
                let clipped = boolean(&n.props, "clip", false)?;
                if clipped {
                    clip(out, num(&n.props, "radius", 0.)?);
                }
                let p = insets(n.props.get("padding"))?;
                x += p[3];
                y += p[0];
                w = (w - p[1] - p[3]).max(0.);
                h = (h - p[0] - p[2]).max(0.);
                let cols = sizes(n, 0, w)?;
                let rows = sizes(n, 1, h)?;
                let gap = gaps(&n.props)?;
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
                for c in &n.children {
                    let ci = cell(c, 0, cols.len())?;
                    let ri = cell(c, 1, rows.len())?;
                    self.flatten(
                        c,
                        [
                            x + ci.map_or(0., |i| col_offsets[i]),
                            y + ri.map_or(0., |i| row_offsets[i]),
                            ci.map_or(w, |i| cols[i]),
                            ri.map_or(h, |i| rows[i]),
                        ],
                        out,
                    )?;
                }
                if clipped {
                    out.push_str("ContentClipEnd {} ");
                }
            }
            "Rectangle" => {
                clip(out, num(&n.props, "radius", 0.)?);
                if let Some(color) = n.props.get("background") {
                    let _ = std::fmt::Write::write_fmt(
                        out,
                        format_args!(
                            "ContentShape {{ points:'{x} {y} {} {y} {} {} {x} {}'; color:",
                            x + w,
                            x + w,
                            y + h,
                            y + h,
                        ),
                    );
                    color.write_literal(out)?;
                    out.push_str("; } ");
                }
                for c in &n.children {
                    self.flatten(c, [x, y, w, h], out)?;
                }
                out.push_str("ContentClipEnd {} ");
            }
            "TextInput" => {
                let _ = std::fmt::Write::write_fmt(
                    out,
                    format_args!("ContentInput {{ x:{x}; y:{y}; width:{w}; height:{h}; value:"),
                );
                write_str_literal(&text_ref(&n.props, "value", "")?, out);
                out.push_str("; label:");
                write_str_literal(&text_ref(&n.props, "label", "")?, out);
                out.push_str("; placeholder:");
                write_str_literal(&text_ref(&n.props, "placeholder", "")?, out);
                let _ = std::fmt::Write::write_fmt(
                    out,
                    format_args!(
                        "; color:{}; placeholderColor:{}; fontSize:{}; multiline:{}; readOnly:{}; wrap:{}; }} ",
                        text_ref(&n.props, "color", "#ffffff")?,
                        text_ref(&n.props, "placeholderColor", "#999999")?,
                        num(&n.props, "fontSize", 15.)?,
                        boolean(&n.props, "multiline", false)?,
                        boolean(&n.props, "readOnly", false)?,
                        boolean(&n.props, "wrap", false)?,
                    ),
                );
            }
            "Text" => {
                let value = text_ref(&n.props, "text", "")?;
                let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
                let color = text_ref(&n.props, "color", "#ffffff")?;
                let wrap = boolean(&n.props, "wrap", false)?;
                let family = font_family(&n.props)?;
                // The default family is left implicit, so existing scenes stay byte-identical.
                let family_field = match family {
                    crate::text::FontFamily::Sans => String::new(),
                    other => format!(" fontFamily:{};", other.as_str()),
                };
                if wrap && w > 0. {
                    let wrapped =
                        crate::text::wrap_text_ranges_in(family, &value, fs as f32, w as f32);
                    let multiline = wrapped.len() > 1 || value.contains('\n');
                    if multiline {
                        clip(out, 0.);
                    }
                    for (i, (byte_range, line_w)) in wrapped.iter().enumerate() {
                        let line = &value[byte_range.clone()];
                        let (width, height) = if multiline {
                            (*line_w as f64, fs * 1.5)
                        } else {
                            (w, h)
                        };
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!(
                                "ContentText {{ x:{x}; y:{}; width:{width}; height:{height}; text:",
                                y + i as f64 * fs * 1.5
                            ),
                        );
                        write_str_literal(line, out);
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("; color:{color}; fontSize:{fs};{family_field} }} "),
                        );
                    }
                    if multiline {
                        out.push_str("ContentClipEnd {} ");
                    }
                } else {
                    let multiline = value.contains('\n');
                    if multiline {
                        clip(out, 0.);
                    }
                    for (i, line) in value.split('\n').enumerate() {
                        let (width, height) = if multiline {
                            (
                                crate::text::measure_line_in(family, line, fs as f32)[0] as f64,
                                fs * 1.5,
                            )
                        } else {
                            (w, h)
                        };
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!(
                                "ContentText {{ x:{x}; y:{}; width:{width}; height:{height}; text:",
                                y + i as f64 * fs * 1.5
                            ),
                        );
                        write_str_literal(line, out);
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("; color:{color}; fontSize:{fs};{family_field} }} "),
                        );
                    }
                    if multiline {
                        out.push_str("ContentClipEnd {} ");
                    }
                }
            }
            "Image" => {
                let source = text_ref(&n.props, "source", "")?;
                let image = self
                    .images
                    .get(source.as_ref())
                    .ok_or_else(|| format!("Missing SVG {source}"))?;
                let scale = (w / image.width).min(h / image.height);
                let dx = x + (w - image.width * scale) / 2.;
                let dy = y + (h - image.height * scale) / 2.;
                for (points, color) in &image.shapes {
                    let color = if color == "currentColor" {
                        text_ref(&n.props, "color", "#ffffff")?
                    } else {
                        std::borrow::Cow::Borrowed(color.as_str())
                    };
                    out.push_str("ContentShape { points:'");
                    for (i, p) in points.iter().enumerate() {
                        if i > 0 {
                            out.push(' ');
                        }
                        let _ = std::fmt::Write::write_fmt(
                            out,
                            format_args!("{} {}", dx + p[0] * scale, dy + p[1] * scale),
                        );
                    }
                    let _ = std::fmt::Write::write_fmt(out, format_args!("'; color:{color}; }} "));
                }
            }
            _ => return Err(format!("Unknown visual {}", n.kind)),
        }
        Ok(())
    }
}
