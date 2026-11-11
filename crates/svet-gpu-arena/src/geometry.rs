use crate::arena::checked_range;

/// Screen-space viewport parameters for 3D clip-space vertex picking.
#[derive(Clone, Copy, Debug)]
pub struct ViewportPickQuery {
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub click_x: f32,
    pub click_y: f32,
    pub max_radius_px: f32,
}

impl ViewportPickQuery {
    #[inline]
    pub const fn new(
        viewport_w: f32,
        viewport_h: f32,
        click_x: f32,
        click_y: f32,
        max_radius_px: f32,
    ) -> Self {
        Self {
            viewport_w,
            viewport_h,
            click_x,
            click_y,
            max_radius_px,
        }
    }

    #[inline]
    fn is_valid(self) -> bool {
        self.viewport_w.is_finite()
            && self.viewport_h.is_finite()
            && self.viewport_w > 0.0
            && self.viewport_h > 0.0
            && self.click_x.is_finite()
            && self.click_y.is_finite()
            && self.max_radius_px.is_finite()
            && self.max_radius_px >= 0.0
    }
}

/// Clips a segment (`indices.len() == 2`) or convex polygon (`indices.len() in 3..=4`)
/// in homogeneous WebGPU clip coordinates (`x, y in [-w, w]`, `z in [0, w]`, `w > 0`),
/// writing clipped `[x, y, z, w]` vertices into `out` and returning the vertex count.
pub fn clip_primitive_3d(
    verts: &[f32],
    offset: u32,
    count: u32,
    indices: &[u32],
    out: &mut Vec<f32>,
) -> u32 {
    out.clear();
    if !(2..=4).contains(&indices.len()) || indices.iter().any(|&i| i >= count) {
        return 0;
    }
    let Some(total) = (count as usize).checked_mul(4) else {
        return 0;
    };
    let Some((base_start, _)) = checked_range(offset, total, verts.len()) else {
        return 0;
    };

    let mut buf_a = [[0.0f64; 4]; 12];
    let mut len_a = indices.len();
    for (slot, &index) in indices.iter().enumerate() {
        let base = base_start + index as usize * 4;
        let v = std::array::from_fn(|axis| f64::from(verts[base + axis]));
        if v.iter().any(|value| !value.is_finite()) {
            return 0;
        }
        buf_a[slot] = v;
    }

    let distance = |v: [f64; 4], plane: usize| match plane {
        0 => v[3] + v[0],
        1 => v[3] - v[0],
        2 => v[3] + v[1],
        3 => v[3] - v[1],
        4 => v[2],
        _ => v[3] - v[2],
    };
    let interpolate = |a: [f64; 4], b: [f64; 4], t: f64| {
        std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
    };

    if len_a == 2 {
        let a = buf_a[0];
        let b = buf_a[1];
        let mut enter: f64 = 0.0;
        let mut exit: f64 = 1.0;
        for plane in 0..6 {
            let da = distance(a, plane);
            let db = distance(b, plane);
            if da < 0.0 && db < 0.0 {
                return 0;
            }
            if da < 0.0 {
                enter = enter.max(da / (da - db));
            }
            if db < 0.0 {
                exit = exit.min(da / (da - db));
            }
        }
        if enter > exit {
            return 0;
        }
        buf_a[0] = interpolate(a, b, enter);
        buf_a[1] = interpolate(a, b, exit);
    } else {
        let mut buf_b = [[0.0f64; 4]; 12];
        for plane in 0..6 {
            if len_a == 0 {
                break;
            }
            let mut len_b = 0usize;
            let mut previous = buf_a[len_a - 1];
            let mut dp = distance(previous, plane);
            for &current in &buf_a[..len_a] {
                let dc = distance(current, plane);
                if (dp >= 0.0) != (dc >= 0.0) && len_b < buf_b.len() {
                    buf_b[len_b] = interpolate(previous, current, dp / (dp - dc));
                    len_b += 1;
                }
                if dc >= 0.0 && len_b < buf_b.len() {
                    buf_b[len_b] = current;
                    len_b += 1;
                }
                previous = current;
                dp = dc;
            }
            buf_a[..len_b].copy_from_slice(&buf_b[..len_b]);
            len_a = len_b;
        }
    }

    if buf_a[..len_a].iter().any(|v| v[3] <= 0.0) {
        return 0;
    }
    for vertex in &buf_a[..len_a] {
        out.extend(vertex.map(|value| value as f32));
    }
    (out.len() / 4) as u32
}

