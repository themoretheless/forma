//! Rendering a document to scene text, templates and scene controls.

use super::*;

impl Document {
    /// Re-evaluate the retained program. The binding owner keeps the last valid scene on error.
    pub fn render(&self) -> Result<(String, String), String> {
        self.render_scene()
            .map(|(source, templates, _)| (source, templates))
    }
    /// Current control identity without visual instantiation/layout/serialization.
    /// A failed visual must not authorize bindings from a branch no longer selected.
    pub(crate) fn binding_scene_controls(&self) -> Result<Vec<SceneControl>, String> {
        let mut walk = scene::SceneWalk::root();
        let mut expanded = self.scene_expand(
            &self.root,
            &Properties::new(),
            "",
            &Properties::new(),
            0,
            &mut walk,
        )?;
        if expanded.len() != 1 || !is_layout(&expanded[0].kind) {
            return Err("A scene requires one layout root".into());
        }
        fn collect(mut node: Node, controls: &mut Vec<SceneControl>) -> Result<(), String> {
            if is_layout(&node.kind) {
                for child in node.children {
                    collect(child, controls)?;
                }
            } else {
                let id = num(&node.props, "__node", 0.)? as usize;
                let key = text(&node.props, "key", "")?;
                let state = match node.props.remove("__state") {
                    Some(Value::Object(state)) => state,
                    _ => Properties::new(),
                };
                let event_args = match node.props.remove("__event_args") {
                    Some(Value::Object(events)) => events
                        .into_iter()
                        .map(|(name, value)| {
                            let Value::List(values) = value else {
                                return Err("Event arguments must be a list".to_string());
                            };
                            Ok((name, values))
                        })
                        .collect::<Result<_, String>>()?,
                    _ => BTreeMap::new(),
                };
                controls.push(SceneControl {
                    node: id,
                    key,
                    state: compact_scene_control_state(state),
                    event_args,
                });
            }
            Ok(())
        }
        let mut controls = Vec::new();
        collect(expanded.remove(0), &mut controls)?;
        Ok(controls)
    }
    pub fn render_scene(&self) -> Result<(String, String, Vec<SceneControl>), String> {
        self.render_scene_with_extents()
            .map(|(source, templates, controls, _)| (source, templates, controls))
    }
    /// Render an owned artifact for `Runtime::from_scene_artifact`, retaining empty layout extents.
    pub fn render_scene_artifact(&self) -> Result<SceneArtifact, String> {
        let (source, templates, controls, logical_content_end) =
            self.render_scene_with_extents()?;
        Ok(SceneArtifact {
            source,
            templates,
            controls,
            logical_content_end,
        })
    }
    /// Runtime metadata retains logical layout bounds that serialized control leaves cannot express.
    pub(crate) fn render_scene_with_extents(
        &self,
    ) -> Result<(String, String, Vec<SceneControl>, Option<[f64; 2]>), String> {
        let mut walk = scene::SceneWalk::root();
        let mut expanded = self.scene_expand(
            &self.root,
            &Properties::new(),
            "",
            &Properties::new(),
            0,
            &mut walk,
        )?;
        if expanded.len() != 1 {
            return Err("A scene requires one layout root".into());
        }
        let mut root = expanded.remove(0);
        if !is_layout(&root.kind) {
            return Err("A scene requires a layout root".into());
        }
        let measuring = if root.kind == "Frame"
            && !root.props.contains_key("columns")
            && !root.props.contains_key("rows")
        {
            let kind = std::mem::replace(&mut root.kind, "Column".into());
            let measured = self.scene_visual(&mut root);
            root.kind = kind;
            measured?
        } else {
            self.scene_visual(&mut root)?
        };
        let root_width = dimension(&measuring, 0, 360.)?;
        let root_height = dimension(&measuring, 1, 220.)?;
        root.props.insert("width".into(), number(root_width));
        root.props.insert("height".into(), number(root_height));
        let scroll = root.children.len() == 1 && root.children[0].kind == "Scroll";
        let scroll_viewport = if scroll {
            let layout = &root.children[0];
            for key in layout.props.keys() {
                if !key.starts_with("__")
                    && ![
                        "key",
                        "width",
                        "height",
                        "minWidth",
                        "maxWidth",
                        "minHeight",
                        "maxHeight",
                        "padding",
                        "gap",
                        "columns",
                        "rows",
                    ]
                    .contains(&key.as_str())
                {
                    return Err(format!("Unsupported root Scroll property: {key}"));
                }
            }
            if layout.action.is_some() {
                return Err("Root Scroll actions are unsupported".into());
            }
            let padding = insets(root.props.get("padding"))?;
            let available = [
                root_width - padding[1] - padding[3],
                root_height - padding[0] - padding[2],
            ];
            let visual = measuring.children.first().ok_or("Missing Scroll visual")?;
            let width = dimension(visual, 0, available[0])?;
            let height = dimension(visual, 1, available[1])?;
            if width < 0. || height < 0. || width > available[0] || height > available[1] {
                return Err("Root Scroll viewport must fit inside the scene content area".into());
            }
            Some([
                padding[3] + (available[0] - width) / 2.,
                padding[0] + (available[1] - height) / 2.,
                width,
                height,
            ])
        } else {
            None
        };
        let layout = if scroll { &root.children[0] } else { &root };
        let advanced = scroll
            || layout.props.contains_key("columns")
            || layout.props.contains_key("rows")
            || !["Frame", "Scroll"].contains(&layout.kind.as_str())
            || layout.children.iter().any(|n| is_layout(&n.kind));
        let mut instances: Vec<(Node, Option<Node>)> = Vec::new();
        let mut logical_end = [0_f64; 2];
        if advanced {
            let width = num(&root.props, "width", 360.)?;
            let height = num(&root.props, "height", 220.)?;
            let precomputed = if scroll {
                measuring.children.into_iter().next()
            } else {
                Some(measuring)
            };
            let mut positioned = Node {
                kind: layout.kind.clone(),
                props: layout.props.clone(),
                children: Vec::new(),
                action: layout.action.clone(),
            };
            // Retain only the root/Scroll shell for final scene serialization.
            // Expanded children and their owned snapshots move into positioning.
            positioned.children = if scroll {
                std::mem::take(&mut root.children[0].children)
            } else {
                std::mem::take(&mut root.children)
            };
            self.position_scene(
                positioned,
                precomputed,
                scroll_viewport.unwrap_or([0., 0., width, height]),
                &mut instances,
                true,
                &mut logical_end,
                scroll,
            )?;
            for key in ["columns", "rows", "padding", "gap"] {
                root.props.remove(key);
            }
            root.props.insert("padding".into(), number(0.));
            root.props.insert("gap".into(), number(0.));
            if scroll {
                let [x, y, width, height] = scroll_viewport.unwrap();
                root.props.insert(
                    "padding".into(),
                    Value::List(vec![
                        number(y),
                        number(root_width - x - width),
                        number(root_height - y - height),
                        number(x),
                    ]),
                );
                for key in ["columns", "rows", "padding", "gap"] {
                    root.children[0].props.remove(key);
                }
            }
        } else {
            let precomputed = if scroll {
                measuring.children.into_iter().next()
            } else {
                Some(measuring)
            };
            // The root shell is serialized separately; transfer its expanded
            // children and their snapshots just as the positioned path does.
            let children = if scroll {
                std::mem::take(&mut root.children[0].children)
            } else {
                std::mem::take(&mut root.children)
            };
            if let Some(vis) = precomputed.filter(|v| v.children.len() == children.len()) {
                instances = children
                    .into_iter()
                    .zip(vis.children.into_iter().map(Some))
                    .collect();
            } else {
                instances = children.into_iter().map(|n| (n, None)).collect();
            }
        }
        root.kind = "Frame".into();
        let empty = Properties::new();
        let mut mapping = Vec::with_capacity(instances.len());
        let mut source_controls = Vec::with_capacity(instances.len());
        let mut templates = String::with_capacity(32 + instances.len() * 256);
        let mut content = String::with_capacity(256);
        let mut template = String::with_capacity(320);
        templates.push_str("FORMA-TEMPLATES-1\n");
        for (mut instance, precomputed_visual) in instances {
            let event_args = match instance.props.get("__event_args") {
                Some(Value::Object(events)) => events
                    .iter()
                    .map(|(name, value)| {
                        let Value::List(values) = value else {
                            return Err("Event arguments must be a list".to_string());
                        };
                        Ok((name.clone(), values.clone()))
                    })
                    .collect::<Result<_, String>>()?,
                _ => BTreeMap::new(),
            };
            let node = num(&instance.props, "__node", 0.)? as usize;
            let key = text(&instance.props, "key", "")?;
            let (mut visual, mut resolved) = match precomputed_visual {
                Some(mut visual) => {
                    let mut resolved = std::mem::take(&mut visual.props);
                    let rect_props = match resolved.remove("__rect_props") {
                        Some(Value::Object(p)) => p,
                        _ => resolved.clone(),
                    };
                    for key in ["x", "y", "width", "height", "key"] {
                        if let Some(val) = instance.props.get(key) {
                            resolved.insert(key.into(), val.clone());
                        }
                    }
                    visual.props = rect_props;
                    (visual, resolved)
                }
                None => self.instantiate_scene(&mut instance)?,
            };
            let state = match instance.props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &empty,
            };
            content.clear();
            // Measure the same children with resolved instance properties,
            // then restore rectangle properties before flattening/serialization.
            std::mem::swap(&mut visual.props, &mut resolved);
            let bounds = [
                0.,
                0.,
                dimension(&visual, 0, 260.)?,
                dimension(&visual, 1, 70.)?,
            ];
            std::mem::swap(&mut visual.props, &mut resolved);
            const FLATTENABLE_KINDS: [&str; 9] = [
                "Frame",
                "Row",
                "Column",
                "Grid",
                "Stack",
                "Text",
                "TextInput",
                "Image",
                "Rectangle",
            ];
            for child in &visual.children {
                if FLATTENABLE_KINDS.contains(&child.kind.as_str()) {
                    self.flatten(child, bounds, &mut content)?;
                } else {
                    serialize_into(child, &mut content)?;
                    content.push(' ');
                }
            }
            if instance.kind == "Slider" {
                let v = resolved
                    .get("value")
                    .ok_or("Slider value missing")?
                    .text()?
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .map_err(|_| "Invalid slider value")?
                    / 100.;
                let _ = std::fmt::Write::write_fmt(
                    &mut content,
                    format_args!("RangeInput {{ value:{v}; "),
                );
                for (name, value_str) in [("Minimum", "0%"), ("Maximum", "100%")] {
                    let mut endpoint = instance.clone();
                    endpoint
                        .props
                        .insert("value".into(), Value::Expr(value_str.into()));
                    let (visual, _) = self.instantiate(&endpoint, true, state, &[])?;
                    content.push_str(name);
                    content.push_str(" { ");
                    for c in &visual.children {
                        if FLATTENABLE_KINDS.contains(&c.kind.as_str()) {
                            self.flatten(c, bounds, &mut content)?;
                        }
                    }
                    content.push_str("} ");
                }
                content.push_str("} ");
            }
            visual.children.clear();
            template.clear();
            template.push_str("component Button { ");
            serialize_into(&visual, &mut template)?;
            template.pop();
            template.push_str(&content);
            template.push_str(" } }");
            let _ = std::fmt::Write::write_fmt(
                &mut templates,
                format_args!("{}\n{template}", template.len()),
            );
            let standard = [
                "key",
                "x",
                "y",
                "width",
                "height",
                "text",
                "fontSize",
                "color",
                "radius",
                "disabled",
                "background",
                "hoverBackground",
                "pressedBackground",
                "disabledBackground",
                "borderWidth",
                "borderColor",
                "focusBorderColor",
                "transitionDuration",
            ];
            resolved.insert("width".into(), number(bounds[2]));
            resolved.insert("height".into(), number(bounds[3]));
            let props = resolved
                .into_iter()
                .filter(|(k, _)| standard.contains(&k.as_str()))
                .collect();
            source_controls.push(Node {
                kind: "Button".into(),
                props,
                children: vec![],
                action: instance.action.clone(),
            });
            // Templates (including Slider endpoints) may read props.__state.
            // Transfer its owned snapshot only after their last evaluation.
            let state = match instance.props.remove("__state") {
                Some(Value::Object(state)) => state,
                _ => Properties::new(),
            };
            mapping.push(SceneControl {
                node,
                key,
                state: compact_scene_control_state(state),
                event_args,
            });
        }
        // Scene keys must survive serialization for interaction reconciliation.
        fn write_scene(n: &Node, out: &mut String) -> Result<(), String> {
            out.push_str(&n.kind);
            out.push_str(" { ");
            let mut first = true;
            for (k, v) in &n.props {
                if n.kind == "Scroll"
                    || k.starts_with("__")
                    || ["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str())
                    || (k.as_str() == "key" && ["Frame", "Scroll"].contains(&n.kind.as_str()))
                {
                    continue;
                }
                if !first {
                    out.push(' ');
                }
                first = false;
                out.push_str(k);
                out.push(':');
                write_property_literal(k, v, out)?;
                out.push(';');
            }
            out.push(' ');
            if let Some(a) = &n.action {
                out.push_str("clicked -> ");
                out.push_str(a);
                out.push_str("();");
            }
            out.push(' ');
            for (i, child) in n.children.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_scene(child, out)?;
            }
            out.push_str(" }");
            Ok(())
        }
        if scroll {
            root.children[0].children = source_controls;
        } else {
            root.children = source_controls;
        }
        let control_count = if scroll {
            root.children[0].children.len()
        } else {
            root.children.len()
        };
        let mut scene_source = String::with_capacity(256 + control_count * 128);
        scene_source.push_str("component ");
        scene_source.push_str(&self.name);
        scene_source.push_str(" { ");
        write_scene(&root, &mut scene_source)?;
        scene_source.push_str(" }");
        Ok((
            scene_source,
            templates,
            mapping,
            scroll.then_some(logical_end),
        ))
    }
}
