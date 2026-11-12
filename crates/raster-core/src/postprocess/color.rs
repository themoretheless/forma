//! Effects on the colour buffer: anti-aliasing, tone mapping, bloom and lens artefacts.

/// Fast Approximate Anti-Aliasing (FXAA) screen-space pass based on Rec. 709 luma contrast
/// detection and local gradient blending.
pub fn fxaa_luma_antialias(rgba: &[[f32; 4]], width: usize, height: usize) -> Vec<[f32; 4]> {
    let total = width.saturating_mul(height);
    if rgba.len() < total || width < 3 || height < 3 {
        return rgba.to_vec();
    }

    const EDGE_THRESHOLD_MIN: f32 = 0.0312;
    const EDGE_THRESHOLD_REL: f32 = 0.125;

    #[inline]
    fn luma(c: [f32; 4]) -> f32 {
        0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
    }

    let mut out = rgba[..total].to_vec();

    for y in 1..(height - 1) {
        let row_prev = &rgba[(y - 1) * width..y * width];
        let row_curr = &rgba[y * width..(y + 1) * width];
        let row_next = &rgba[(y + 1) * width..(y + 2) * width];
        let out_row = &mut out[y * width..(y + 1) * width];

        for x in 1..(width - 1) {
            let m = row_curr[x];
            let n = row_prev[x];
            let s = row_next[x];
            let w = row_curr[x - 1];
            let e = row_curr[x + 1];

            let l_m = luma(m);
            let l_n = luma(n);
            let l_s = luma(s);
            let l_w = luma(w);
            let l_e = luma(e);

            let l_min = l_m.min(l_n.min(l_s.min(l_w.min(l_e))));
            let l_max = l_m.max(l_n.max(l_s.max(l_w.max(l_e))));
            let l_range = l_max - l_min;

            if l_range < EDGE_THRESHOLD_MIN.max(l_max * EDGE_THRESHOLD_REL) {
                continue;
            }

            let nw = row_prev[x - 1];
            let ne = row_prev[x + 1];
            let sw = row_next[x - 1];
            let se = row_next[x + 1];

            let l_nw = luma(nw);
            let l_ne = luma(ne);
            let l_sw = luma(sw);
            let l_se = luma(se);

            // Subpixel contrast and edge orientation (horizontal vs vertical edge)
            // A horizontal edge has strong vertical gradient (|N - S|), whereas a vertical edge
            // has strong horizontal gradient (|W - E|).
            let edge_horiz = (l_nw + l_sw - 2.0 * l_w).abs()
                + 2.0 * (l_n + l_s - 2.0 * l_m).abs()
                + (l_ne + l_se - 2.0 * l_e).abs();
            let edge_vert = (l_nw + l_ne - 2.0 * l_n).abs()
                + 2.0 * (l_w + l_e - 2.0 * l_m).abs()
                + (l_sw + l_se - 2.0 * l_s).abs();

            let is_horizontal = edge_horiz >= edge_vert;
            let (neg_neighbor, pos_neighbor, l_neg, l_pos) = if is_horizontal {
                (n, s, l_n, l_s)
            } else {
                (w, e, l_w, l_e)
            };

            let grad_neg = (l_neg - l_m).abs();
            let grad_pos = (l_pos - l_m).abs();
            let pair_neighbor = if grad_neg >= grad_pos {
                neg_neighbor
            } else {
                pos_neighbor
            };

            // Subpixel aliasing factor from 3x3 box average vs center luma
            let l_avg =
                (2.0 * (l_n + l_s + l_w + l_e) + (l_nw + l_ne + l_sw + l_se)) * (1.0 / 12.0);
            let subpix = ((l_avg - l_m).abs() / l_range.max(1e-6)).clamp(0.0, 1.0);
            let blend = (0.25 + 0.5 * subpix).clamp(0.0, 0.75);
            let inv_blend = 1.0 - blend;

            out_row[x] = [
                m[0].mul_add(inv_blend, pair_neighbor[0] * blend),
                m[1].mul_add(inv_blend, pair_neighbor[1] * blend),
                m[2].mul_add(inv_blend, pair_neighbor[2] * blend),
                m[3],
            ];
        }
    }

    out
}

