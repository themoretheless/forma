//! Rendering a document to scene text, templates and scene controls.

use super::*;

pub(crate) type RenderedSceneParts = (String, String, Vec<SceneControl>, Option<[f64; 2]>);

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
    pub(crate) fn render_scene_with_extents(&self) -> Result<RenderedSceneParts, String> {
        let mut root = self.expand_scene_root()?;
        let measuring = self.measure_scene_root(&mut root)?;
        let root_size = [
            dimension(&measuring, 0, 360.)?,
            dimension(&measuring, 1, 220.)?,
        ];
        root.props.insert("width".into(), number(root_size[0]));
        root.props.insert("height".into(), number(root_size[1]));
        let scroll = root.children.len() == 1 && root.children[0].kind == "Scroll";
        let scroll_viewport = if scroll {
            Some(root_scroll_viewport(&root, &measuring, root_size)?)
        } else {
            None
        };
        let (instances, logical_end) =
            self.place_scene_instances(&mut root, measuring, scroll_viewport, root_size)?;
        root.kind = "Frame".into();

        let mut emitter = SceneEmitter::with_capacity(instances.len());
        for (instance, precomputed_visual) in instances {
            emitter.emit(self, instance, precomputed_visual)?;
        }
        if scroll {
            root.children[0].children = emitter.source_controls;
        } else {
            root.children = emitter.source_controls;
        }
        Ok((
            self.scene_source(&root, scroll)?,
            emitter.templates,
            emitter.mapping,
            scroll.then_some(logical_end),
        ))
    }

    /// Expands the document to its single layout root.
    fn expand_scene_root(&self) -> Result<Node, String> {
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
        let root = expanded.remove(0);
        if !is_layout(&root.kind) {
            return Err("A scene requires a layout root".into());
        }
        Ok(root)
    }

    /// The root's visual tree, from which its size is read. A plain `Frame` is measured as a
    /// `Column`, which is how its children stack.
    fn measure_scene_root(&self, root: &mut Node) -> Result<Node, String> {
        if root.kind == "Frame"
            && !root.props.contains_key("columns")
            && !root.props.contains_key("rows")
        {
            let kind = std::mem::replace(&mut root.kind, "Column".into());
            let measured = self.scene_visual(root);
            root.kind = kind;
            measured
        } else {
            self.scene_visual(root)
        }
    }

    /// Moves the expanded controls out of the root shell and pairs each with its measured visual,
    /// positioning them when the layout needs it. Returns the controls and, for a root `Scroll`,
    /// the logical end of its content.
    fn place_scene_instances(
        &self,
        root: &mut Node,
        measuring: Node,
        scroll_viewport: Option<[f64; 4]>,
        [root_width, root_height]: [f64; 2],
    ) -> Result<(Vec<SceneInstance>, [f64; 2]), String> {
        let scroll = scroll_viewport.is_some();
        let layout = if scroll { &root.children[0] } else { &*root };
        let advanced = scroll
            || layout.props.contains_key("columns")
            || layout.props.contains_key("rows")
            || !["Frame", "Scroll"].contains(&layout.kind.as_str())
            || layout.children.iter().any(|n| is_layout(&n.kind));
        let mut instances: Vec<SceneInstance> = Vec::new();
        let mut logical_end = [0_f64; 2];
        let precomputed = if scroll {
            measuring.children.into_iter().next()
        } else {
            Some(measuring)
        };
        if advanced {
            let width = num(&root.props, "width", 360.)?;
            let height = num(&root.props, "height", 220.)?;
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
                true,
                &mut scene::ScenePositionTarget {
                    instances: &mut instances,
                    logical_end: &mut logical_end,
                    scroll_content: scroll,
                },
            )?;
            for key in ["columns", "rows", "padding", "gap"] {
                root.props.remove(key);
            }
            root.props.insert("padding".into(), number(0.));
            root.props.insert("gap".into(), number(0.));
            if let Some([x, y, width, height]) = scroll_viewport {
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
        Ok((instances, logical_end))
    }

    /// The scene component as text: the root shell with the emitted controls.
    fn scene_source(&self, root: &Node, scroll: bool) -> Result<String, String> {
        let control_count = if scroll {
            root.children[0].children.len()
        } else {
            root.children.len()
        };
        let mut scene_source = String::with_capacity(256 + control_count * 128);
        scene_source.push_str("component ");
        scene_source.push_str(&self.name);
        scene_source.push_str(" { ");
        write_scene(root, &mut scene_source)?;
        scene_source.push_str(" }");
        Ok(scene_source)
    }
}

/// An expanded control and, when it was measured with the layout, its visual tree.
type SceneInstance = (Node, Option<Node>);

/// Visual kinds flattened into a control's template rather than serialized as they are.
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

