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
                    let condition = node
                        .props
                        .get("condition")
                        .ok_or("If requires a condition")?
                        .evaluate(props, &current_state)?
                        .boolean()?;
                    if condition {
                        let body_children: Vec<&Node> =
                            node.children.iter().filter(|n| n.kind != "Else").collect();
                        result.extend(self.expand_sequence(
                            body_children.iter().copied(),
                            props,
                            &current_state,
                            parents,
                            depth + 1,
                        )?);
                    } else if let Some(else_node) = node.children.iter().find(|n| n.kind == "Else")
                    {
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
                    let Value::List(items) = node
                        .props
                        .get("collection")
                        .ok_or("For requires a collection")?
                        .evaluate(props, &current_state)?
                    else {
                        return Err("For expects a list".into());
                    };
                    let item = text(&node.props, "item", "item")?;
                    let idx_key = format!("__index.{item}");
                    let mut keys = std::collections::BTreeSet::new();
                    if items.is_empty() {
                        if let Some(empty) = node.children.iter().find(|n| n.kind == "Empty") {
                            result.extend(self.expand_sequence(
                                &empty.children,
                                props,
                                &current_state,
                                parents,
                                depth + 1,
                            )?);
                        }
                    }
                    let body_children: Vec<&Node> =
                        node.children.iter().filter(|n| n.kind != "Empty").collect();
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
                            body_children.iter().copied(),
                            props,
                            &state,
                            parents,
                            depth + 1,
                        )?);
                    }
                }
                "Else" | "Empty" => {}
                "Let" => {
                    let name = text(&node.props, "name", "")?;
                    if !name.is_empty() {
                        if let Some(val_expr) = node.props.get("value") {
                            let val = val_expr.evaluate(props, &current_state)?;
                            scope_mut(&mut current_state).insert(name, val);
                            if !node.children.is_empty() {
                                result.extend(self.expand_sequence(
                                    &node.children,
                                    props,
                                    &current_state,
                                    parents,
                                    depth + 1,
                                )?);
                            }
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
                        if branch.kind != "Branch" {
                            continue;
                        }
                        let pat = branch
                            .props
                            .get("pattern")
                            .ok_or("Match branch requires a pattern")?;
                        if pattern_matches(pat, &subject, props, &current_state, &mut Vec::new())? {
                            let guard_ok = match branch.props.get("guard") {
                                Some(g) => g.evaluate(props, &current_state)?.boolean()?,
                                None => true,
                            };
                            if guard_ok {
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
        let node = Node {
            kind: n.kind.clone(),
            props: properties(n, props, state)?
                .into_iter()
                .map(|(k, v)| Ok((k, self.expand_value(&v, props, state, parents, depth)?)))
                .collect::<Result<_, String>>()?,
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
        let supplied = definition
            .defaults
            .keys()
            .chain(instance.props.keys())
            .cloned()
            .chain(owned_snapshot.iter().map(|_| "__state".to_owned()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut props = if top {
            self.fallback.clone()
        } else {
            Properties::new()
        };
        props.extend(definition.defaults.clone());
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
            let mut selected = select_groups(definition.defaults.get("__matches"), &props, state)?;
            // Explicit scene metadata wins over group defaults without cloning its payload.
            if transferred_snapshot {
                selected.remove("__state");
            }
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
            if let Some(value) = instance.props.get("font.size") {
                props.insert("fontSize".into(), value.clone());
            }
            let mut parents = parents.to_vec();
            parents.push(instance.kind.clone());
            let state = supplied_state.unwrap_or_else(|| match props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty_state,
            });
            let resolved: Properties = props
                .iter()
                .filter(|(k, _)| !k.starts_with("__"))
                .map(|(k, v)| Ok((k.clone(), self.expand_value(v, &props, state, &parents, 0)?)))
                .collect::<Result<_, String>>()?;
            let mut contract_values = std::borrow::Cow::Borrowed(&resolved);
            if let Some(Value::Object(contracts)) = definition.defaults.get("__contracts") {
                for (key, contract) in contracts {
                    if matches!(contract,Value::Object(contract) if contract.get("required")==Some(&Value::Bool(true)))
                        && !supplied.contains(key)
                    {
                        if contract_values.contains_key(key) {
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
                write_str_literal(&text(&n.props, "value", "")?, out);
                out.push_str("; label:");
                write_str_literal(&text(&n.props, "label", "")?, out);
                out.push_str("; placeholder:");
                write_str_literal(&text(&n.props, "placeholder", "")?, out);
                let _ = std::fmt::Write::write_fmt(
                    out,
                    format_args!(
                        "; color:{}; placeholderColor:{}; fontSize:{}; multiline:{}; readOnly:{}; wrap:{}; }} ",
                        text(&n.props, "color", "#ffffff")?,
                        text(&n.props, "placeholderColor", "#999999")?,
                        num(&n.props, "fontSize", 15.)?,
                        boolean(&n.props, "multiline", false)?,
                        boolean(&n.props, "readOnly", false)?,
                        boolean(&n.props, "wrap", false)?,
                    ),
                );
            }
            "Text" => {
                let value = text(&n.props, "text", "")?;
                let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
                let color = text(&n.props, "color", "#ffffff")?;
                let wrap = boolean(&n.props, "wrap", false)?;
                if wrap && w > 0. {
                    let wrapped = crate::text::wrap_text_ranges(&value, fs as f32, w as f32);
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
                            format_args!("; color:{color}; fontSize:{fs}; }} "),
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
                                crate::text::measure_line(line, fs as f32)[0] as f64,
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
                            format_args!("; color:{color}; fontSize:{fs}; }} "),
                        );
                    }
                    if multiline {
                        out.push_str("ContentClipEnd {} ");
                    }
                }
            }
            "Image" => {
                let source = text(&n.props, "source", "")?;
                let image = self
                    .images
                    .get(&source)
                    .ok_or_else(|| format!("Missing SVG {source}"))?;
                let scale = (w / image.width).min(h / image.height);
                let dx = x + (w - image.width * scale) / 2.;
                let dy = y + (h - image.height * scale) / 2.;
                for (points, color) in &image.shapes {
                    let color = if color == "currentColor" {
                        text(&n.props, "color", "#ffffff")?
                    } else {
                        color.clone()
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
