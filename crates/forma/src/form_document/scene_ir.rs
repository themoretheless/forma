//! Direct typed [`SceneIr`] construction from [`Document`], bypassing `serialize_into` and
//! `FORMA-TEMPLATES-1` parsing on all internal `Document -> Runtime` paths.

use super::render::{
    PreparedScene, extract_scene_control, finalize_control_props, resolve_control_visual,
};
use super::text_runs::{IrRunTargets, flatten_ir};
use super::*;
use std::sync::Arc;

/// Typed intermediate representation ready for `Runtime::from_scene_ir` without
/// intermediate source or template text serialization.
#[derive(Clone, Debug)]
pub struct SceneIr {
    pub scene: crate::markup::Scene,
    pub templates: Vec<Arc<crate::template::Template>>,
    pub controls: Vec<SceneControl>,
    pub logical_content_end: Option<[f64; 2]>,
}

impl Document {
    /// Compiles and lays out this document directly into a validated typed [`SceneIr`].
    pub fn render_scene_ir(&self) -> Result<SceneIr, String> {
        let prepared = self.prepare_scene()?;
        let PreparedScene {
            root,
            instances,
            scroll,
            logical_end,
            scroll_content_sizes,
        } = prepared;

        let width = num(&root.props, "width", 360.)? as f32;
        let height = num(&root.props, "height", 220.)? as f32;
        let background =
            optional_color_prop(&root.props, "background")?.unwrap_or([24, 30, 42, 255]);
        let radius = num(&root.props, "radius", 0.)? as f32;
        let has_clip = root.props.contains_key("clip");
        let has_overflow = root.props.contains_key("overflow");
        if has_clip && has_overflow {
            return Err("Use clip or legacy overflow, not both".into());
        }
        let overflow = text(&root.props, "overflow", "visible")?;
        if !matches!(overflow.as_str(), "visible" | "hidden") {
            return Err(
                "overflow: expected visible or hidden; use a Scroll element for scrolling".into(),
            );
        }
        let clip = boolean(&root.props, "clip", false)? || overflow == "hidden";
        let padding_f64 = insets(root.props.get("padding"))?;
        let padding = padding_f64.map(|v| v as f32);
        let gap = match root.props.get("gap") {
            None => 0.,
            Some(Value::List(values)) => match values.as_slice() {
                [v] | [v, _] => v.number()? as f32,
                _ => return Err("Expected 1 or 2 gap values".into()),
            },
            Some(v) => v.number()? as f32,
        };

        let mut buttons = Vec::with_capacity(instances.len());
        let mut templates = Vec::with_capacity(instances.len());
        let mut mapping = Vec::with_capacity(instances.len());
        let mut pool: Vec<(u64, Arc<crate::template::Template>)> = Vec::new();
        let stateless = Properties::new();

        for (mut instance, precomputed_visual) in instances {
            let (visual, mut resolved, bounds) =
                resolve_control_visual(self, &mut instance, precomputed_visual)?;
            let state = match instance.props.get("__state") {
                Some(Value::Object(state)) => state,
                _ => &stateless,
            };

            let mut template = build_template_shell(&visual, &resolved)?;
            for child in &visual.children {
                if child.kind.is_flattenable() {
                    self.flatten_to_ir(child, bounds, &mut template)?;
                } else {
                    apply_primitive_ir(child, &mut template)?;
                }
            }
            if instance.kind == BuiltinKind::Slider {
                template.range = Some(build_slider_range_ir(
                    self, &instance, &resolved, state, bounds,
                )?);
            }

            finalize_control_props(&mut resolved, bounds);
            let control_props: Properties = resolved
                .into_iter()
                .filter(|(k, _)| super::registry::is_scene_control_property(k))
                .collect();
            let button_spec = build_button_spec(&control_props, instance.action.as_deref())?;
            template.props = button_spec.clone();

            let shared =
                intern_validated_template(template, &mut pool).map_err(friendly_scene_error)?;
            buttons.push(button_spec);
            templates.push(shared);
            mapping.push(extract_scene_control(&mut instance, &scroll_content_sizes)?);
        }

        let button = buttons.first().cloned().unwrap_or_default();
        let mut scene = crate::markup::Scene {
            name: self.name.clone(),
            width,
            height,
            background,
            overflow,
            clip,
            radius,
            scroll,
            padding,
            content_width: width,
            content_height: height,
            button,
            buttons,
            gap,
            modals: Vec::new(),
        };
        crate::template::validate_scene_spec(&mut scene).map_err(friendly_scene_error)?;

        Ok(SceneIr {
            scene,
            templates,
            controls: mapping,
            logical_content_end: logical_end,
        })
    }

