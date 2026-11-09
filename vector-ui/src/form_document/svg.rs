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
        .map(|s| s.parse::<f64>().map_err(|_| "Invalid SVG points".to_string()))
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

// Limits of the control template that receives the shapes (`ContentShape`
// points and content primitives per control), checked here so an oversized
// path is refused with an SVG diagnostic instead of a template one.
const MAX_SHAPE_POINTS: usize = 4096;
const MAX_IMAGE_SHAPES: usize = 4096;
const MAX_COORDINATE: f64 = 100_000.0;
// Bounds the nesting analysis of one path, which compares every pair of subpaths.
const MAX_PATH_POINTS: usize = 65_536;
// Curves are flattened once, before the icon is placed: the tolerance is this
// fixed fraction of the larger image side, about 0.05 px for a 24 px icon.
const PATH_FLATNESS: f64 = 1.0 / 512.0;
const MAX_CURVE_SEGMENTS: f64 = 256.0;

struct Subpath {
    points: Vec<[f64; 2]>,
    closed: bool,
}

struct PathReader<'a> {
    data: &'a str,
    pos: usize,
}

impl PathReader<'_> {
    fn error(&self) -> String {
        format!("Invalid SVG path data at byte {}", self.pos)
    }
    fn skip_separators(&mut self) {
        let bytes = self.data.as_bytes();
        while self.pos < bytes.len() && (bytes[self.pos].is_ascii_whitespace() || bytes[self.pos] == b',') {
            self.pos += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip_separators();
        self.data.as_bytes().get(self.pos).copied()
    }
    fn at_number(&mut self) -> bool {
        self.peek().is_some_and(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.'))
    }
    // Path numbers need no separator: "1.5.5" is two numbers and so is "1-2".
    fn number(&mut self) -> Result<f64, String> {
        self.skip_separators();
        let bytes = self.data.as_bytes();
        let start = self.pos;
        let mut i = start;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        let digits = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if !bytes[digits..i].iter().any(u8::is_ascii_digit) {
            return Err(self.error());
        }
        if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
            let mut j = i + 1;
            if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_digit() {
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
            }
        }
        let value = self.data[start..i].parse::<f64>().map_err(|_| self.error())?;
        if !value.is_finite() || value.abs() > MAX_COORDINATE {
            return Err(self.error());
        }
        self.pos = i;
        Ok(value)
    }
    // Arc flags are single digits and may be written without separators.
    fn flag(&mut self) -> Result<bool, String> {
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.pos += 1;
                Ok(true)
            }
            _ => Err(self.error()),
        }
    }
    fn pair(&mut self, relative: bool, origin: [f64; 2]) -> Result<[f64; 2], String> {
        let x = self.number()?;
        let y = self.number()?;
        Ok(if relative { [origin[0] + x, origin[1] + y] } else { [x, y] })
    }
}

fn curve_segments(deviation: f64, tolerance: f64) -> usize {
    (deviation / tolerance).sqrt().ceil().clamp(1.0, MAX_CURVE_SEGMENTS) as usize
}

fn flatten_cubic(out: &mut Vec<[f64; 2]>, p: [[f64; 2]; 4], tolerance: f64) {
    // Wang's bound: the second differences limit how far a chord can stray.
    let bend = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1]);
    let segments = curve_segments(0.75 * bend(p[0], p[1], p[2]).max(bend(p[1], p[2], p[3])), tolerance);
    for i in 1..segments {
        let t = i as f64 / segments as f64;
        let u = 1.0 - t;
        let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        out.push([
            w[0] * p[0][0] + w[1] * p[1][0] + w[2] * p[2][0] + w[3] * p[3][0],
            w[0] * p[0][1] + w[1] * p[1][1] + w[2] * p[2][1] + w[3] * p[3][1],
        ]);
    }
    out.push(p[3]);
}

fn flatten_quadratic(out: &mut Vec<[f64; 2]>, p: [[f64; 2]; 3], tolerance: f64) {
    let bend = (p[0][0] - 2.0 * p[1][0] + p[2][0]).hypot(p[0][1] - 2.0 * p[1][1] + p[2][1]);
    let segments = curve_segments(0.25 * bend, tolerance);
    for i in 1..segments {
        let t = i as f64 / segments as f64;
        let u = 1.0 - t;
        let w = [u * u, 2.0 * u * t, t * t];
        out.push([
            w[0] * p[0][0] + w[1] * p[1][0] + w[2] * p[2][0],
            w[0] * p[0][1] + w[1] * p[1][1] + w[2] * p[2][1],
        ]);
    }
    out.push(p[2]);
}

