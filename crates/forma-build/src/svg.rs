//! Bounded single-pass SVG parser preserving `currentColor` without sentinel color hacks.

use std::collections::BTreeMap;

const MAX_PATH_POINTS: usize = 4096;
const MAX_CURVE_STEPS: f64 = 128.0;
const QUANTUM: f64 = 1000.0;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgShape {
    pub color: String,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgImage {
    pub width: f64,
    pub height: f64,
    pub shapes: Vec<SvgShape>,
}

struct SubPath {
    points: Vec<[f64; 2]>,
    closed: bool,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Scanner<'_> {
    fn skip_separators(&mut self) {
        while self.at < self.bytes.len()
            && (self.bytes[self.at].is_ascii_whitespace() || self.bytes[self.at] == b',')
        {
            self.at += 1;
        }
    }

    fn command(&mut self) -> Option<u8> {
        self.skip_separators();
        let byte = *self.bytes.get(self.at)?;
        byte.is_ascii_alphabetic().then(|| {
            self.at += 1;
            byte
        })
    }

    fn has_number(&mut self) -> bool {
        self.skip_separators();
        self.bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Result<f64, String> {
        self.skip_separators();
        let start = self.at;
        let digits = |s: &mut Self| {
            let from = s.at;
            while s.bytes.get(s.at).is_some_and(u8::is_ascii_digit) {
                s.at += 1;
            }
            s.at - from
        };
        if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
            self.at += 1;
        }
        let mut mantissa = digits(self);
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            mantissa += digits(self);
        }
        if mantissa == 0 {
            return Err("SVG path: ожидалось число".into());
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'-' | b'+')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                self.at = mark;
            }
        }
        let value: f64 = std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| "SVG path: неверная координата".to_string())?;
        if !value.is_finite() || value.abs() > 100_000.0 {
            return Err("SVG path: неверная координата".into());
        }
        Ok(value)
    }

    fn flag(&mut self) -> Result<bool, String> {
        self.skip_separators();
        match self.bytes.get(self.at) {
            Some(b'0') => {
                self.at += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.at += 1;
                Ok(true)
            }
            _ => Err("SVG path: неверный флаг дуги".into()),
        }
    }
}

fn steps(deviation: f64, factor: f64, tolerance: f64) -> usize {
    (factor * deviation / tolerance)
        .sqrt()
        .ceil()
        .clamp(1.0, MAX_CURVE_STEPS) as usize
}

fn second_difference(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1])
}

fn cubic(out: &mut Vec<[f64; 2]>, p: [[f64; 2]; 4], tolerance: f64) {
    let deviation = second_difference(p[0], p[1], p[2]).max(second_difference(p[1], p[2], p[3]));
    let n = steps(deviation, 0.75, tolerance);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        out.push([
            w[0] * p[0][0] + w[1] * p[1][0] + w[2] * p[2][0] + w[3] * p[3][0],
            w[0] * p[0][1] + w[1] * p[1][1] + w[2] * p[2][1] + w[3] * p[3][1],
        ]);
    }
}

fn quadratic(out: &mut Vec<[f64; 2]>, p: [[f64; 2]; 3], tolerance: f64) {
    let n = steps(second_difference(p[0], p[1], p[2]), 0.25, tolerance);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let u = 1.0 - t;
        let w = [u * u, 2.0 * u * t, t * t];
        out.push([
            w[0] * p[0][0] + w[1] * p[1][0] + w[2] * p[2][0],
            w[0] * p[0][1] + w[1] * p[1][1] + w[2] * p[2][1],
        ]);
    }
}