/// Apply Krzysztof Narkowicz's ACES filmic HDR tone-mapping curve followed by gamma correction
/// (`pow(1.0 / gamma)`), returning display-referred `[R, G, B]` in `[0.0, 1.0]`.
pub fn tone_map_aces_filmic(hdr_rgb: &[[f32; 3]], exposure: f32, gamma: f32) -> Vec<[f32; 3]> {
    const A: f32 = 2.51;
    const B: f32 = 0.03;
    const C: f32 = 2.43;
    const D: f32 = 0.59;
    const E: f32 = 0.14;

    let exp = if exposure.is_finite() && exposure > 0.0 {
        exposure
    } else {
        1.0
    };
    let inv_gamma = if gamma.is_finite() && gamma > 0.0 {
        1.0 / gamma
    } else {
        1.0 / 2.2
    };

    #[inline]
    fn map_channel(v: f32, exp: f32, inv_gamma: f32) -> f32 {
        if !v.is_finite() || v <= 0.0 {
            return 0.0;
        }
        let x = v * exp;
        let mapped = ((x * (A * x + B)) / (x * (C * x + D) + E)).clamp(0.0, 1.0);
        mapped.powf(inv_gamma).clamp(0.0, 1.0)
    }

    hdr_rgb
        .iter()
        .map(|&[r, g, b]| {
            [
                map_channel(r, exp, inv_gamma),
                map_channel(g, exp, inv_gamma),
                map_channel(b, exp, inv_gamma),
            ]
        })
        .collect()
}

/// Separable 2-pass horizontal + vertical 5-tap Gaussian bloom pass for emissive CAD highlights
/// and selected edges.
///
/// 1. Extracts HDR bright-pass residuals above Rec. 709 luminance `threshold`.
/// 2. Convolves horizontally and vertically with `[1, 4, 6, 4, 1] / 16` binomial Gaussian weights.
/// 3. Adds the blurred glow scaled by `intensity` onto `hdr_rgb`.
pub fn bloom_bright_pass_and_blur(
    hdr_rgb: &[[f32; 3]],
    width: usize,
    height: usize,
    threshold: f32,
    intensity: f32,
) -> Vec<[f32; 3]> {
    let total = width.saturating_mul(height);
    if hdr_rgb.len() < total || width == 0 || height == 0 {
        return hdr_rgb.to_vec();
    }
    let gain = if intensity.is_finite() && intensity > 0.0 {
        intensity
    } else {
        0.0
    };
    if gain == 0.0 {
        return hdr_rgb[..total].to_vec();
    }
    let thresh = if threshold.is_finite() {
        threshold.max(0.0)
    } else {
        1.0
    };

    // Pass 0: bright-pass extraction
    let mut bright = vec![[0.0_f32; 3]; total];
    for (dst, &[r, g, b]) in bright.iter_mut().zip(hdr_rgb[..total].iter()) {
        let cr = if r.is_finite() && r > 0.0 { r } else { 0.0 };
        let cg = if g.is_finite() && g > 0.0 { g } else { 0.0 };
        let cb = if b.is_finite() && b > 0.0 { b } else { 0.0 };
        let luma = 0.2126 * cr + 0.7152 * cg + 0.0722 * cb;
        if luma > thresh && luma > 1e-6 {
            let scale = (luma - thresh) / luma;
            *dst = [cr * scale, cg * scale, cb * scale];
        }
    }

    const WEIGHTS: [f32; 5] = [0.0625, 0.25, 0.375, 0.25, 0.0625];
    let max_x = (width - 1) as isize;
    let max_y = (height - 1) as isize;

    // Pass 1: horizontal 5-tap Gaussian blur
    let mut temp = vec![[0.0_f32; 3]; total];
    for y in 0..height {
        let row_in = &bright[y * width..(y + 1) * width];
        let row_out = &mut temp[y * width..(y + 1) * width];
        for (x, out_px) in row_out.iter_mut().enumerate() {
            let xi = x as isize;
            let mut acc = [0.0_f32; 3];
            for (k, &w) in WEIGHTS.iter().enumerate() {
                let offset = (k as isize) - 2;
                let sx = (xi + offset).clamp(0, max_x) as usize;
                let sample = row_in[sx];
                acc[0] = sample[0].mul_add(w, acc[0]);
                acc[1] = sample[1].mul_add(w, acc[1]);
                acc[2] = sample[2].mul_add(w, acc[2]);
            }
            *out_px = acc;
        }
    }

    // Pass 2: vertical 5-tap Gaussian blur + additive composite onto hdr_rgb
    let mut out = Vec::with_capacity(total);
    for y in 0..height {
        let yi = y as isize;
        let base_row = &hdr_rgb[y * width..(y + 1) * width];
        for (x, &base_px) in base_row.iter().enumerate() {
            let mut acc = [0.0_f32; 3];
            for (k, &w) in WEIGHTS.iter().enumerate() {
                let offset = (k as isize) - 2;
                let sy = (yi + offset).clamp(0, max_y) as usize;
                let sample = temp[sy * width + x];
                acc[0] = sample[0].mul_add(w, acc[0]);
                acc[1] = sample[1].mul_add(w, acc[1]);
                acc[2] = sample[2].mul_add(w, acc[2]);
            }
            out.push([
                acc[0].mul_add(gain, base_px[0]),
                acc[1].mul_add(gain, base_px[1]),
                acc[2].mul_add(gain, base_px[2]),
            ]);
        }
    }

    out
}

