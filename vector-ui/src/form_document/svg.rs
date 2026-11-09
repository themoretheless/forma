use std::collections::BTreeMap;

use super::Image;

fn parse_svg_attrs(raw: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
        {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let key = &raw[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
            continue;
        }
        let quote = bytes[i];
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i <= bytes.len() {
            attrs.insert(key.to_string(), raw[val_start..i].trim().to_string());
        }
        i += 1;
    }
    attrs
}

fn svg_num(attrs: &BTreeMap<String, String>, key: &str, fallback: f64) -> Result<f64, String> {
    let Some(raw) = attrs.get(key) else {
        return Ok(fallback);
    };
    let v: f64 = raw
        .parse()
        .map_err(|_| format!("Invalid SVG number {key}={raw}"))?;
    if !v.is_finite() {
        return Err(format!("Invalid SVG number {key}={raw}"));
    }
    Ok(v)
}

fn parse_svg_points(raw: &str) -> Result<Vec<[f64; 2]>, String> {
    let values: Vec<f64> = raw
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<f64>()
                .map_err(|_| "Invalid SVG points".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() < 4 || values.len() % 2 != 0 || values.iter().any(|n| !n.is_finite()) {
        return Err("Invalid SVG points".into());
    }
    Ok(values.chunks_exact(2).map(|c| [c[0], c[1]]).collect())
}

fn svg_ring(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<[f64; 2]> {
    (0..24)
        .map(|i| {
            let a = (i as f64 / 24.0) * std::f64::consts::TAU;
            [cx + a.cos() * rx, cy + a.sin() * ry]
        })
        .collect()
}

fn svg_stroke_segment(a: [f64; 2], b: [f64; 2], width: f64) -> Option<Vec<[f64; 2]>> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = dx.hypot(dy);
    if len == 0.0 {
        return None;
    }
    let nx = (-dy / len) * width * 0.5;
    let ny = (dx / len) * width * 0.5;
    Some(vec![
        [a[0] + nx, a[1] + ny],
        [b[0] + nx, b[1] + ny],
        [b[0] - nx, b[1] - ny],
        [a[0] - nx, a[1] - ny],
    ])
}

pub(super) fn parse_svg_shapes(svg: &str) -> Result<Image, String> {
    let mut root_attrs: Option<BTreeMap<String, String>> = None;
    let mut shapes: Vec<(Vec<[f64; 2]>, String)> = Vec::new();
    let mut pos = 0;
    while let Some(open) = svg[pos..].find('<') {
        let tag_start = pos + open + 1;
        let Some(close) = svg[tag_start..].find('>') else {
            break;
        };
        let inside = svg[tag_start..tag_start + close].trim();
        pos = tag_start + close + 1;
        if inside.starts_with('/') || inside.starts_with('!') || inside.starts_with('?') {
            continue;
        }
        let inside = inside.strip_suffix('/').unwrap_or(inside).trim();
        let (tag, attr_str) = match inside.find(char::is_whitespace) {
            Some(idx) => (&inside[..idx], &inside[idx..]),
            None => (inside, ""),
        };
        if tag == "g" || tag == "defs" || tag == "title" || tag == "desc" {
            continue;
        }
        let attrs = parse_svg_attrs(attr_str);
        if tag == "svg" {
            root_attrs = Some(attrs);
            continue;
        }
        let root_ref = root_attrs.as_ref().ok_or("Expected <svg> root")?;
        let fill = attrs
            .get("fill")
            .or_else(|| root_ref.get("fill"))
            .map(String::as_str)
            .unwrap_or("currentColor");
        let stroke = attrs
            .get("stroke")
            .or_else(|| root_ref.get("stroke"))
            .map(String::as_str)
            .unwrap_or("none");
        let sw = svg_num(
            &attrs,
            "stroke-width",
            svg_num(root_ref, "stroke-width", 2.0)?,
        )?;
        let points = match tag {
            "rect" => {
                let x = svg_num(&attrs, "x", 0.0)?;
                let y = svg_num(&attrs, "y", 0.0)?;
                let w = svg_num(&attrs, "width", 0.0)?;
                let h = svg_num(&attrs, "height", 0.0)?;
                vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
            }
            "circle" => {
                let r = svg_num(&attrs, "r", 0.0)?;
                svg_ring(
                    svg_num(&attrs, "cx", 0.0)?,
                    svg_num(&attrs, "cy", 0.0)?,
                    r,
                    r,
                )
            }
            "ellipse" => svg_ring(
                svg_num(&attrs, "cx", 0.0)?,
                svg_num(&attrs, "cy", 0.0)?,
                svg_num(&attrs, "rx", 0.0)?,
                svg_num(&attrs, "ry", 0.0)?,
            ),
            "line" => vec![
                [svg_num(&attrs, "x1", 0.0)?, svg_num(&attrs, "y1", 0.0)?],
                [svg_num(&attrs, "x2", 0.0)?, svg_num(&attrs, "y2", 0.0)?],
            ],
            "polyline" | "polygon" => {
                parse_svg_points(attrs.get("points").map(String::as_str).unwrap_or(""))?
            }
            other => return Err(format!("Unsupported SVG element <{other}>")),
        };
        if tag != "line" && tag != "polyline" && fill != "none" {
            shapes.push((points.clone(), fill.to_string()));
        }
        if stroke != "none" && sw > 0.0 {
            let limit = if tag == "line" || tag == "polyline" {
                points.len().saturating_sub(1)
            } else {
                points.len()
            };
            for i in 0..limit {
                if let Some(quad) =
                    svg_stroke_segment(points[i], points[(i + 1) % points.len()], sw)
                {
                    shapes.push((quad, stroke.to_string()));
                }
            }
        }
    }
    let root = root_attrs.ok_or("Expected <svg> root")?;
    let view: Vec<f64> = root
        .get("viewBox")
        .map(|s| {
            s.split(|c: char| c.is_ascii_whitespace() || c == ',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse::<f64>().ok())
                .collect()
        })
        .unwrap_or_default();
    let width = if view.len() == 4 {
        view[2]
    } else {
        svg_num(&root, "width", 24.0)?
    };
    let height = if view.len() == 4 {
        view[3]
    } else {
        svg_num(&root, "height", 24.0)?
    };
    if width <= 0.0 || height <= 0.0 || shapes.is_empty() {
        return Err("SVG image has no drawable shapes".into());
    }
    Ok(Image {
        width,
        height,
        shapes,
    })
}