#[allow(clippy::too_many_arguments)]
fn arc(
    out: &mut Vec<[f64; 2]>,
    from: [f64; 2],
    radii: [f64; 2],
    rotation_deg: f64,
    large: bool,
    sweep: bool,
    to: [f64; 2],
    tolerance: f64,
) {
    let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
    if from == to {
        return;
    }
    if rx == 0.0 || ry == 0.0 {
        out.push(to);
        return;
    }
    let (sin, cos) = rotation_deg.to_radians().sin_cos();
    let dx = (from[0] - to[0]) / 2.0;
    let dy = (from[1] - to[1]) / 2.0;
    let x1 = cos * dx + sin * dy;
    let y1 = -sin * dx + cos * dy;
    let scale = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if scale > 1.0 {
        rx *= scale.sqrt();
        ry *= scale.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut factor = (numerator / denominator).max(0.0).sqrt();
    if large == sweep {
        factor = -factor;
    }
    let cx1 = factor * rx * y1 / ry;
    let cy1 = -factor * ry * x1 / rx;
    let cx = cos * cx1 - sin * cy1 + (from[0] + to[0]) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from[1] + to[1]) / 2.0;
    let start = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
    let end = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx);
    let mut delta = end - start;
    if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    } else if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    }
    let radius = rx.max(ry);
    let step = 2.0 * (1.0 - (tolerance / radius).min(1.0)).acos();
    let n = if step > 0.0 {
        (delta.abs() / step).ceil().clamp(1.0, MAX_CURVE_STEPS) as usize
    } else {
        MAX_CURVE_STEPS as usize
    };
    for i in 1..n {
        let (s, c) = (start + delta * i as f64 / n as f64).sin_cos();
        out.push([
            cx + cos * rx * c - sin * ry * s,
            cy + sin * rx * c + cos * ry * s,
        ]);
    }
    out.push(to);
}

fn flatten_svg_path(d: &str, tolerance: f64) -> Result<Vec<SubPath>, String> {
    let mut scan = Scanner {
        bytes: d.as_bytes(),
        at: 0,
    };
    let mut subpaths: Vec<SubPath> = Vec::new();
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut started = false;
    let (mut current, mut start) = ([0.0f64; 2], [0.0f64; 2]);
    let (mut cubic_ctrl, mut quad_ctrl): (Option<[f64; 2]>, Option<[f64; 2]>) = (None, None);
    let mut command = 0u8;
    let flush = |points: &mut Vec<[f64; 2]>, closed: bool, subpaths: &mut Vec<SubPath>| {
        if points.len() > 1 {
            subpaths.push(SubPath {
                points: std::mem::take(points),
                closed,
            });
        } else {
            points.clear();
        }
    };
    loop {
        if let Some(next) = scan.command() {
            command = next;
        } else if !scan.has_number() {
            scan.skip_separators();
            if scan.at < scan.bytes.len() {
                return Err("SVG path: неверная команда".into());
            }
            break;
        } else if command == 0 || command.eq_ignore_ascii_case(&b'z') {
            return Err("SVG path: ожидалось M".into());
        }
        let relative = command.is_ascii_lowercase();
        let base = if relative { current } else { [0.0, 0.0] };
        let point = |scan: &mut Scanner| -> Result<[f64; 2], String> {
            Ok([base[0] + scan.number()?, base[1] + scan.number()?])
        };
        let (mut next_cubic, mut next_quad) = (None, None);
        match command.to_ascii_uppercase() {
            b'M' => {
                flush(&mut points, false, &mut subpaths);
                started = true;
                current = point(&mut scan)?;
                start = current;
                points.push(current);
                command = if relative { b'l' } else { b'L' };
            }
            _ if !started => return Err("SVG path: ожидалось M".into()),
            b'Z' => {
                flush(&mut points, true, &mut subpaths);
                current = start;
            }
            other => {
                if points.is_empty() {
                    points.push(current);
                }
                match other {
                    b'L' => current = point(&mut scan)?,
                    b'H' => current[0] = base[0] + scan.number()?,
                    b'V' => current[1] = base[1] + scan.number()?,
                    b'C' | b'S' => {
                        let first = if other == b'C' {
                            point(&mut scan)?
                        } else {
                            cubic_ctrl.map_or(current, |c| {
                                [2.0 * current[0] - c[0], 2.0 * current[1] - c[1]]
                            })
                        };
                        let second = point(&mut scan)?;
                        let end = point(&mut scan)?;
                        cubic(&mut points, [current, first, second, end], tolerance);
                        next_cubic = Some(second);
                        current = end;
                    }
                    b'Q' | b'T' => {
                        let control = if other == b'Q' {
                            point(&mut scan)?
                        } else {
                            quad_ctrl.map_or(current, |c| {
                                [2.0 * current[0] - c[0], 2.0 * current[1] - c[1]]
                            })
                        };
                        let end = point(&mut scan)?;
                        quadratic(&mut points, [current, control, end], tolerance);
                        next_quad = Some(control);
                        current = end;
                    }
                    b'A' => {
                        let radii = [scan.number()?, scan.number()?];
                        let rotation = scan.number()?;
                        let (large, sweep) = (scan.flag()?, scan.flag()?);
                        let end = point(&mut scan)?;
                        arc(
                            &mut points,
                            current,
                            radii,
                            rotation,
                            large,
                            sweep,
                            end,
                            tolerance,
                        );
                        current = end;
                    }
                    _ => {
                        return Err(format!(
                            "SVG path: команда {} не поддержана",
                            command as char
                        ));
                    }
                }
                if matches!(other, b'L' | b'H' | b'V') {
                    points.push(current);
                }
            }
        }
        (cubic_ctrl, quad_ctrl) = (next_cubic, next_quad);
        let total = subpaths.iter().map(|s| s.points.len()).sum::<usize>() + points.len();
        if total > MAX_PATH_POINTS {
            return Err("SVG path: слишком длинный путь".into());
        }
    }
    flush(&mut points, false, &mut subpaths);
    if subpaths.is_empty() {
        return Err("SVG path: нет отрезков".into());
    }
    Ok(subpaths)
}