    fn flatten_to_ir(
        &self,
        n: &Node,
        bounds: [f64; 4],
        template: &mut crate::template::Template,
    ) -> Result<(), String> {
        let [mut x, mut y, mut w, mut h] = bounds;
        let owned_normalized;
        let n = if matches!(
            n.kind.builtin(),
            Some(BuiltinKind::Row | BuiltinKind::Column | BuiltinKind::Grid | BuiltinKind::Stack)
        ) {
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

        let push_clip = |content: &mut Vec<crate::template::Content>, radius: f64| {
            content.push(crate::template::Content::Clip {
                bounds: [x as f32, y as f32, w as f32, h as f32],
                radius: radius as f32,
            });
        };

        match n.kind.builtin() {
            Some(BuiltinKind::Frame) => {
                let clipped = boolean(&n.props, "clip", false)?;
                if clipped {
                    push_clip(&mut template.content, num(&n.props, "radius", 0.)?);
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
                    self.flatten_to_ir(
                        c,
                        [
                            x + ci.map_or(0., |i| col_offsets[i]),
                            y + ri.map_or(0., |i| row_offsets[i]),
                            ci.map_or(w, |i| cols[i]),
                            ri.map_or(h, |i| rows[i]),
                        ],
                        template,
                    )?;
                }
                if clipped {
                    template.content.push(crate::template::Content::ClipEnd);
                }
            }
            Some(BuiltinKind::Rectangle) => {
                push_clip(&mut template.content, num(&n.props, "radius", 0.)?);
                if let Some(color_val) = n.props.get("background") {
                    let color = value_color(color_val, "background")?;
                    let (lx, ty, rx, by) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
                    template.content.push(crate::template::Content::Shape {
                        points: vec![[lx, ty], [rx, ty], [rx, by], [lx, by]],
                        color,
                    });
                }
                for c in &n.children {
                    self.flatten_to_ir(c, [x, y, w, h], template)?;
                }
                template.content.push(crate::template::Content::ClipEnd);
            }
            Some(BuiltinKind::Canvas) => {
                if template.canvas.is_some() {
                    return Err("One Canvas per control is supported".into());
                }
                template.canvas = Some(crate::template::CanvasSlot {
                    x: x as f32,
                    y: y as f32,
                    width: Some(w as f32),
                    height: Some(h as f32),
                });
            }
            Some(BuiltinKind::TextInput) => {
                if n.props.contains_key("direction") && n.props.contains_key("dir") {
                    return Err("direction and dir are the same property".into());
                }
                let dir_val = n.props.get("direction").or_else(|| n.props.get("dir"));
                let direction = match dir_val {
                    Some(v) => crate::text::TextDirection::parse(&v.text()?)?,
                    None => crate::text::TextDirection::Ltr,
                };
                let mut input = crate::template::Input {
                    bounds: [x as f32, y as f32, w as f32, h as f32],
                    value: text_ref(&n.props, "value", "")?.into_owned(),
                    placeholder: text_ref(&n.props, "placeholder", "")?.into_owned(),
                    label: text_ref(&n.props, "label", "")?.into_owned(),
                    read_only: boolean(&n.props, "readOnly", false)?,
                    color: optional_color_prop(&n.props, "color")?.unwrap_or([255; 4]),
                    placeholder_color: optional_color_prop(&n.props, "placeholderColor")?
                        .unwrap_or([150, 150, 150, 255]),
                    font_size: num(&n.props, "fontSize", 15.)? as f32,
                    multiline: boolean(&n.props, "multiline", false)?,
                    wrap: boolean(&n.props, "wrap", false)?,
                    direction,
                };
                if n.props.get("placeholderColor").is_none() {
                    input.placeholder_color = parse_hex_color("#999999", "placeholderColor")?;
                }
                template.inputs.push(input);
                if template.inputs.len() > 1 {
                    return Err("One TextInput per control is supported".into());
                }
            }
            Some(BuiltinKind::Text)
                if n.props
                    .get("runs")
                    .is_some_and(|v| !matches!(v, Value::Null)) =>
            {
                if let Some(runs) = super::text_runs::runs(&n.props)? {
                    let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
                    let wrap = boolean(&n.props, "wrap", false)?;
                    let mut targets = IrRunTargets {
                        content: &mut template.content,
                        links: &mut template.links,
                    };
                    flatten_ir(&runs, fs, wrap, [x, y, w, h], parse_hex_color, &mut targets)?;
                }
            }
            Some(BuiltinKind::Text) => {
                if n.props.contains_key("direction") && n.props.contains_key("dir") {
                    return Err("direction and dir are the same property".into());
                }
                let direction = match n.props.get("direction").or_else(|| n.props.get("dir")) {
                    Some(v) => crate::text::TextDirection::parse(&v.text()?)?,
                    None => crate::text::TextDirection::Ltr,
                };
                let value = text_ref(&n.props, "text", "")?;
                let fs = num(&n.props, "fontSize", num(&n.props, "font.size", 16.)?)?;
                let color = optional_color_prop(&n.props, "color")?.unwrap_or([255; 4]);
                let wrap = boolean(&n.props, "wrap", false)?;
                let family = font_family(&n.props)?;
                if wrap && w > 0. {
                    let wrapped =
                        crate::text::wrap_text_ranges_in(family, &value, fs as f32, w as f32);
                    let multiline = wrapped.len() > 1 || value.contains('\n');
                    if multiline {
                        push_clip(&mut template.content, 0.);
                    }
                    for (i, (byte_range, line_w)) in wrapped.iter().enumerate() {
                        let line = &value[byte_range.clone()];
                        let (bx, bw, bh) = if multiline {
                            let align = if direction != crate::text::TextDirection::Ltr
                                && crate::text::resolve_direction(line, direction)
                                    == crate::text::ResolvedDirection::Rtl
                            {
                                (w - f64::from(*line_w)).max(0.)
                            } else {
                                0.
                            };
                            ((x + align) as f32, *line_w, (fs * 1.5) as f32)
                        } else {
                            (x as f32, w as f32, h as f32)
                        };
                        template.content.push(crate::template::Content::Text {
                            bounds: [bx, (y + i as f64 * fs * 1.5) as f32, bw, bh],
                            text: crate::template::Text {
                                text: line.to_owned(),
                                color,
                                font_size: fs as f32,
                                family,
                                direction,
                            },
                        });
                    }
                    if multiline {
                        template.content.push(crate::template::Content::ClipEnd);
                    }
                } else {
                    let multiline = value.contains('\n');
                    if multiline {
                        push_clip(&mut template.content, 0.);
                    }
                    for (i, line) in value.split('\n').enumerate() {
                        let (bx, bw, bh) = if multiline {
                            let line_w = crate::text::measure_line_in(family, line, fs as f32)[0];
                            let align = if direction != crate::text::TextDirection::Ltr
                                && crate::text::resolve_direction(line, direction)
                                    == crate::text::ResolvedDirection::Rtl
                            {
                                (w - f64::from(line_w)).max(0.)
                            } else {
                                0.
                            };
                            ((x + align) as f32, line_w, (fs * 1.5) as f32)
                        } else {
                            (x as f32, w as f32, h as f32)
                        };
                        template.content.push(crate::template::Content::Text {
                            bounds: [bx, (y + i as f64 * fs * 1.5) as f32, bw, bh],
                            text: crate::template::Text {
                                text: line.to_owned(),
                                color,
                                font_size: fs as f32,
                                family,
                                direction,
                            },
                        });
                    }
                    if multiline {
                        template.content.push(crate::template::Content::ClipEnd);
                    }
                }
            }
            Some(BuiltinKind::Image) => {
                let source = text_ref(&n.props, "source", "")?;
                let image = self
                    .images
                    .get(source.as_ref())
                    .ok_or_else(|| format!("Missing SVG {source}"))?;
                let scale = (w / image.width).min(h / image.height);
                let dx = x + (w - image.width * scale) / 2.;
                let dy = y + (h - image.height * scale) / 2.;
                for (points, raw_color) in &image.shapes {
                    let color = if raw_color == "currentColor" {
                        optional_color_prop(&n.props, "color")?.unwrap_or([255; 4])
                    } else {
                        parse_hex_color(raw_color, "color")?
                    };
                    let pts = points
                        .iter()
                        .map(|p| [(dx + p[0] * scale) as f32, (dy + p[1] * scale) as f32])
                        .collect();
                    template
                        .content
                        .push(crate::template::Content::Shape { points: pts, color });
                }
            }
            _ => return Err(format!("Unknown visual {}", n.kind)),
        }
        Ok(())
    }
}

fn template_hash(template: &crate::template::Template) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mix = |h: &mut u64, v: u64| {
        *h ^= v;
        *h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for byte in template.props.key.as_bytes() {
        mix(&mut h, u64::from(*byte));
    }
    mix(&mut h, 0xff);
    for byte in template.props.text.as_bytes() {
        mix(&mut h, u64::from(*byte));
    }
    mix(&mut h, template.content.len() as u64);
    mix(&mut h, template.inputs.len() as u64);
    mix(&mut h, u64::from(template.props.width.to_bits()));
    mix(&mut h, u64::from(template.props.height.to_bits()));
    mix(&mut h, u64::from(template.radius.to_bits()));
    mix(&mut h, u64::from(template.clickable));
    h
}

fn intern_validated_template(
    template: crate::template::Template,
    pool: &mut Vec<(u64, Arc<crate::template::Template>)>,
) -> Result<Arc<crate::template::Template>, String> {
    let hash = template_hash(&template);
    if let Some((_, existing)) = pool
        .iter()
        .find(|(h, t)| *h == hash && t.as_ref() == &template)
    {
        return Ok(Arc::clone(existing));
    }
    crate::template::validate_template(&template)?;
    let shared = Arc::new(template);
    pool.push((hash, Arc::clone(&shared)));
    Ok(shared)
}

fn build_slider_range_ir(
    document: &Document,
    instance: &Node,
    resolved: &Properties,
    state: &Properties,
    bounds: [f64; 4],
) -> Result<crate::template::RangeInput, String> {
    let v = resolved
        .get("value")
        .ok_or("Slider value missing")?
        .text()?
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| "Invalid slider value")?
        / 100.;
    let scale = super::RangeBounds::of(|name| resolved.get(name));
    let step = scale.step_fraction();
    let (min, max) = scale.scale().unwrap_or((0., 1.));