/// Properties a control keeps in the scene source; the rest live in its template.
const SCENE_CONTROL_PROPERTIES: [&str; 18] = [
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

/// Where a root `Scroll` sits inside the scene: `[x, y, width, height]`, centred in the content
/// area the root's padding leaves.
fn root_scroll_viewport(
    root: &Node,
    measuring: &Node,
    [root_width, root_height]: [f64; 2],
) -> Result<[f64; 4], String> {
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
    Ok([
        padding[3] + (available[0] - width) / 2.,
        padding[0] + (available[1] - height) / 2.,
        width,
        height,
    ])
}

/// Turns placed controls into the three outputs of a render: the template stream, the control
/// nodes of the scene source and the control records. Its buffers are reused across controls.
struct SceneEmitter {
    templates: String,
    source_controls: Vec<Node>,
    mapping: Vec<SceneControl>,
    content: String,
    template: String,
    /// The state of a control that carries none. Creating a `Properties` allocates, so this
    /// one is shared by every control of the render.
    stateless: Properties,
}

impl SceneEmitter {
    fn with_capacity(controls: usize) -> Self {
        let mut templates = String::with_capacity(32 + controls * 256);
        templates.push_str("FORMA-TEMPLATES-1\n");
        Self {
            templates,
            source_controls: Vec::with_capacity(controls),
            mapping: Vec::with_capacity(controls),
            content: String::with_capacity(256),
            template: String::with_capacity(320),
            stateless: Properties::new(),
        }
    }

    fn emit(
        &mut self,
        document: &Document,
        mut instance: Node,
        precomputed_visual: Option<Node>,
    ) -> Result<(), String> {
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
            None => document.instantiate_scene(&mut instance)?,
        };
        let state = match instance.props.get("__state") {
            Some(Value::Object(state)) => state,
            _ => &self.stateless,
        };
        self.content.clear();
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
        for child in &visual.children {
            if FLATTENABLE_KINDS.contains(&child.kind.as_str()) {
                document.flatten(child, bounds, &mut self.content)?;
            } else {
                serialize_into(child, &mut self.content)?;
                self.content.push(' ');
            }
        }
        if instance.kind == "Slider" {
            write_slider_range(
                document,
                &instance,
                &resolved,
                state,
                bounds,
                &mut self.content,
            )?;
        }
        visual.children.clear();
        self.template.clear();
        self.template.push_str("component Button { ");
        serialize_into(&visual, &mut self.template)?;
        self.template.pop();
        self.template.push_str(&self.content);
        self.template.push_str(" } }");
        let _ = std::fmt::Write::write_fmt(
            &mut self.templates,
            format_args!("{}\n{}", self.template.len(), self.template),
        );
        resolved.insert("width".into(), number(bounds[2]));
        resolved.insert("height".into(), number(bounds[3]));
        // A run Text has no `text` of its own: its label (accessibility, journal) is the
        // concatenated runs.
        if let Some(label) = super::text_runs::plain_label(&resolved) {
            resolved.insert("text".into(), Value::Text(label));
        }
        let props = resolved
            .into_iter()
            .filter(|(k, _)| SCENE_CONTROL_PROPERTIES.contains(&k.as_str()))
            .collect();
        self.source_controls.push(Node {
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
        self.mapping.push(SceneControl {
            node,
            key,
            state: compact_scene_control_state(state),
            event_args,
        });
        Ok(())
    }
}

/// Appends a Slider's `RangeInput`: its current value and the visuals of both endpoints.
fn write_slider_range(
    document: &Document,
    instance: &Node,
    resolved: &Properties,
    state: &Properties,
    bounds: [f64; 4],
    content: &mut String,
) -> Result<(), String> {
    let v = resolved
        .get("value")
        .ok_or("Slider value missing")?
        .text()?
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| "Invalid slider value")?
        / 100.;
    let _ = std::fmt::Write::write_fmt(content, format_args!("RangeInput {{ value:{v}; "));
    let scale = super::RangeBounds::of(|name| resolved.get(name));
    let step = scale.step_fraction();
    if step > 0. {
        let _ = std::fmt::Write::write_fmt(content, format_args!("step:{step}; "));
    }
    if let Some((min, max)) = scale.scale() {
        let _ = std::fmt::Write::write_fmt(content, format_args!("min:{min}; max:{max}; "));
    }
    for (name, value_str) in [("Minimum", "0%"), ("Maximum", "100%")] {
        let mut endpoint = instance.clone();
        endpoint
            .props
            .insert("value".into(), Value::Expr(value_str.into()));
        let (visual, _) = document.instantiate(&endpoint, true, state, &[])?;
        content.push_str(name);
        content.push_str(" { ");
        for c in &visual.children {
            if FLATTENABLE_KINDS.contains(&c.kind.as_str()) {
                document.flatten(c, bounds, content)?;
            }
        }
        content.push_str("} ");
    }
    content.push_str("} ");
    Ok(())
}

/// Scene keys must survive serialization for interaction reconciliation.
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