fn overlapping_contours(subpaths: &[SubPath]) -> bool {
    let bounds: Vec<[f64; 4]> = subpaths
        .iter()
        .filter(|sub| sub.points.len() > 2)
        .map(|sub| {
            sub.points.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |b, p| {
                    [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ]
                },
            )
        })
        .collect();
    bounds.iter().enumerate().any(|(i, a)| {
        bounds[i + 1..]
            .iter()
            .any(|b| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3])
    })
}

fn is_valid_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn resolve_ink<'a>(raw: &'a str, current_color: &'a str) -> Result<Option<&'a str>, String> {
    if raw == "none" {
        return Ok(None);
    }
    let resolved = if raw == "currentColor" {
        current_color
    } else {
        raw
    };
    if resolved == "currentColor" || is_valid_hex_color(resolved) {
        return Ok(Some(resolved));
    }
    Err("SVG: поддерживаются hex и currentColor".into())
}

fn strip_pattern(input: &str, start_pat: &str, end_pat: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find(start_pat) {
        out.push_str(&rest[..start]);
        let after = &rest[start + start_pat.len()..];
        let Some(end) = after.find(end_pat) else {
            return input.to_string();
        };
        rest = &after[end + end_pat.len()..];
    }
    out.push_str(rest);
    out
}

fn parse_svg_attributes(tail: &str) -> Result<BTreeMap<String, String>, String> {
    let mut attrs = BTreeMap::new();
    let bytes = tail.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let key_start = pos;
        while pos < bytes.len()
            && (bytes[pos].is_ascii_alphanumeric() || matches!(bytes[pos], b'_' | b':' | b'-'))
        {
            pos += 1;
        }
        if pos == key_start || !bytes[pos..].contains(&b'=') {
            return Err("SVG: некорректные атрибуты".into());
        }
        let key = &tail[key_start..pos];
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if bytes.get(pos) != Some(&b'=') {
            return Err("SVG: некорректные атрибуты".into());
        }
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let Some(&quote) = bytes.get(pos).filter(|&&q| q == b'\'' || q == b'"') else {
            return Err("SVG: некорректные атрибуты".into());
        };
        pos += 1;
        let val_start = pos;
        while pos < bytes.len() && bytes[pos] != quote {
            pos += 1;
        }
        if pos >= bytes.len() {
            return Err("SVG: некорректные атрибуты".into());
        }
        let value = &tail[val_start..pos];
        pos += 1;
        if attrs.insert(key.to_string(), value.to_string()).is_some() {
            return Err("SVG: повторный атрибут".into());
        }
    }
    Ok(attrs)
}