    let mut endpoints = Vec::with_capacity(2);
    for value_str in ["0%", "100%"] {
        let mut endpoint = instance.clone();
        endpoint
            .props
            .insert("value".into(), Value::Expr(value_str.into()));
        let (visual, _) = document.instantiate(&endpoint, true, state, &[])?;
        let mut temp_template = crate::template::Template {
            props: Default::default(),
            radius: 0.,
            fill: None,
            border: None,
            reveal: None,
            text: None,
            clickable: false,
            live: crate::template::A11yLive::Off,
            content: Vec::new(),
            inputs: Vec::new(),
            range: None,
            canvas: None,
            links: Vec::new(),
        };
        for c in &visual.children {
            if c.kind.is_flattenable() {
                document.flatten_to_ir(c, bounds, &mut temp_template)?;
            }
        }
        endpoints.push(temp_template.content);
    }
    let maximum = endpoints.pop().unwrap_or_default();
    let minimum = endpoints.pop().unwrap_or_default();
    let scaled = min.is_finite() && max.is_finite() && min < max && (max - min).is_finite();
    Ok(crate::template::RangeInput {
        value: (v as f32).clamp(0., 1.),
        step: if step.is_finite() {
            (step as f32).clamp(0., 1.)
        } else {
            0.
        },
        min: if scaled { min as f32 } else { 0. },
        max: if scaled { max as f32 } else { 1. },
        minimum,
        maximum,
    })
}