// Endpoint to center conversion of the SVG implementation notes (F.6.5/F.6.6).
#[allow(clippy::too_many_arguments)]
fn flatten_arc(out: &mut Vec<[f64; 2]>, from: [f64; 2], radii: [f64; 2], rotation: f64, large: bool, sweep: bool, to: [f64; 2], tolerance: f64) {
    let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
    if from == to {
        return;
    }
    if rx == 0.0 || ry == 0.0 {
        out.push(to);
        return;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (dx, dy) = ((from[0] - to[0]) / 2.0, (from[1] - to[1]) / 2.0);
    let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let scale = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if scale > 1.0 {
        rx *= scale.sqrt();
        ry *= scale.sqrt();
    }
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let numerator = rx * rx * ry * ry - denominator;
    let mut factor = (numerator / denominator).max(0.0).sqrt();
    if large == sweep {
        factor = -factor;
    }
    let (cx1, cy1) = (factor * rx * y1 / ry, -factor * ry * x1 / rx);
    let cx = cos * cx1 - sin * cy1 + (from[0] + to[0]) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from[1] + to[1]) / 2.0;
    let start = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
    let mut delta = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx) - start;
    if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    } else if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    }
    let radius = rx.max(ry);
    let step = if tolerance < radius { 2.0 * (1.0 - tolerance / radius).acos() } else { std::f64::consts::PI };
    let segments = (delta.abs() / step).ceil().clamp(1.0, MAX_CURVE_SEGMENTS) as usize;
    for i in 1..segments {
        let (s, c) = (start + delta * i as f64 / segments as f64).sin_cos();
        out.push([cx + rx * c * cos - ry * s * sin, cy + rx * c * sin + ry * s * cos]);
    }
    out.push(to);
}

/// Flattens path data (`M L H V C S Q T A Z`, absolute and relative) into subpaths.
fn parse_svg_path(data: &str, tolerance: f64) -> Result<Vec<Subpath>, String> {
    let mut reader = PathReader { data, pos: 0 };
    let mut subpaths: Vec<Subpath> = Vec::new();
    let mut points: Vec<[f64; 2]> = Vec::new();
    let (mut current, mut start) = ([0.0f64; 2], [0.0f64; 2]);
    // Control point that a following smooth command reflects, with its curve degree.
    let mut control: Option<(u8, [f64; 2])> = None;
    let mut command = 0u8;
    while let Some(next) = reader.peek() {
        if next.is_ascii_alphabetic() {
            reader.pos += 1;
            command = next;
        } else if command == 0 || command.eq_ignore_ascii_case(&b'z') || !reader.at_number() {
            return Err(reader.error());
        }
        if subpaths.is_empty() && points.is_empty() && !command.eq_ignore_ascii_case(&b'm') {
            return Err("SVG path data must start with a moveto".into());
        }
        let relative = command.is_ascii_lowercase();
        let from = current;
        let mut smooth = None;
        match command.to_ascii_uppercase() {
            b'M' => {
                let point = reader.pair(relative, current)?;
                if !points.is_empty() {
                    subpaths.push(Subpath { points: std::mem::take(&mut points), closed: false });
                }
                points.push(point);
                (current, start) = (point, point);
                // Further coordinate pairs after a moveto are linetos.
                command = if relative { b'l' } else { b'L' };
            }
            b'Z' => {
                if !points.is_empty() {
                    subpaths.push(Subpath { points: std::mem::take(&mut points), closed: true });
                }
                current = start;
            }
            other => {
                // Drawing after a closepath continues from the subpath's start.
                if points.is_empty() {
                    points.push(current);
                }
                match other {
                    b'L' => current = reader.pair(relative, from)?,
                    b'H' => current[0] = reader.number()? + if relative { from[0] } else { 0.0 },
                    b'V' => current[1] = reader.number()? + if relative { from[1] } else { 0.0 },
                    b'C' | b'S' => {
                        let first = if other == b'C' {
                            reader.pair(relative, from)?
                        } else {
                            match control {
                                Some((3, c)) => [2.0 * from[0] - c[0], 2.0 * from[1] - c[1]],
                                _ => from,
                            }
                        };
                        let second = reader.pair(relative, from)?;
                        current = reader.pair(relative, from)?;
                        flatten_cubic(&mut points, [from, first, second, current], tolerance);
                        smooth = Some((3, second));
                    }
                    b'Q' | b'T' => {
                        let middle = if other == b'Q' {
                            reader.pair(relative, from)?
                        } else {
                            match control {
                                Some((2, c)) => [2.0 * from[0] - c[0], 2.0 * from[1] - c[1]],
                                _ => from,
                            }
                        };
                        current = reader.pair(relative, from)?;
                        flatten_quadratic(&mut points, [from, middle, current], tolerance);
                        smooth = Some((2, middle));
                    }
                    b'A' => {
                        let radii = [reader.number()?, reader.number()?];
                        let rotation = reader.number()?;
                        let (large, sweep) = (reader.flag()?, reader.flag()?);
                        current = reader.pair(relative, from)?;
                        flatten_arc(&mut points, from, radii, rotation, large, sweep, current, tolerance);
                    }
                    _ => {
                        reader.pos -= 1;
                        return Err(reader.error());
                    }
                }
                if smooth.is_none() && other != b'A' {
                    points.push(current);
                }
            }
        }
        control = smooth;
        if points.len() > MAX_SHAPE_POINTS {
            return Err(format!("SVG path exceeds {MAX_SHAPE_POINTS} points in one subpath"));
        }
    }
    if !points.is_empty() {
        subpaths.push(Subpath { points, closed: false });
    }
    // Relative steps and arcs can leave the range that single numbers respect.
    let mut total = 0usize;
    for subpath in &subpaths {
        total += subpath.points.len();
        if total > MAX_PATH_POINTS {
            return Err(format!("SVG path exceeds {MAX_PATH_POINTS} points"));
        }
        if subpath.points.iter().flatten().any(|n| !n.is_finite() || n.abs() > MAX_COORDINATE) {
            return Err("SVG path coordinate out of range".into());
        }
    }
    Ok(subpaths)
}

