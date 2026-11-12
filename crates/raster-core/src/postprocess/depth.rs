//! Effects computed from the depth buffer: occlusion, smoothing, normals and outlines.

/// Screen-space ambient occlusion (SSAO) estimation from a normalized depth buffer (`0.0..=1.0`,
/// where `1.0` is the far background clear value).
///
/// Returns one occlusion visibility factor in `[0.0, 1.0]` per pixel (`1.0` = fully unoccluded,
/// `< 1.0` = darkened crease/cavity).
pub fn compute_ssao_ao(
    depth: &[f32],
    width: u32,
    height: u32,
    radius_px: u32,
    bias: f32,
    intensity: f32,
) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let total = w.saturating_mul(h);
    if depth.len() < total || w == 0 || h == 0 || radius_px == 0 {
        return vec![1.0; depth.len().max(total)];
    }

    // 8-tap radial spiral offsets in [-1, 1]^2
    const DIAG: f32 = std::f32::consts::FRAC_1_SQRT_2;
    const OFFSETS: [(f32, f32); 8] = [
        (1.0, 0.0),
        (-1.0, 0.0),
        (0.0, 1.0),
        (0.0, -1.0),
        (DIAG, DIAG),
        (-DIAG, DIAG),
        (DIAG, -DIAG),
        (-DIAG, -DIAG),
    ];

    let r = radius_px as f32;
    let range_limit = (bias * 32.0).max(0.05);
    let inv_range_limit = 1.0 / range_limit;
    let intensity_scale = (intensity.max(0.0)) * (1.0 / OFFSETS.len() as f32);
    // Precompute integer tap offsets (dx, dy) once per pass
    let tap_offsets: [(isize, isize); 8] = std::array::from_fn(|i| {
        (
            (OFFSETS[i].0 * r).round() as isize,
            (OFFSETS[i].1 * r).round() as isize,
        )
    });
    let max_x = (w - 1) as isize;
    let max_y = (h - 1) as isize;
    let mut ao = vec![1.0_f32; total];

    for y in 0..h {
        let row_off = y * w;
        let yi = y as isize;
        for x in 0..w {
            let center_d = depth[row_off + x];
            if !center_d.is_finite() || center_d >= 1.0 {
                continue;
            }
            let xi = x as isize;
            let mut occluded = 0.0_f32;
            for &(dx, dy) in &tap_offsets {
                let sx = (xi + dx).clamp(0, max_x) as usize;
                let sy = (yi + dy).clamp(0, max_y) as usize;
                let sample_d = depth[sy * w + sx];
                let diff = center_d - sample_d;
                if diff > bias && diff < range_limit {
                    occluded += diff.mul_add(-inv_range_limit, 1.0);
                }
            }
            let occ_norm = occluded * intensity_scale;
            ao[row_off + x] = (1.0 - occ_norm).clamp(0.0, 1.0);
        }
    }
    ao
}

/// Apply a 3x3 Sobel edge silhouette detector to a single-channel depth or luminance image.
///
/// Returns edge magnitude normalized to `[0.0, 1.0]` per pixel (`1.0` where gradient >= `threshold`).
pub fn sobel_edge_outline(channel: &[f32], width: u32, height: u32, threshold: f32) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let total = w.saturating_mul(h);
    if channel.len() < total || w < 3 || h < 3 {
        return vec![0.0; channel.len().max(total)];
    }
    let inv_thresh = 1.0 / threshold.max(1e-6);
    let mut out = vec![0.0_f32; total];

    for y in 1..(h - 1) {
        let row_prev = &channel[(y - 1) * w..y * w];
        let row_curr = &channel[y * w..(y + 1) * w];
        let row_next = &channel[(y + 1) * w..(y + 2) * w];
        let out_row = &mut out[y * w..(y + 1) * w];

        for x in 1..(w - 1) {
            let tl = row_prev[x - 1];
            let tc = row_prev[x];
            let tr = row_prev[x + 1];
            let ml = row_curr[x - 1];
            let mr = row_curr[x + 1];
            let bl = row_next[x - 1];
            let bc = row_next[x];
            let br = row_next[x + 1];

            let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
            let gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);
            let mag = gx.mul_add(gx, gy * gy).sqrt();
            out_row[x] = (mag * inv_thresh).clamp(0.0, 1.0);
        }
    }
    out
}