fn build_template_shell(
    visual: &Node,
    control_props: &Properties,
) -> Result<crate::template::Template, String> {
    let mut template = crate::template::Template {
        props: crate::markup::ButtonSpec::default(),
        radius: 0.,
        fill: None,
        border: None,
        reveal: None,
        text: None,
        clickable: false,
        live: crate::template::A11yLive::Off,
        content: Vec::new(),
        inputs: Vec::new(),
        range: None,
        canvas: None,
        links: Vec::new(),
    };
    if visual.kind != BuiltinKind::Rectangle {
        return Err("A control requires a Rectangle root".into());
    }
    for (k, v) in &visual.props {
        if k == "key" || k == "transition" || k.starts_with("__") {
            continue;
        }
        if ["minWidth", "maxWidth", "minHeight", "maxHeight"].contains(&k.as_str()) {
            continue;
        }
        match k.as_str() {
            "radius" => template.radius = v.number()? as f32,
            "background" => template.fill = Some(parse_brush_value(v, control_props)?),
            "live" => {
                let raw = v.text()?;
                template.live = crate::template::A11yLive::parse(&raw)?;
            }
            "Border" | "Reveal" | "Text" | "PointerArea" => {}
            other => {
                return Err(format!(
                    "Unsupported Rectangle field or primitive '{other}'"
                ));
            }
        }
    }
    if !visual.props.contains_key("live") {
        if let Some(v) = control_props.get("live") {
            let raw = v.text()?;
            template.live = crate::template::A11yLive::parse(&raw)?;
        }
    }
    Ok(template)
}