fn allowed_attrs_for(tag: &str) -> Option<&'static [&'static str]> {
    Some(match tag {
        "svg" => &["xmlns", "viewBox", "width", "height"],
        "rect" => &[
            "fill",
            "stroke",
            "stroke-width",
            "x",
            "y",
            "width",
            "height",
        ],
        "circle" => &["fill", "stroke", "stroke-width", "cx", "cy", "r"],
        "ellipse" => &["fill", "stroke", "stroke-width", "cx", "cy", "rx", "ry"],
        "line" => &["fill", "stroke", "stroke-width", "x1", "y1", "x2", "y2"],
        "polyline" | "polygon" => &["fill", "stroke", "stroke-width", "points"],
        "path" => &["fill", "stroke", "stroke-width", "d"],
        _ => return None,
    })
}

fn svg_number(raw: Option<&str>, fallback: f64) -> Result<f64, String> {
    let Some(raw) = raw else {
        return Ok(fallback);
    };
    let n: f64 = raw
        .trim()
        .parse()
        .map_err(|_| "SVG: неверная координата".to_string())?;
    if !n.is_finite() || n.abs() > 100_000.0 {
        return Err("SVG: неверная координата".into());
    }
    Ok(n)
}

fn parse_view_box(attrs: &BTreeMap<String, String>) -> Result<[f64; 4], String> {
    if let Some(vb) = attrs.get("viewBox") {
        let nums: Vec<f64> = vb
            .trim()
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| svg_number(Some(s), 0.0))
            .collect::<Result<Vec<_>, _>>()?;
        if nums.len() != 4 || nums[2] <= 0.0 || nums[3] <= 0.0 {
            return Err("SVG требует положительный viewBox".into());
        }
        return Ok([nums[0], nums[1], nums[2], nums[3]]);
    }
    let w = svg_number(attrs.get("width").map(String::as_str), 0.0)?;
    let h = svg_number(attrs.get("height").map(String::as_str), 0.0)?;
    if w <= 0.0 || h <= 0.0 {
        return Err("SVG требует положительный viewBox".into());
    }
    Ok([0.0, 0.0, w, h])
}

fn push_stroke_segment(out: &mut Vec<SvgShape>, a: [f64; 2], b: [f64; 2], width: f64, color: &str) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = dx.hypot(dy);
    if len == 0.0 {
        return;
    }
    let nx = (-dy / len) * width * 0.5;
    let ny = (dx / len) * width * 0.5;
    out.push(SvgShape {
        color: color.to_string(),
        points: vec![
            [a[0] + nx, a[1] + ny],
            [b[0] + nx, b[1] + ny],
            [b[0] - nx, b[1] - ny],
            [a[0] - nx, a[1] - ny],
        ],
    });
}

fn unit_circle_point(index: usize) -> [f64; 2] {
    let angle = (index as f64) * std::f64::consts::PI / 32.0;
    [angle.cos(), angle.sin()]
}

