//! SVG path data (`d`) flattened to polylines for the polygon rasterizer.
//! Curves and arcs are subdivided so the chord error stays under `tolerance`
//! (viewBox units). The JS twin is `flattenSvgPath` in `src/svg-shapes.js`;
//! keep the two in step.

const MAX_POINTS: usize = 4096;
const MAX_CURVE_STEPS: f64 = 128.0;

pub(super) struct SubPath {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
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
            return Err("Invalid SVG path number".into());
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
        // The scanned range is ASCII digits, signs, a dot and an exponent.
        let value: f64 = std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse().ok())
            .ok_or("Invalid SVG path number")?;
        if !value.is_finite() || value.abs() > 100_000.0 {
            return Err("Invalid SVG path number".into());
        }
        Ok(value)
    }

    /// Arc flags are single digits and may be written without separators.
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
            _ => Err("Invalid SVG arc flag".into()),
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

/// Endpoint arc (SVG 1.1 F.6.5) as chords; a zero radius is a straight line.
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

pub(super) fn flatten_svg_path(d: &str, tolerance: f64) -> Result<Vec<SubPath>, String> {
    let mut scan = Scanner {
        bytes: d.as_bytes(),
        at: 0,
    };
    let mut subpaths: Vec<SubPath> = Vec::new();
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut started = false;
    let (mut current, mut start) = ([0.0f64; 2], [0.0f64; 2]);
    // Second control point of the previous C/S and control point of Q/T.
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
                return Err("Invalid SVG path data".into());
            }
            break;
        } else if command == 0 || command.eq_ignore_ascii_case(&b'z') {
            return Err("SVG path must start with M".into());
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
                // Further coordinate pairs after a moveto are implicit linetos.
                command = if relative { b'l' } else { b'L' };
            }
            _ if !started => {
                return Err("SVG path must start with M".into());
            }
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
                    _ => return Err(format!("Unsupported SVG path command {}", command as char)),
                }
                if matches!(other, b'L' | b'H' | b'V') {
                    points.push(current);
                }
            }
        }
        (cubic_ctrl, quad_ctrl) = (next_cubic, next_quad);
        let total = subpaths.iter().map(|s| s.points.len()).sum::<usize>() + points.len();
        if total > MAX_POINTS {
            return Err("SVG path is too long".into());
        }
    }
    flush(&mut points, false, &mut subpaths);
    if subpaths.is_empty() {
        return Err("SVG path has no segments".into());
    }
    Ok(subpaths)
}

/// The rasterizer fills one contour at a time, so a contour inside another
/// would paint over it instead of cutting a hole.
pub(super) fn overlapping_contours(subpaths: &[SubPath]) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(d: &str) -> Vec<SubPath> {
        flatten_svg_path(d, 0.01).unwrap()
    }

    #[test]
    fn lines_relative_moves_and_close_follow_the_svg_grammar() {
        let subs = flat("M1 2 3 4h2v-1l-1-1zm10 0L20,5");
        assert_eq!(subs.len(), 2);
        assert!(subs[0].closed);
        assert_eq!(
            subs[0].points,
            [[1., 2.], [3., 4.], [5., 4.], [5., 3.], [4., 2.]]
        );
        // A relative moveto after Z starts from the closed subpath's first point.
        assert!(!subs[1].closed);
        assert_eq!(subs[1].points, [[11., 2.], [20., 5.]]);
    }

    #[test]
    fn arcs_stay_on_the_circle_and_honour_compact_flags() {
        // Half circle of radius 10 around (10, 10), flags written without separators.
        let subs = flat("M0 10a10 10 0 0120 0");
        let points = &subs[0].points;
        assert!(points.len() > 16);
        assert_eq!(*points.last().unwrap(), [20., 10.]);
        for p in points {
            let r = (p[0] - 10.).hypot(p[1] - 10.);
            assert!((r - 10.).abs() < 1e-9, "radius {r}");
            assert!(
                p[1] <= 10. + 1e-9,
                "sweep=1 from the left goes over the top"
            );
        }
        // Too-small radii are scaled up until the arc reaches its end point.
        let scaled = flat("M0 0A1 1 0 0 0 10 0");
        let mid = scaled[0].points[scaled[0].points.len() / 2];
        assert!((mid[0] - 5.).abs() < 0.5 && (mid[1].abs() - 5.).abs() < 0.1);
    }

    #[test]
    fn curves_subdivide_to_the_tolerance_and_reflect_smooth_controls() {
        let fine = flatten_svg_path("M0 0C0 10 10 10 10 0", 0.001).unwrap();
        let coarse = flatten_svg_path("M0 0C0 10 10 10 10 0", 1.0).unwrap();
        assert!(fine[0].points.len() > coarse[0].points.len());
        let top = fine[0].points.iter().map(|p| p[1]).fold(0.0, f64::max);
        assert!((top - 7.5).abs() < 0.01);
        // S mirrors the previous second control point: the join stays smooth.
        let smooth = flat("M0 0C0 5 5 5 5 0S10 -5 10 0");
        let bottom = smooth[0].points.iter().map(|p| p[1]).fold(0.0, f64::min);
        assert!((bottom + 3.75).abs() < 0.01);
        let quad = flat("M0 0Q5 10 10 0T20 0");
        let low = quad[0].points.iter().map(|p| p[1]).fold(0.0, f64::min);
        assert!((low + 5.).abs() < 0.01);
    }

    #[test]
    fn a_path_element_becomes_fill_polygons_and_stroke_quads() {
        use super::super::svg::parse_svg_shapes;
        // A ring sector, as a sunburst draws it: outer arc, edge, inner arc back, close.
        let sector = parse_svg_shapes(
            r##"<svg viewBox="0 0 200 200"><path fill="#ffffff" d="M100 20A80 80 0 0 1 180 100L140 100A40 40 0 0 0 100 60Z"/></svg>"##,
        )
        .unwrap();
        assert_eq!(sector.shapes.len(), 1);
        assert!(sector.shapes[0].0.len() > 24);
        for p in &sector.shapes[0].0 {
            let r = (p[0] - 100.).hypot(p[1] - 100.);
            assert!(
                r > 39.9 && r < 80.1 && p[0] >= 99.999 && p[1] <= 100.001,
                "{p:?}"
            );
        }
        let stroked = parse_svg_shapes(
            r##"<svg viewBox="0 0 24 24"><path fill="none" stroke="#ffffff" stroke-width="2" d="M2 2L20 2L20 20"/></svg>"##,
        )
        .unwrap();
        assert_eq!(stroked.shapes.len(), 2);
        let holed = parse_svg_shapes(
            r##"<svg viewBox="0 0 24 24"><path d="M0 0h10v10h-10zM2 2h6v6h-6z"/></svg>"##,
        );
        assert!(holed.unwrap_err().contains("holes"));
    }

    #[test]
    fn bad_data_is_diagnosed() {
        for d in [
            "",
            "L1 1",
            "M1",
            "M0 0L1",
            "M0 0A1 1 0 2 0 1 1",
            "M0 0X1 1",
            "M0 0L1e999 0",
        ] {
            assert!(flatten_svg_path(d, 0.01).is_err(), "{d:?}");
        }
    }

    #[test]
    fn nested_contours_are_reported_and_separate_ones_are_not() {
        assert!(overlapping_contours(&flat("M0 0h10v10h-10zM2 2h6v6h-6z")));
        assert!(!overlapping_contours(&flat("M0 0h10v10h-10zM20 0h6v6h-6z")));
    }
}