fn apply_primitive_ir(node: &Node, template: &mut crate::template::Template) -> Result<(), String> {
    match node.kind.builtin() {
        Some(BuiltinKind::Border) => {
            if template.border.is_some() {
                return Err("Duplicate template field or primitive 'Border'".into());
            }
            let width = num(&node.props, "width", 1.)? as f32;
            let bg = node
                .props
                .get("background")
                .ok_or("Border requires background: Brush { ... };")?;
            let brush = parse_brush_value(bg, &Properties::new())?;
            template.border = Some(crate::template::Border { width, brush });
        }
        Some(BuiltinKind::Reveal) => {
            if template.reveal.is_some() {
                return Err("Duplicate template field or primitive 'Reveal'".into());
            }
            template.reveal = Some(parse_reveal_node(node)?);
        }
        Some(BuiltinKind::PointerArea) => {
            if template.clickable {
                return Err("Duplicate PointerArea clicked event".into());
            }
            if let Some(action) = &node.action {
                if action != "events.clicked" && action != "clicked" {
                    return Err(
                        "events.clicked() forwarding; arbitrary code is not supported".into(),
                    );
                }
                template.clickable = true;
            }
        }
        _ => {
            return Err(format!(
                "Unsupported Rectangle field or primitive '{}'",
                node.kind
            ));
        }
    }
    Ok(())
}

fn parse_brush_value(value: &Value, _scope: &Properties) -> Result<crate::template::Brush, String> {
    match value {
        Value::Node(node) if node.kind == BuiltinKind::Brush => {
            let color_val = node
                .props
                .get("color")
                .ok_or("Brush requires a base 'color'")?;
            Ok(crate::template::Brush {
                color: value_color(color_val, "color")?,
                hover: optional_color_prop(&node.props, "hover")?,
                pressed: optional_color_prop(&node.props, "pressed")?,
                disabled: optional_color_prop(&node.props, "disabled")?,
                focus: optional_color_prop(&node.props, "focus")?,
                duration_ms: match node.props.get("transition") {
                    Some(v) => parse_duration_value(v, "transition")?,
                    None => 0.,
                },
            })
        }
        other => Ok(crate::template::Brush {
            color: value_color(other, "brush")?,
            hover: None,
            pressed: None,
            disabled: None,
            focus: None,
            duration_ms: 0.,
        }),
    }
}

fn parse_reveal_node(node: &Node) -> Result<crate::reveal::Reveal, String> {
    let mut reveal = crate::reveal::Reveal::default();
    for (k, v) in &node.props {
        match k.as_str() {
            "targetX" => reveal.target_x = Some(v.number()? as f32),
            "targetY" => reveal.target_y = Some(v.number()? as f32),
            "targetWidth" => reveal.target_width = Some(v.number()? as f32),
            "targetHeight" => reveal.target_height = Some(v.number()? as f32),
            "targetRadius" => reveal.target_radius = Some(v.number()? as f32),
            "width" => reveal.width = v.number()? as f32,
            "color" => reveal.color = value_color(v, "color")?,
            "radius" => reveal.radius = v.number()? as f32,
            "activeRadius" => reveal.active_radius = v.number()? as f32,
            "transition" => reveal.duration_ms = parse_duration_value(v, "transition")?,
            "stop" => reveal.stop = v.number()? as f32,
            "activeStop" => reveal.active_stop = v.number()? as f32,
            key if key.starts_with("__") => {}
            other => return Err(format!("Unsupported Reveal field '{other}'")),
        }
    }
    Ok(reveal)
}