pub(crate) fn parse_svg_image(source: &str) -> Result<SvgImage, String> {
    if source.len() > 100_000 {
        return Err("SVG должен быть текстом до 100 КБ".into());
    }
    let cleaned = strip_pattern(&strip_pattern(source, "<!--", "-->"), "<?xml", "?>");
    let mut tags = Vec::new();
    let mut outside = String::new();
    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('<') {
        outside.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('>') else {
            return Err("SVG: незакрытый тег".into());
        };
        tags.push(&after[..close]);
        rest = &after[close + 1..];
    }
    outside.push_str(rest);
    if !outside.trim().is_empty() {
        return Err("SVG: текстовые элементы пока не поддерживаются".into());
    }

    let mut out = Vec::new();
    let mut view: Option<[f64; 4]> = None;
    let mut root_attrs: Option<BTreeMap<String, String>> = None;
    let mut closed = false;

    for raw in tags {
        let raw = raw.trim();
        if raw == "/svg" {
            closed = true;
            continue;
        }
        if closed {
            return Err("SVG: лишние элементы после корня".into());
        }
        let name_len = raw.bytes().take_while(u8::is_ascii_alphabetic).count();
        let name = &raw[..name_len];
        let Some(allowed) = allowed_attrs_for(name) else {
            return Err(format!(
                "SVG {}: пока поддерживаются rect, circle, ellipse, line, polyline, polygon, path",
                if name.is_empty() { raw } else { name }
            ));
        };
        let self_closing = raw.ends_with('/');
        let tail = raw[name_len..]
            .strip_suffix('/')
            .unwrap_or(&raw[name_len..]);
        let attrs = parse_svg_attributes(tail)?;
        for key in attrs.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(format!("SVG: атрибут {key} пока не поддерживается"));
            }
        }
        if name == "svg" {
            if view.is_some() {
                return Err("SVG: один корень".into());
            }
            view = Some(parse_view_box(&attrs)?);
            root_attrs = Some(attrs);
            continue;
        }
        let Some(vb) = view else {
            return Err("SVG: примитивы внутри svg должны быть самозакрывающимися".into());
        };
        if !self_closing {
            return Err("SVG: примитивы внутри svg должны быть самозакрывающимися".into());
        }
        let fill = resolve_ink(
            attrs.get("fill").map(String::as_str).unwrap_or("#000000"),
            "currentColor",
        )?;
        let stroke = resolve_ink(
            attrs.get("stroke").map(String::as_str).unwrap_or("none"),
            "currentColor",
        )?;
        let sw = svg_number(attrs.get("stroke-width").map(String::as_str), 1.0)?;
        if sw < 0.0 {
            return Err("SVG: отрицательная толщина".into());
        }
        if name == "path" {
            let d = attrs
                .get("d")
                .map(String::as_str)
                .ok_or("SVG path: нет данных d")?;
            let tolerance = (vb[2].max(vb[3]) / 2048.0).max(1e-6);
            let subpaths = flatten_svg_path(d, tolerance)?;
            if let Some(fill_color) = fill {
                if overlapping_contours(&subpaths) {
                    return Err(
                        "SVG path: вложенные и пересекающиеся контуры (отверстия) пока не поддерживаются"
                            .into(),
                    );
                }
                for sub in &subpaths {
                    if sub.points.len() > 2 {
                        out.push(SvgShape {
                            color: fill_color.to_string(),
                            points: sub.points.clone(),
                        });
                    }
                }
            }
            if let Some(stroke_color) = stroke
                && sw != 0.0
            {
                for sub in &subpaths {
                    let count = sub.points.len();
                    let limit = if sub.closed {
                        count
                    } else {
                        count.saturating_sub(1)
                    };
                    for i in 0..limit {
                        push_stroke_segment(
                            &mut out,
                            sub.points[i],
                            sub.points[(i + 1) % count],
                            sw,
                            stroke_color,
                        );
                    }
                }
            }
            if out.len() > 2048 {
                return Err("SVG: слишком много примитивов".into());
            }
            continue;
        }

        let (points, close) = match name {
            "rect" => {
                let x = svg_number(attrs.get("x").map(String::as_str), 0.0)?;
                let y = svg_number(attrs.get("y").map(String::as_str), 0.0)?;
                let w = svg_number(attrs.get("width").map(String::as_str), 0.0)?;
                let h = svg_number(attrs.get("height").map(String::as_str), 0.0)?;
                if w < 0.0 || h < 0.0 {
                    return Err("SVG: отрицательный размер".into());
                }
                (vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true)
            }
            "circle" | "ellipse" => {
                let x = svg_number(attrs.get("cx").map(String::as_str), 0.0)?;
                let y = svg_number(attrs.get("cy").map(String::as_str), 0.0)?;
                let rx = svg_number(
                    attrs
                        .get("rx")
                        .or_else(|| attrs.get("r"))
                        .map(String::as_str),
                    0.0,
                )?;
                let ry = svg_number(
                    attrs
                        .get("ry")
                        .or_else(|| attrs.get("r"))
                        .map(String::as_str),
                    0.0,
                )?;
                if rx < 0.0 || ry < 0.0 {
                    return Err("SVG: отрицательный радиус".into());
                }
                let pts = (0..64)
                    .map(|i| {
                        let [cx, sy] = unit_circle_point(i);
                        [x + rx * cx, y + ry * sy]
                    })
                    .collect();
                (pts, true)
            }
            "line" => (
                vec![
                    [
                        svg_number(attrs.get("x1").map(String::as_str), 0.0)?,
                        svg_number(attrs.get("y1").map(String::as_str), 0.0)?,
                    ],
                    [
                        svg_number(attrs.get("x2").map(String::as_str), 0.0)?,
                        svg_number(attrs.get("y2").map(String::as_str), 0.0)?,
                    ],
                ],
                false,
            ),
            "polyline" | "polygon" => {
                let coords: Vec<f64> = attrs
                    .get("points")
                    .map(|s| {
                        s.trim()
                            .split(|c: char| c.is_ascii_whitespace() || c == ',')
                            .filter(|part| !part.is_empty())
                            .map(|part| svg_number(Some(part), 0.0))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if coords.len() < 4 || !coords.len().is_multiple_of(2) || coords.len() > 2048 {
                    return Err("SVG: неверный points".into());
                }
                let pts = coords.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
                (pts, name == "polygon")
            }
            _ => unreachable!(),
        };

        if name != "line"
            && let Some(fill_color) = fill
        {
            out.push(SvgShape {
                color: fill_color.to_string(),
                points: points.clone(),
            });
        }
        if let Some(stroke_color) = stroke
            && sw != 0.0
        {
            let limit = if close {
                points.len()
            } else {
                points.len().saturating_sub(1)
            };
            for i in 0..limit {
                push_stroke_segment(
                    &mut out,
                    points[i],
                    points[(i + 1) % points.len()],
                    sw,
                    stroke_color,
                );
            }
        }
        if out.len() > 2048 {
            return Err("SVG: слишком много примитивов".into());
        }
    }

    let Some(vb) = view else {
        return Err("SVG: незакрытый корень".into());
    };
    if !closed {
        return Err("SVG: незакрытый корень".into());
    }
    let attrs = root_attrs
        .as_ref()
        .expect("root attrs exist when view exists");
    let width = if attrs.contains_key("viewBox") {
        vb[2]
    } else {
        svg_number(attrs.get("width").map(String::as_str), vb[2])?
    };
    let height = if attrs.contains_key("viewBox") {
        vb[3]
    } else {
        svg_number(attrs.get("height").map(String::as_str), vb[3])?
    };

    let scale = (width / vb[2]).min(height / vb[3]);
    let ox = (width - vb[2] * scale) * 0.5;
    let oy = (height - vb[3] * scale) * 0.5;
    let quantize = |val: f64| {
        let q = (val * QUANTUM).round() / QUANTUM;
        if q == 0.0 { 0.0 } else { q }
    };
    let placed = out
        .into_iter()
        .map(|shape| SvgShape {
            color: shape.color,
            points: shape
                .points
                .into_iter()
                .map(|[a, b]| {
                    [
                        quantize(ox + (a - vb[0]) * scale),
                        quantize(oy + (b - vb[1]) * scale),
                    ]
                })
                .collect(),
        })
        .collect();

    Ok(SvgImage {
        width,
        height,
        shapes: placed,
    })
}