fn ring_area(points: &[[f64; 2]]) -> f64 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn ring_contains(ring: &[[f64; 2]], point: [f64; 2]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

/// Turns the filled area of a path into polygons the renderer can take one at
/// a time. A shape is a single ring, so a hole is joined to its outer ring by
/// a bridge walked in both directions. Every ring ends up wound once, which
/// keeps nonzero and even-odd coverage, and so the CPU and GPU, in agreement.
fn path_fill_rings(subpaths: &[Subpath], even_odd: bool) -> Result<Vec<Vec<[f64; 2]>>, String> {
    let mut rings: Vec<(Vec<[f64; 2]>, f64)> = Vec::new();
    for subpath in subpaths {
        let mut ring = subpath.points.clone();
        ring.dedup();
        while ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        // A figure of eight may enclose no signed area and still has two lobes to fill.
        if ring.len() >= 3 {
            let sign = if ring_area(&ring) < 0.0 { -1.0 } else { 1.0 };
            rings.push((ring, sign));
        }
    }
    if rings.len() > MAX_IMAGE_SHAPES {
        return Err(format!("SVG path exceeds {MAX_IMAGE_SHAPES} primitives"));
    }
    if rings.len() == 1 {
        return Ok(rings.into_iter().map(|(ring, _)| ring).collect());
    }
    // Icon subpaths nest without crossing, so one point tells what encloses a ring.
    // It is taken just inside the ring: a vertex may touch a neighbouring subpath.
    let inside: Vec<[f64; 2]> = rings
        .iter()
        .map(|(ring, sign)| {
            let (a, b) = (ring[0], ring[1]);
            let lean = sign * 1e-6;
            [(a[0] + b[0]) / 2.0 - (b[1] - a[1]) * lean, (a[1] + b[1]) / 2.0 + (b[0] - a[0]) * lean]
        })
        .collect();
    let enclosing: Vec<Vec<usize>> = (0..rings.len())
        .map(|i| (0..rings.len()).filter(|&j| j != i && ring_contains(&rings[j].0, inside[i])).collect())
        .collect();
    let filled = |winding: f64, depth: usize| if even_odd { depth % 2 == 1 } else { winding != 0.0 };
    // (ring, holes) for a ring that starts filled area; holes name their rings.
    let mut outer: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut holes = Vec::new();
    for i in 0..rings.len() {
        let around: f64 = enclosing[i].iter().map(|&j| rings[j].1).sum();
        let (outside, inside) = (filled(around, enclosing[i].len()), filled(around + rings[i].1, enclosing[i].len() + 1));
        if inside && !outside {
            outer.push((i, Vec::new()));
        } else if outside && !inside {
            holes.push(i);
        }
    }
    for hole in holes {
        // The innermost filled ring around a hole is the one it cuts.
        if let Some(owner) = outer
            .iter_mut()
            .filter(|(ring, _)| enclosing[hole].contains(ring))
            .max_by_key(|(ring, _)| enclosing[*ring].len())
        {
            owner.1.push(hole);
        }
    }
    let mut result = Vec::with_capacity(outer.len());
    for (ring, holes) in outer {
        let mut polygon = rings[ring].0.clone();
        if rings[ring].1 < 0.0 {
            polygon.reverse();
        }
        for hole in holes {
            let mut inner = rings[hole].0.clone();
            if rings[hole].1 > 0.0 {
                inner.reverse();
            }
            if polygon.len() + inner.len() + 2 > MAX_SHAPE_POINTS {
                return Err(format!("SVG path exceeds {MAX_SHAPE_POINTS} points in one shape"));
            }
            let (mut at, mut from, mut best) = (0, 0, f64::INFINITY);
            for (i, a) in polygon.iter().enumerate() {
                for (j, b) in inner.iter().enumerate() {
                    let distance = (a[0] - b[0]).hypot(a[1] - b[1]);
                    if distance < best {
                        (at, from, best) = (i, j, distance);
                    }
                }
            }
            inner.rotate_left(from);
            inner.push(inner[0]);
            inner.push(polygon[at]);
            polygon.splice(at + 1..at + 1, inner);
        }
        result.push(polygon);
    }
    Ok(result)
}

fn svg_image_size(root: &BTreeMap<String, String>) -> Result<(f64, f64), String> {
    let view: Vec<f64> = root
        .get("viewBox")
        .map(|s| {
            s.split(|c: char| c.is_ascii_whitespace() || c == ',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse::<f64>().ok())
                .collect()
        })
        .unwrap_or_default();
    if view.len() == 4 {
        Ok((view[2], view[3]))
    } else {
        Ok((svg_num(root, "width", 24.0)?, svg_num(root, "height", 24.0)?))
    }
}

fn push_svg_path(
    shapes: &mut Vec<(Vec<[f64; 2]>, String)>,
    attrs: &BTreeMap<String, String>,
    root: &BTreeMap<String, String>,
    fill: &str,
    stroke: &str,
    stroke_width: f64,
) -> Result<(), String> {
    let (width, height) = svg_image_size(root)?;
    let side = width.max(height);
    let tolerance = if side.is_finite() && side > 0.0 { side } else { 24.0 } * PATH_FLATNESS;
    let subpaths = parse_svg_path(attrs.get("d").map(String::as_str).unwrap_or(""), tolerance)?;
    if subpaths.is_empty() {
        return Err("SVG path has no segments".into());
    }
    if fill != "none" {
        let even_odd = attrs.get("fill-rule").or_else(|| root.get("fill-rule")).is_some_and(|rule| rule == "evenodd");
        for ring in path_fill_rings(&subpaths, even_odd)? {
            shapes.push((ring, fill.to_string()));
        }
    }
    let too_many = || format!("SVG path exceeds {MAX_IMAGE_SHAPES} primitives");
    if shapes.len() > MAX_IMAGE_SHAPES {
        return Err(too_many());
    }
    // Stroked like the other elements: one quad per segment, without joins or caps.
    if stroke != "none" && stroke_width > 0.0 {
        for subpath in &subpaths {
            let points = &subpath.points;
            let segments = if subpath.closed { points.len() } else { points.len() - 1 };
            if shapes.len() + segments > MAX_IMAGE_SHAPES {
                return Err(too_many());
            }
            for i in 0..segments {
                if let Some(quad) = svg_stroke_segment(points[i], points[(i + 1) % points.len()], stroke_width) {
                    shapes.push((quad, stroke.to_string()));
                }
            }
        }
    }
    Ok(())
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
        if tag == "path" {
            push_svg_path(&mut shapes, &attrs, root_ref, fill, stroke, sw)?;
            continue;
        }
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
                svg_ring(svg_num(&attrs, "cx", 0.0)?, svg_num(&attrs, "cy", 0.0)?, r, r)
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
                if let Some(quad) = svg_stroke_segment(points[i], points[(i + 1) % points.len()], sw)
                {
                    shapes.push((quad, stroke.to_string()));
                }
            }
        }
    }
    let root = root_attrs.ok_or("Expected <svg> root")?;
    let (width, height) = svg_image_size(&root)?;
    if width <= 0.0 || height <= 0.0 || shapes.is_empty() {
        return Err("SVG image has no drawable shapes".into());
    }
    Ok(Image {
        width,
        height,
        shapes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(body: &str) -> Result<Image, String> {
        parse_svg_shapes(&format!("<svg viewBox=\"0 0 24 24\">{body}</svg>"))
    }
    fn fill(d: &str) -> Vec<Vec<[f64; 2]>> {
        image(&format!("<path d=\"{d}\"/>")).unwrap_or_else(|e| panic!("{d}: {e}")).shapes.into_iter().map(|(points, _)| points).collect()
    }
    fn area(points: &[[f64; 2]]) -> f64 {
        ring_area(points).abs()
    }
    fn close(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9)
    }
    const TOLERANCE: f64 = 24.0 * PATH_FLATNESS;
    // Largest distance from a chord midpoint to the curve it replaces.
    fn chord_error(points: &[[f64; 2]], distance: impl Fn([f64; 2]) -> f64) -> f64 {
        points.windows(2).map(|w| distance([(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0])).fold(0.0, f64::max)
    }

    #[test]
    fn path_lines_in_absolute_and_relative_form() {
        let square = [[2.0, 2.0], [10.0, 2.0], [10.0, 10.0], [2.0, 10.0]];
        for d in [
            "M2 2 L10 2 L10 10 L2 10 Z",
            "M2 2 H10 V10 H2 Z",
            "M2,2 10,2 10,10 2,10z",
            "m2 2 l8 0 l0 8 l-8 0 z",
            "m2 2 h8 v8 h-8 z",
            "m2 2 8 0 0 8-8 0z",
            "M2 2h8v8H2",
            " \n M 2 2 H 1e1 V 10.0 L 2 +10 Z ",
        ] {
            assert_eq!(fill(d), [square.to_vec()], "{d}");
        }
        // Coordinates after a closepath are relative to the subpath's start.
        assert_eq!(fill("M2 2 h4 v4 z m10 10 h4 v4 z"), [vec![[2.0, 2.0], [6.0, 2.0], [6.0, 6.0]], vec![[12.0, 12.0], [16.0, 12.0], [16.0, 16.0]]]);
        assert_eq!(fill("M2 2 h4 v4 z l0 6 l6 0"), [vec![[2.0, 2.0], [6.0, 2.0], [6.0, 6.0]], vec![[8.0, 8.0], [2.0, 8.0], [2.0, 2.0]]]);
        assert_eq!(fill("M.5.5 6.5.5 6.5 6.5"), [vec![[0.5, 0.5], [6.5, 0.5], [6.5, 6.5]]]);
    }

    #[test]
    fn path_cubic_and_smooth_cubic() {
        let curve = fill("M0 0 C0 12 12 12 12 0");
        let points = &curve[0];
        assert_eq!((points[0], *points.last().unwrap()), ([0.0, 0.0], [12.0, 0.0]));
        assert!(points.len() > 8 && points.len() < 64, "{}", points.len());
        // Distance to the curve x = 36t² - 24t³, y = 36t(1 - t), sampled densely.
        let on_curve = |p: [f64; 2]| {
            (0..=20_000).map(|i| i as f64 / 20_000.0).map(|t| (36.0 * t * t - 24.0 * t * t * t - p[0]).hypot(36.0 * t * (1.0 - t) - p[1])).fold(f64::INFINITY, f64::min)
        };
        assert!(points.iter().all(|p| on_curve(*p) < 1e-3));
        let error = chord_error(points, on_curve);
        assert!(error > 0.005 && error <= TOLERANCE, "{error}");
        assert!(close(&fill("m0 0 c0 12 12 12 12 0")[0], points));
        // S reflects the previous cubic control point; alone it starts at the current point.
        assert!(close(&fill("M0 0 C0 12 12 12 12 0 S24 -12 24 0")[0], &fill("M0 0 C0 12 12 12 12 0 C12 -12 24 -12 24 0")[0]));
        assert!(close(&fill("M0 0 c0 12 12 12 12 0 s12 -12 12 0")[0], &fill("M0 0 C0 12 12 12 12 0 C12 -12 24 -12 24 0")[0]));
        assert!(close(&fill("M0 0 S12 12 12 0")[0], &fill("M0 0 C0 0 12 12 12 0")[0]));
        assert!(close(&fill("M0 0 Q6 12 12 0 S24 12 24 0")[0], &fill("M0 0 Q6 12 12 0 C12 0 24 12 24 0")[0]));
    }

    #[test]
    fn path_quadratic_and_smooth_quadratic() {
        let curve = fill("M0 0 Q6 12 12 0");
        let points = &curve[0];
        assert_eq!((points[0], *points.last().unwrap()), ([0.0, 0.0], [12.0, 0.0]));
        let on_curve = |p: [f64; 2]| (p[0] * (12.0 - p[0]) / 6.0 - p[1]).abs();
        assert!(points.len() > 6 && points.iter().all(|p| on_curve(*p) < 1e-9));
        // The vertical distance bounds the true one from above.
        let error = chord_error(points, on_curve);
        assert!(error > 0.005 && error <= TOLERANCE, "{error}");
        assert!(close(&fill("m0 0 q6 12 12 0")[0], points));
        assert!(close(&fill("M0 0 Q6 12 12 0 T24 0")[0], &fill("M0 0 Q6 12 12 0 Q18 -12 24 0")[0]));
        assert!(close(&fill("M0 0 q6 12 12 0 t12 0 t-12 0")[0], &fill("M0 0 Q6 12 12 0 Q18 -12 24 0 Q30 12 12 0")[0]));
        assert!(close(&fill("M0 0 T12 6 0 12")[0], &fill("M0 0 Q0 0 12 6 Q24 12 0 12")[0]));
        assert!(close(&fill("M0 0 C0 12 12 12 12 0 T24 6 12 12")[0], &fill("M0 0 C0 12 12 12 12 0 Q12 0 24 6 Q36 12 12 12")[0]));
    }

    #[test]
    fn path_arcs() {
        let from_center = |center: [f64; 2], radius: f64| move |p: [f64; 2]| ((p[0] - center[0]).hypot(p[1] - center[1]) - radius).abs();
        // Sweep 1 runs clockwise on screen: from the left point over the top.
        let top = fill("M2 12 A10 10 0 0 1 22 12 Z");
        assert_eq!((top[0][0], *top[0].last().unwrap()), ([2.0, 12.0], [22.0, 12.0]));
        assert!(top[0].len() > 12 && top[0].iter().all(|p| from_center([12.0, 12.0], 10.0)(*p) < 1e-9 && p[1] <= 12.0));
        let error = chord_error(&top[0], from_center([12.0, 12.0], 10.0));
        assert!(error > 0.005 && error <= TOLERANCE, "{error}");
        assert!((area(&top[0]) - 50.0 * std::f64::consts::PI).abs() < 1.5);
        let bottom = fill("M2 12 A10 10 0 0 0 22 12 Z");
        assert!(bottom[0].iter().all(|p| from_center([12.0, 12.0], 10.0)(*p) < 1e-9 && p[1] >= 12.0));
        // Relative form, flags written without separators.
        assert!(close(&fill("m2 12 a10 10 0 0120 0z")[0], &top[0]));
        // The large flag selects the far center: three quarters of a circle.
        let large = fill("M12 2 A10 10 0 1 1 2 12 L12 12 Z");
        assert!(large[0][..large[0].len() - 1].iter().all(|p| from_center([12.0, 12.0], 10.0)(*p) < 1e-9));
        assert!((area(&large[0]) - 75.0 * std::f64::consts::PI).abs() < 2.5);
        let small = fill("M12 2 A10 10 0 0 0 2 12 L12 12 Z");
        assert!((area(&small[0]) - 25.0 * std::f64::consts::PI).abs() < 1.0);
        // Radii too small for the endpoints are scaled up to a half circle.
        let scaled = fill("M2 12 A1 1 0 0 1 22 12 Z");
        assert!(scaled[0].iter().all(|p| from_center([12.0, 12.0], 10.0)(*p) < 1e-9));
        // A rotated ellipse: every point satisfies the rotated equation.
        let ellipse = fill("M12 12 m-8 0 a8 4 90 1 0 8 0 a8 4 90 1 0 -8 0");
        assert!(ellipse[0].iter().all(|p| p[0] >= 3.99 && p[0] <= 12.01));
        assert!((area(&ellipse[0]) - 32.0 * std::f64::consts::PI).abs() < 1.5, "{}", area(&ellipse[0]));
        // A zero radius is a straight line; an arc to the current point draws nothing.
        assert_eq!(fill("M2 2 A0 5 0 0 1 10 2 L10 10 A5 5 0 1 1 10 10 Z"), [vec![[2.0, 2.0], [10.0, 2.0], [10.0, 10.0]]]);
    }

    #[test]
    fn path_subpaths_become_separate_shapes_and_bridged_holes() {
        // Opposite winding cuts a hole under either rule.
        for attrs in ["", " fill-rule=\"evenodd\""] {
            let ring = image(&format!("<path{attrs} d=\"M0 0H20V20H0Z M5 5V15H15V5Z\"/>")).unwrap().shapes;
            assert_eq!(ring.len(), 1);
            assert_eq!(area(&ring[0].0), 300.0);
            // Outer ring, hole and the bridge walked there and back.
            assert_eq!(ring[0].0.len(), 4 + 4 + 2);
        }
        // The same winding keeps the inner square filled under nonzero only.
        assert_eq!(fill("M0 0H20V20H0Z M5 5H15V15H5Z").iter().map(|r| area(r)).collect::<Vec<_>>(), [400.0]);
        let even_odd = image("<path fill-rule=\"evenodd\" d=\"M0 0H20V20H0Z M5 5H15V15H5Z\"/>").unwrap().shapes;
        assert_eq!(even_odd.iter().map(|(r, _)| area(r)).collect::<Vec<_>>(), [300.0]);
        // Separate subpaths, and an island inside a hole, are shapes of their own.
        assert_eq!(fill("M0 0H4V4H0Z M10 10H14V14H10Z").len(), 2);
        let island = fill("M0 0H20V20H0Z M4 4V16H16V4Z M8 8H12V12H8Z M22 0h2v2h-2z");
        assert_eq!(island.iter().map(|r| area(r)).collect::<Vec<_>>(), [400.0 - 144.0, 16.0, 4.0]);
        // Two holes in one outline share its shape.
        let two = fill("M0 0H20V20H0Z M2 2V8H8V2Z M12 12V18H18V12Z");
        assert_eq!(two.iter().map(|r| area(r)).collect::<Vec<_>>(), [400.0 - 72.0]);
        // A subpath of fewer than three points has nothing to fill.
        assert_eq!(fill("M0 0H4V4H0Z M10 10 L12 12 M5 5 Z").len(), 1);
        // Equal lobes of opposite winding are both filled.
        assert_eq!(fill("M0 0 L4 4 L4 0 L0 4 Z"), [vec![[0.0, 0.0], [4.0, 4.0], [4.0, 0.0], [0.0, 4.0]]]);
    }

    #[test]
    fn path_fill_and_stroke_follow_the_other_elements() {
        let both = image("<path d=\"M2 2 H10 V10\" fill=\"#ff0000\" stroke=\"#00ff00\" stroke-width=\"2\"/>").unwrap().shapes;
        assert_eq!(both.iter().map(|(_, color)| color.as_str()).collect::<Vec<_>>(), ["#ff0000", "#00ff00", "#00ff00"]);
        assert_eq!(both[1].0, [[2.0, 3.0], [10.0, 3.0], [10.0, 1.0], [2.0, 1.0]]);
        let open = image("<path d=\"M2 2 H10 V10\" fill=\"none\" stroke=\"#00ff00\"/>").unwrap().shapes;
        assert_eq!(open.len(), 2);
        let closed = image("<path d=\"M2 2 H10 V10 Z M14 14 h4\" fill=\"none\" stroke=\"#00ff00\"/>").unwrap().shapes;
        assert_eq!(closed.len(), 4);
        // Root presentation attributes apply to a path as to the other elements.
        let inherited = parse_svg_shapes("<svg viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"><path d=\"M4 4 L20 20\"/><circle cx=\"12\" cy=\"12\" r=\"3\" fill=\"#fff\"/></svg>").unwrap().shapes;
        assert_eq!(inherited[0].1, "currentColor");
        assert_eq!(inherited.len(), 1 + 1 + 24);
        assert_eq!(fill("M0 0H4V4Z").len(), 1);
        assert_eq!(image("<path d=\"M0 0H4V4Z\"/>").unwrap().shapes[0].1, "currentColor");
        // The tolerance follows the image size, so a large viewBox does not add points.
        let large = parse_svg_shapes("<svg viewBox=\"0 0 2400 2400\"><path d=\"M0 0 Q600 1200 1200 0\"/></svg>").unwrap().shapes;
        assert_eq!(large[0].0.len(), fill("M0 0 Q6 12 12 0")[0].len());
    }

    #[test]
    fn malformed_path_data_is_refused() {
        for d in [
            "", "   ", "L1 1", "1 1", "Z", "M1", "M1 1 L", "M1 1 L2", "M1 1 X2 2", "M1 1 L2 2 #", "M1 1 L1e999 0", "M1 1 L200000 0",
            "M1 1 L-", "M1 1 L. 1", "M1 1 Z 5 5", "M0 0 A1 1 0 2 0 5 5", "M0 0 A1 1 0 0 5 5", "M0 0 C1 1 2 2", "M0 0 Q1 1", "M0 0 H", "M0 0 L1 1 e",
            "M0 0 L1 NaN", "M0 0 L1 inf", "M0 0 Lé",
            // Relative steps that leave the supported range one by one.
            "M0 0 l90000 0 l90000 0 l0 5 z", "M0 0 h90000 h90000 v5 z", "M90000 0 a90000 90000 0 1 1 1 0 z",
        ] {
            let result = image(&format!("<path d=\"{d}\"/>"));
            assert!(result.is_err(), "{d}: {:?}", result.map(|i| i.shapes));
        }
        assert!(image("<path/>").is_err());
        // Radii that vanish in the arithmetic degrade to a line instead of failing.
        assert_eq!(image("<path d=\"M0 0 A1e-200 1e-200 0 0 1 5 5 L0 5 Z\"/>").unwrap().shapes[0].0, [[0.0, 0.0], [5.0, 5.0], [0.0, 5.0]]);
        assert!(image("<path d=\"M0 0 L5 5\"/>").unwrap_err().contains("no drawable shapes"));
        // Elements that are still unsupported keep their diagnostic.
        assert_eq!(image("<text>a</text>").unwrap_err(), "Unsupported SVG element <text>");
    }

    #[test]
    fn path_respects_the_control_primitive_limits() {
        let zigzag = |count: usize| (0..count).map(|i| format!("L{} {}", i as f64 / 512.0, 1 + i % 2)).collect::<String>();
        assert_eq!(fill(&format!("M0 0 {}", zigzag(MAX_SHAPE_POINTS - 1)))[0].len(), MAX_SHAPE_POINTS);
        assert!(image(&format!("<path d=\"M0 0 {}\"/>", zigzag(MAX_SHAPE_POINTS))).unwrap_err().contains("4096 points"));
        // A hole and its bridge count toward the points of the outline it cuts.
        let holed = |count: usize| image(&format!("<path d=\"M0 3 {} V24 H0 Z M5 5V15H15V5Z\"/>", zigzag(count)));
        assert_eq!(holed(MAX_SHAPE_POINTS - 9).unwrap().shapes[0].0.len(), MAX_SHAPE_POINTS);
        assert!(holed(MAX_SHAPE_POINTS - 8).unwrap_err().contains("4096 points"));
        // Every stroked segment and every filled subpath is one primitive.
        let stroked = |count: usize| image(&format!("<path fill=\"none\" stroke=\"#fff\" d=\"{}\"/>", "M0 0 L5 0 L5 5 ".repeat(count)));
        assert_eq!(stroked(MAX_IMAGE_SHAPES / 2).unwrap().shapes.len(), MAX_IMAGE_SHAPES);
        assert!(stroked(MAX_IMAGE_SHAPES / 2 + 1).unwrap_err().contains("4096 primitives"));
        let filled = |count: usize| image(&format!("<path d=\"{}\"/>", (0..count).map(|i| format!("M{} {} h1 v1 z", i % 64 * 2, i / 64 * 2)).collect::<String>()));
        assert_eq!(filled(MAX_IMAGE_SHAPES).unwrap().shapes.len(), MAX_IMAGE_SHAPES);
        assert!(filled(MAX_IMAGE_SHAPES + 1).unwrap_err().contains("4096 primitives"));
        assert!(image(&format!("<path fill=\"none\" d=\"{}\"/>", "M0 0 L5 0 L5 5 ".repeat(MAX_PATH_POINTS / 3 + 1))).unwrap_err().contains("65536 points"));
    }

    // Material Symbols "search", 24 px: an outline with the lens cut out of it.
    const SEARCH: &str = "M15.5 14h-.79l-.28-.27C15.41 12.59 16 11.11 16 9.5 16 5.91 13.09 3 9.5 3S3 5.91 3 9.5 5.91 16 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z";

    #[test]
    fn real_icon_path_is_one_outline_with_its_hole() {
        let shapes = fill(SEARCH);
        assert_eq!(shapes.len(), 1);
        assert!(shapes[0].len() > 60 && shapes[0].len() < 400, "{}", shapes[0].len());
        assert!(shapes[0].iter().flatten().all(|n| (3.0..=21.0).contains(n)));
        // Ring between radii 6.5 and 4.5 plus the handle.
        let ring = std::f64::consts::PI * (6.5 * 6.5 - 4.5 * 4.5);
        assert!(area(&shapes[0]) > ring + 15.0 && area(&shapes[0]) < ring + 17.0, "{}", area(&shapes[0]));
    }

    #[test]
    fn real_icon_path_renders_with_an_open_lens() {
        let files = BTreeMap::from([
            ("search.svg".to_string(), format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><path d=\"{SEARCH}\"/></svg>")),
            ("Icon.component.ui".to_string(), "component Icon { width:48; height:48; Rectangle { Image { source:'search.svg'; width:48; height:48; color:#ffffff; } } }".to_string()),
            ("App.ui".to_string(), "component App { Column { width:48; height:48; Icon { key:'icon'; } } }".to_string()),
        ]);
        let runtime = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        let pixels = runtime.pixels(48, 48, 1.0);
        // Icon coordinates are doubled by the 48 px box.
        let pixel = |x: usize, y: usize| <[u8; 4]>::try_from(&pixels[(y * 2 * 48 + x * 2) * 4..][..4]).unwrap();
        let (background, ink) = (pixel(1, 1), [255; 4]);
        assert_ne!(background, ink);
        assert_eq!(pixel(9, 4), ink, "lens rim");
        assert_eq!(pixel(4, 9), ink, "lens rim");
        assert_eq!(pixel(18, 18), ink, "handle");
        assert_eq!(pixel(9, 9), background, "inside of the lens");
        assert_eq!(pixel(12, 8), background, "inside of the lens, on the bridge side");
        assert_eq!(pixel(20, 6), background);
    }
}