fn build_button_spec(
    props: &Properties,
    action: Option<&str>,
) -> Result<crate::markup::ButtonSpec, String> {
    let mut spec = crate::markup::ButtonSpec::default();
    for (k, v) in props {
        match k.as_str() {
            "key" => {
                spec.specified.try_insert("key");
                spec.key = v.text()?;
            }
            "x" => {
                spec.specified.try_insert("x");
                spec.x = v.number()? as f32;
            }
            "y" => {
                spec.specified.try_insert("y");
                spec.y = v.number()? as f32;
            }
            "width" => {
                spec.specified.try_insert("width");
                spec.width = v.number()? as f32;
            }
            "height" => {
                spec.specified.try_insert("height");
                spec.height = v.number()? as f32;
            }
            "radius" => {
                spec.specified.try_insert("radius");
                spec.radius = v.number()? as f32;
            }
            "background" => {
                spec.specified.try_insert("background");
                spec.background = match v {
                    Value::Node(node) if node.kind == BuiltinKind::Brush => {
                        let color = node
                            .props
                            .get("color")
                            .ok_or("Brush requires a base 'color'")?;
                        value_color(color, "background")?
                    }
                    other => value_color(other, "background")?,
                };
            }
            "color" => {
                spec.specified.try_insert("color");
                spec.color = value_color(v, "color")?;
            }
            "fontSize" => {
                spec.specified.try_insert("fontSize");
                spec.font_size = v.number()? as f32;
            }
            "hoverBackground" | "pressedBackground" | "disabledBackground" | "borderColor"
            | "focusBorderColor" => {
                spec.specified.try_insert(k.as_str());
                spec.colors.set(k.as_str(), value_color(v, k.as_str())?);
            }
            "borderWidth" => {
                spec.specified.try_insert("borderWidth");
                spec.numbers.set("borderWidth", v.number()? as f32);
            }
            "transitionDuration" => {
                spec.specified.try_insert("transitionDuration");
                spec.numbers.set(
                    "transitionDuration",
                    parse_duration_value(v, "transitionDuration")?,
                );
            }
            "text" => {
                spec.specified.try_insert("text");
                spec.text = v.text()?;
            }
            "disabled" => {
                spec.specified.try_insert("disabled");
                spec.disabled = v.boolean()?;
            }
            _ => {}
        }
    }
    if let Some(a) = action {
        spec.specified.try_insert("clicked");
        spec.action = Some(a.to_owned());
    }
    Ok(spec)
}

fn parse_duration_value(value: &Value, name: &str) -> Result<f32, String> {
    let v = match value {
        Value::Number(n) => *n as f32,
        Value::Text(s) | Value::Expr(s) => {
            if let Some(raw) = s.strip_suffix("ms") {
                raw.parse::<f32>()
                    .map_err(|_| format!("'{name}' requires ms"))?
            } else {
                return Err(format!("'{name}' requires ms"));
            }
        }
        _ => return Err(format!("'{name}' requires ms")),
    };
    if !v.is_finite() || !(0. ..=2000.).contains(&v) {
        return Err(format!("'{name}' must be between 0ms and 2000ms"));
    }
    Ok(v)
}

fn optional_color_prop(props: &Properties, key: &str) -> Result<Option<[u8; 4]>, String> {
    match props.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => value_color(v, key).map(Some),
    }
}

fn friendly_scene_error(error: String) -> String {
    if error.contains("must be non-negative") {
        format!("A control does not fit inside its container: {error}")
    } else {
        error
    }
}

fn value_color(value: &Value, name: &str) -> Result<[u8; 4], String> {
    match value {
        Value::Text(s) | Value::Expr(s) => parse_hex_color(s, name),
        _ => Err(format!(
            "'{name}' requires an unquoted #RGB, #RRGGBB or #RRGGBBAA color"
        )),
    }
}