/// Applies radial lens vignette darkening and subtle chromatic aberration to a flat `RGBA` (`4 * width * height`)
/// floating-point framebuffer, preserving the alpha channel.
pub fn apply_chromatic_aberration_and_vignette(
    rgba: &[f32],
    width: usize,
    height: usize,
    vignette_strength: f32,
) -> Vec<f32> {
    let total_px = width.saturating_mul(height);
    let total_floats = total_px.saturating_mul(4);
    if width == 0 || height == 0 || rgba.len() < total_floats {
        return rgba.to_vec();
    }
    let strength = if vignette_strength.is_finite() {
        vignette_strength.clamp(0.0, 2.0)
    } else {
        0.0
    };
    let inv_w = if width > 1 {
        2.0 / ((width - 1) as f32)
    } else {
        0.0
    };
    let inv_h = if height > 1 {
        2.0 / ((height - 1) as f32)
    } else {
        0.0
    };
    let max_x = (width - 1) as isize;
    let mut out = vec![0.0_f32; total_floats];

    for y in 0..height {
        let ny = (y as f32) * inv_h - 1.0;
        let ny2 = ny * ny;
        let row_base = y * width * 4;
        for x in 0..width {
            let nx = (x as f32) * inv_w - 1.0;
            // Normalized squared radius r^2 in [0, 2] (0 at center, 1 at edge midpoints, 2 at corners)
            let r2 = 0.5 * (nx * nx + ny2);
            let vig = (1.0 - strength * r2).clamp(0.0, 1.0);

            // Radial horizontal dispersion offset (+/- 1px near outer edges when strength > 0)
            let shift = if strength > 0.0 && nx.abs() > 0.5 {
                if nx > 0.0 { 1_isize } else { -1_isize }
            } else {
                0_isize
            };
            let xr = ((x as isize) + shift).clamp(0, max_x) as usize;
            let xb = ((x as isize) - shift).clamp(0, max_x) as usize;

            let idx = row_base + x * 4;
            let r_ch = rgba[row_base + xr * 4];
            let g_ch = rgba[idx + 1];
            let b_ch = rgba[row_base + xb * 4 + 2];
            let a_ch = rgba[idx + 3];

            out[idx] = r_ch * vig;
            out[idx + 1] = g_ch * vig;
            out[idx + 2] = b_ch * vig;
            out[idx + 3] = a_ch;
        }
    }

    out
}