/// Z-aware 3D vertex picking without triangle occlusion.
pub fn pick_nearest_vertex_3d(
    verts: &[f32],
    clip_verts_offset: u32,
    vertex_count: u32,
    query: ViewportPickQuery,
) -> i32 {
    pick_vertex_impl(verts, clip_verts_offset, vertex_count, query, &[])
}

/// Z-aware 3D vertex picking with clipped-triangle foreground occlusion.
pub fn pick_visible_vertex_3d(
    verts: &[f32],
    offset: u32,
    count: u32,
    pick_indices: &[u32],
    index_count: u32,
    clip_scratch: &mut Vec<f32>,
    query: ViewportPickQuery,
) -> i32 {
    let len = index_count as usize;
    if len > pick_indices.len() || !len.is_multiple_of(3) {
        return -1;
    }
    let mut triangles = Vec::new();
    for base in (0..len).step_by(3) {
        let indices = [
            pick_indices[base],
            pick_indices[base + 1],
            pick_indices[base + 2],
        ];
        let size = clip_primitive_3d(verts, offset, count, &indices, clip_scratch) as usize;
        if size < 3 {
            continue;
        }
        let mut points = [[0.0f64; 3]; 12];
        for (slot, v) in clip_scratch.chunks_exact(4).take(12).enumerate() {
            let w = f64::from(v[3]);
            points[slot] = [
                f64::from(v[0]) / w,
                f64::from(v[1]) / w,
                f64::from(v[2]) / w,
            ];
        }
        for i in 1..size.min(12) - 1 {
            triangles.push([points[0], points[i], points[i + 1]]);
        }
    }
    pick_vertex_impl(verts, offset, count, query, &triangles)
}

fn pick_vertex_impl(
    verts: &[f32],
    clip_verts_offset: u32,
    vertex_count: u32,
    query: ViewportPickQuery,
    triangles: &[[[f64; 3]; 3]],
) -> i32 {
    if !query.is_valid() {
        return -1;
    }
    let Some(total) = (vertex_count as usize).checked_mul(4) else {
        return -1;
    };
    let Some((base, _)) = checked_range(clip_verts_offset, total, verts.len()) else {
        return -1;
    };

    let half_w = query.viewport_w * 0.5;
    let half_h = query.viewport_h * 0.5;
    let max_dist_sq = query.max_radius_px * query.max_radius_px;

    let mut best_idx: i32 = -1;
    let mut best_score: f32 = f32::INFINITY;
    let mut best_ndc_z: f32 = f32::INFINITY;

    for i in 0..(vertex_count as usize) {
        let off = base + i * 4;
        let x = verts[off];
        let y = verts[off + 1];
        let z = verts[off + 2];
        let w = verts[off + 3];

        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || !w.is_finite()
            || w <= 0.0
            || x < -w
            || x > w
            || y < -w
            || y > w
            || z < 0.0
            || z > w
        {
            continue;
        }
        let sx = half_w + (x / w) * half_w;
        let sy = half_h - (y / w) * half_h;
        let ndc_z = z / w;

        let dx = sx - query.click_x;
        let dy = sy - query.click_y;
        let dist_sq = dx * dx + dy * dy;

        if dist_sq <= max_dist_sq {
            let px = f64::from(x) / f64::from(w);
            let py = f64::from(y) / f64::from(w);
            let pz = f64::from(z) / f64::from(w);
            let hidden = triangles.iter().any(|[a, b, c]| {
                let denominator = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if denominator.abs() < 1e-14 {
                    return false;
                }
                let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / denominator;
                let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / denominator;
                let t = 1.0 - u - v;
                u >= -1e-9
                    && v >= -1e-9
                    && t >= -1e-9
                    && u * a[2] + v * b[2] + t * c[2] + 1e-6 < pz
            });
            if hidden {
                continue;
            }
            // Depth-aware composite score: when two vertices project near the same 2D pixel
            // (within ~4px tolerance), the foreground vertex (smaller `ndc_z`) always wins.
            let depth_bias = ndc_z * 16.0;
            let score = dist_sq + depth_bias;
            if score < best_score || ((score - best_score).abs() < 1e-3 && ndc_z < best_ndc_z) {
                best_score = score;
                best_ndc_z = ndc_z;
                best_idx = i as i32;
            }
        }
    }

    best_idx
}