fn parse_hex_color(raw: &str, name: &str) -> Result<[u8; 4], String> {
    let Some(hex) = raw.strip_prefix('#') else {
        return Err(format!(
            "'{name}' requires an unquoted #RGB, #RRGGBB or #RRGGBBAA color"
        ));
    };
    let bytes = hex.as_bytes();
    let digit = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    match bytes.len() {
        3 => {
            let (Some(r), Some(g), Some(b)) = (digit(bytes[0]), digit(bytes[1]), digit(bytes[2]))
            else {
                return Err("Expected #RGB, #RRGGBB or #RRGGBBAA color".into());
            };
            Ok([r * 17, g * 17, b * 17, 255])
        }
        6 => {
            let (Some(r0), Some(r1), Some(g0), Some(g1), Some(b0), Some(b1)) = (
                digit(bytes[0]),
                digit(bytes[1]),
                digit(bytes[2]),
                digit(bytes[3]),
                digit(bytes[4]),
                digit(bytes[5]),
            ) else {
                return Err("Expected #RGB, #RRGGBB or #RRGGBBAA color".into());
            };
            Ok([(r0 << 4) | r1, (g0 << 4) | g1, (b0 << 4) | b1, 255])
        }
        8 => {
            let (Some(r0), Some(r1), Some(g0), Some(g1), Some(b0), Some(b1), Some(a0), Some(a1)) = (
                digit(bytes[0]),
                digit(bytes[1]),
                digit(bytes[2]),
                digit(bytes[3]),
                digit(bytes[4]),
                digit(bytes[5]),
                digit(bytes[6]),
                digit(bytes[7]),
            ) else {
                return Err("Expected #RGB, #RRGGBB or #RRGGBBAA color".into());
            };
            Ok([
                (r0 << 4) | r1,
                (g0 << 4) | g1,
                (b0 << 4) | b1,
                (a0 << 4) | a1,
            ])
        }
        _ => Err("Expected #RGB, #RRGGBB or #RRGGBBAA color".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Runtime;
    use crate::form_document::registry::InvalidationClass;
    use std::collections::BTreeMap;

    #[test]
    fn scene_ir_matches_scene_artifact_across_primitives_and_interactions() {
        let fancy_button = "component FancyButton { \
            width: 120; height: 34; label: 'Go'; background: #223344; \
            Rectangle { \
                radius: 8; \
                background: Brush { color: props.background; hover: #334455; pressed: #112233; }; \
                Border { width: 1; background: Brush { color: #556677; }; } \
                Reveal { width: 1; color: #ff880080; } \
                Text { text: props.label; color: #ffffff; fontSize: 13; } \
                PointerArea { clicked -> events.clicked(); } \
            } \
        }";
        let choice = "component Choice { \
            width: 80; height: 28; value: 'a'; checked: false; \
            Rectangle { \
                radius: 6; background: #1e293b; \
                Text { text: props.value; color: #ffffff; fontSize: 12; } \
                PointerArea { clicked -> events.clicked(); } \
            } \
        }";
        let icon = "component Icon { \
            width: 24; height: 24; source: ''; color: #ffffff; \
            Rectangle { \
                background: #00000000; \
                Image { width: 24; height: 24; source: props.source; color: props.color; } \
            } \
        }";
        let icon_svg = "<svg viewBox=\"0 0 24 24\"><circle cx=\"12\" cy=\"12\" r=\"9\" fill=\"currentColor\"/></svg>";
        let app = "component App { \
            state query: String = 'abc'; \
            state level: Number = 25; \
            state choice: String = 'a'; \
            Frame { \
                width: 360; height: 260; \
                Column { \
                    width: 360; height: 260; padding: 8; gap: 6; \
                    FancyButton { key: 'fancy'; label: 'Run'; clicked -> actions.run(); } \
                    TextInput { key: 'input'; width: 160; height: 40; value <-> state.query; placeholder: 'Type'; multiline: true; wrap: true; } \
                    Slider { key: 'slider'; width: 160; height: 24; min: 0; max: 100; step: 5; value <-> state.level; } \
                    Text { key: 'rich'; width: 160; height: 40; wrap: true; color: #ffffff; runs: [{ text: 'Hi ' }, { text: 'docs', color: #6fa8ff, link: 'docs' }]; } \
                    Icon { key: 'icon'; width: 24; height: 24; source: 'assets/icon.svg'; color: #38bdf8; } \
                    Choice { key: 'c_a'; value: 'a'; group <-> state.choice; } \
                    Choice { key: 'c_b'; value: 'b'; group <-> state.choice; } \
                    Column { width: 140; height: 32; clip: true; Text { key: 'clipped'; width: 200; height: 48; text: 'OverflowClip'; } } \
                    Scroll { width: 140; height: 32; Column { FancyButton { key: 'scrolled'; width: 120; height: 80; label: 'Inner'; } } } \
                } \
            } \
        }";
        let files = BTreeMap::from([
            (
                "components/FancyButton.ui".to_owned(),
                fancy_button.to_owned(),
            ),
            ("components/Choice.ui".to_owned(), choice.to_owned()),
            ("components/Icon.ui".to_owned(), icon.to_owned()),
            ("assets/icon.svg".to_owned(), icon_svg.to_owned()),
            ("App.ui".to_owned(), app.to_owned()),
        ]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let ir = doc.render_scene_ir().unwrap();
        let artifact = doc.render_scene_artifact().unwrap();

        let rt_ir = Runtime::from_scene_ir(&ir).unwrap();
        let rt_art = Runtime::from_scene_artifact(&artifact).unwrap();

        assert_eq!(rt_ir.width(), rt_art.width());
        assert_eq!(rt_ir.height(), rt_art.height());
        assert_eq!(rt_ir.render_width(), rt_art.render_width());
        assert_eq!(rt_ir.render_height(), rt_art.render_height());
        assert_eq!(rt_ir.control_count(), rt_art.control_count());
        for i in 0..rt_ir.control_count() {
            assert_eq!(
                rt_ir.control_bounds(i),
                rt_art.control_bounds(i),
                "bounds[{i}]"
            );
            assert_eq!(rt_ir.control_key(i), rt_art.control_key(i), "key[{i}]");
            assert_eq!(
                rt_ir.control_label(i),
                rt_art.control_label(i),
                "label[{i}]"
            );
            assert_eq!(rt_ir.text_value(i), rt_art.text_value(i), "text_value[{i}]");
            assert_eq!(
                rt_ir.range_value(i).to_bits(),
                rt_art.range_value(i).to_bits(),
                "range_value[{i}]"
            );
            assert_eq!(ir.controls[i].clip, artifact.controls[i].clip, "clip[{i}]");
            assert_eq!(
                ir.controls[i].scroll, artifact.controls[i].scroll,
                "scroll[{i}]"
            );
        }
        assert_eq!(rt_ir.accessibility_tree(), rt_art.accessibility_tree());
        assert_eq!(rt_ir.pixels(360, 260, 1.0), rt_art.pixels(360, 260, 1.0));
    }

    #[test]
    fn paint_only_state_update_bumps_visual_revision_without_layout_revision() {
        let app = "component App { \
            state title: String = 'One'; \
            state pad: Number = 8; \
            Column { \
                width: 300; height: 120; padding: state.pad; \
                Text { key: 't'; width: 120; height: 24; text: state.title; } \
            } \
        }";
        let files = BTreeMap::from([("App.ui".to_owned(), app.to_owned())]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let summary = doc.dependency_summary();
        let base = doc.states.first().cloned().unwrap_or_default();
        let mut paint_only = base.clone();
        paint_only.insert("title".into(), Value::Text("Two".into()));
        assert_eq!(
            summary.classify_state_change(&base, &paint_only),
            InvalidationClass::Paint
        );
        let mut layout_change = base;
        layout_change.insert("pad".into(), Value::Number(20.));
        assert_eq!(
            summary.classify_state_change(&doc.states[0], &layout_change),
            InvalidationClass::Layout
        );

        let mut rt = Runtime::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let vis0 = rt.visual_revision();
        let lay0 = rt.layout_revision();
        assert!(rt.patch_ui_state_json_native(r#"{"title":"Two"}"#).unwrap());
        let vis1 = rt.visual_revision();
        let lay1 = rt.layout_revision();
        assert!(vis1 > vis0, "paint change should advance visual_revision");
        assert_eq!(lay1, lay0, "paint-only change must keep layout_revision");

        assert!(rt.patch_ui_state_json_native(r#"{"pad":20}"#).unwrap());
        assert!(rt.visual_revision() > vis1);
        assert!(rt.layout_revision() > lay1);
    }

    #[test]
    fn scene_ir_renders_256_controls_with_shared_templates() {
        let mut items = Vec::with_capacity(256);
        for i in 0..256 {
            items.push(format!("{{ id: 'k{i}', title: 'Item {i}' }}"));
        }
        let app = format!(
            "component App {{ \
                width: 360; height: 800; \
                let items = [{}]; \
                Scroll {{ width: 360; height: 800; \
                    Column {{ \
                        for item in items key item.id {{ \
                            Button {{ key: item.id; width: 280; height: 32; text: item.title; }} \
                        }} \
                    }} \
                }} \
            }}",
            items.join(", ")
        );
        let files = BTreeMap::from([
            (
                "components/Button.ui".to_owned(),
                crate::BUTTON_COMPONENT.to_owned(),
            ),
            ("App.ui".to_owned(), app),
        ]);
        let doc = Document::from_ui_files(&files, "App.ui", &Properties::new()).unwrap();
        let start = std::time::Instant::now();
        let ir = doc.render_scene_ir().unwrap();
        let rt = Runtime::from_scene_ir(&ir).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(ir.templates.len(), 256);
        assert_eq!(rt.control_count(), 256);
        assert!(
            elapsed.as_secs_f64() < 0.5,
            "SceneIr build + load of 256 controls took {elapsed:?}"
        );
    }
}
