//! Source-over compositing of straight-alpha RGBA8 pixels, shared by every CPU raster path.
//!
//! `vector.wgsl` carries the same operator for the GPU path; keep both in step.

/// Composites `color`, scaled by `coverage` in `0..=1`, over `pixel`.
pub(crate) fn blend(pixel: &mut [u8], color: [u8; 4], coverage: f32) {
    if pixel[3] == 0 {
        let a = (color[3] as f32 * coverage).round() as u8;
        if a > 0 {
            pixel[0] = color[0];
            pixel[1] = color[1];
            pixel[2] = color[2];
            pixel[3] = a;
        }
        return;
    }
    let source_alpha = color[3] as f32 / 255. * coverage;
    let retained_alpha = pixel[3] as f32 / 255. * (1. - source_alpha);
    let alpha = source_alpha + retained_alpha;
    for channel in 0..3 {
        pixel[channel] = ((color[channel] as f32 * source_alpha
            + pixel[channel] as f32 * retained_alpha)
            / alpha)
            .round() as u8;
    }
    pixel[3] = (alpha * 255.).round() as u8;
}

/// Composites a fully covering `src` pixel over `dst`: `blend` with coverage 1, with fast paths
/// for the transparent and opaque cases that dominate cached frames.
pub(crate) fn over(dst: &mut [u8], src: &[u8]) {
    match src[3] {
        0 => {}
        255 => dst.copy_from_slice(src),
        _ => blend(dst, [src[0], src[1], src[2], src[3]], 1.),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The operator `over` had before it delegated to `blend`, kept as the reference.
    fn previous_over(dst: &mut [u8], src: &[u8]) {
        if src[3] == 0 {
            return;
        }
        if src[3] == 255 || dst[3] == 0 {
            dst.copy_from_slice(src);
            return;
        }
        let sa = src[3] as f32 / 255.;
        let retained = dst[3] as f32 / 255. * (1. - sa);
        let a = sa + retained;
        for k in 0..3 {
            dst[k] = ((src[k] as f32 * sa + dst[k] as f32 * retained) / a).round() as u8;
        }
        dst[3] = (a * 255.).round() as u8;
    }

    #[test]
    fn over_is_unchanged_for_every_alpha_pair_and_a_spread_of_colours() {
        let channels = [0u8, 1, 37, 128, 200, 254, 255];
        for src_alpha in 0..=255u8 {
            for dst_alpha in 0..=255u8 {
                for &c in &channels {
                    let src = [c, 255 - c, c / 2, src_alpha];
                    let dst = [255 - c, c, 90, dst_alpha];
                    let (mut want, mut got) = (dst, dst);
                    previous_over(&mut want, &src);
                    over(&mut got, &src);
                    assert_eq!(got, want, "src {src:?} over dst {dst:?}");
                }
            }
        }
    }
}