/// Edge-preserving bilateral filter for smoothing SSAO / depth buffers without bleeding across
/// silhouette depth discontinuities.
///
/// - `depth`: input scalar buffer of length `width * height`.
/// - `spatial_sigma`: Gaussian spatial standard deviation in pixels (e.g. `1.5`).
/// - `depth_sigma`: Gaussian range/depth standard deviation (e.g. `0.02`).
pub fn bilateral_depth_filter(
    depth: &[f32],
    width: usize,
    height: usize,
    spatial_sigma: f32,
    depth_sigma: f32,
) -> Vec<f32> {
    let total = width.saturating_mul(height);
    if depth.len() < total || width == 0 || height == 0 {
        return depth.to_vec();
    }
    if !spatial_sigma.is_finite()
        || spatial_sigma <= 0.0
        || !depth_sigma.is_finite()
        || depth_sigma <= 0.0
    {
        return depth[..total].to_vec();
    }
    let radius = ((spatial_sigma * 2.0).ceil() as isize).clamp(1, 4);
    let inv_two_spatial_var = 0.5 / (spatial_sigma * spatial_sigma);
    let inv_two_depth_var = 0.5 / (depth_sigma * depth_sigma);
    // Precompute 2D spatial Gaussian kernel weights outside the pixel loop (max 9x9 = 81 taps)
    let k_dim = (2 * radius + 1) as usize;
    let mut spatial_weights = [0.0_f32; 81];
    for dy in -radius..=radius {
        let ky = (dy + radius) as usize;
        for dx in -radius..=radius {
            let kx = (dx + radius) as usize;
            let dist_sq = (dx * dx + dy * dy) as f32;
            spatial_weights[ky * k_dim + kx] = (-dist_sq * inv_two_spatial_var).exp();
        }
    }

    let max_x = (width - 1) as isize;
    let max_y = (height - 1) as isize;
    let mut out = vec![0.0_f32; total];

    for y in 0..height {
        let row_off = y * width;
        let yi = y as isize;
        for x in 0..width {
            let center = depth[row_off + x];
            if !center.is_finite() {
                out[row_off + x] = center;
                continue;
            }
            let xi = x as isize;
            let mut sum_val = 0.0_f32;
            let mut sum_w = 0.0_f32;
            for dy in -radius..=radius {
                let ny = (yi + dy).clamp(0, max_y) as usize;
                let ny_row = &depth[ny * width..(ny + 1) * width];
                let ky_base = ((dy + radius) as usize) * k_dim;
                for dx in -radius..=radius {
                    let nx = (xi + dx).clamp(0, max_x) as usize;
                    let sample = ny_row[nx];
                    if !sample.is_finite() {
                        continue;
                    }
                    let w_spatial = spatial_weights[ky_base + (dx + radius) as usize];
                    let diff = sample - center;
                    let w = w_spatial * (-diff * diff * inv_two_depth_var).exp();
                    sum_val = sample.mul_add(w, sum_val);
                    sum_w += w;
                }
            }
            out[row_off + x] = if sum_w > 0.0 { sum_val / sum_w } else { center };
        }
    }
    out
}

/// Reconstruct unit view-space surface normals `[nx, ny, nz]` (with `nz >= 0.0` pointing toward the viewer)
/// from a normalized depth buffer (`0.0..=1.0`, where `1.0` is far background).
pub fn compute_normal_map_from_depth(
    depth: &[f32],
    width: usize,
    height: usize,
    fov_y_rad: f32,
) -> Vec<[f32; 3]> {
    let total = width.saturating_mul(height);
    if depth.len() < total || width == 0 || height == 0 {
        return vec![[0.0, 0.0, 1.0]; total];
    }
    let half_fov = (0.5 * fov_y_rad).clamp(0.05, 1.5);
    let tan_half_fov = half_fov.tan().max(1e-4);
    let inv_w = 2.0 * tan_half_fov / (width.max(1) as f32);
    let inv_h = 2.0 * tan_half_fov / (height.max(1) as f32);
    let mut normals = vec![[0.0_f32, 0.0, 1.0]; total];

    let sample_d = |x: usize, y: usize| -> f32 { depth[y * width + x] };

    for y in 0..height {
        for x in 0..width {
            let c = sample_d(x, y);
            if !c.is_finite() || c >= 1.0 {
                continue;
            }
            let xl = x.saturating_sub(1);
            let xr = (x + 1).min(width - 1);
            let yu = y.saturating_sub(1);
            let yd = (y + 1).min(height - 1);

            let dl = sample_d(xl, y);
            let dr = sample_d(xr, y);
            let du = sample_d(x, yu);
            let dd = sample_d(x, yd);

            // Choose the smaller one-sided depth delta to avoid silhouette bleeding
            let dz_dx = if (dr - c).abs() < (c - dl).abs() {
                dr - c
            } else {
                c - dl
            };
            let dz_dy = if (dd - c).abs() < (c - du).abs() {
                dd - c
            } else {
                c - du
            };

            let nx = -dz_dx;
            let ny = dz_dy;
            let nz = (inv_w + inv_h) * 0.5;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len > 1e-12 {
                normals[y * width + x] = [nx / len, ny / len, nz / len];
            }
        }
    }
    normals
}
