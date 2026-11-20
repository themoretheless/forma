//! The reference CPU raster of a control, the standard every faster path is compared with.

use super::*;

impl Button {
    #[cfg(test)]
    pub(crate) fn raster_reference(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        with_background: bool,
    ) -> Vec<u8> {
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || width as u64 * height as u64 > 8_388_608
            || !scale.is_finite()
            || scale <= 0.
        {
            return Vec::new();
        }
        let mut translated = self.scene.button.clone();
        translated.x -= self.scroll_x;
        translated.y -= self.scroll_y;
        let b = &translated;
        let mut out = vec![0; width as usize * height as usize * 4];
        // Compose children first; background rounding must not clip overflow.
        let base = [0u8; 4];
        let color = self.fill.color();
        let border = self.border.color();
        let border_width = self.template.border.as_ref().map_or(0., |b| b.width);
        let viewport = self.viewport();
        for y in 0..height {
            for x in 0..width {
                let mut rgb = [0.; 3];
                let mut coverage_alpha = 0.;
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    let px = (x as f32 + sx) / scale;
                    let py = (y as f32 + sy) / scale;
                    let in_button = inside(px, py, b);
                    let r = b.radius.min(b.width / 2.).min(b.height / 2.);
                    let dx = (px - (b.x + b.width / 2.)).abs() - (b.width / 2. - r);
                    let dy = (py - (b.y + b.height / 2.)).abs() - (b.height / 2. - r);
                    let distance = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r;
                    let on_border = in_button && distance >= -border_width && border_width > 0.;
                    let fill_alpha = if in_button {
                        color[3] as f32 / 255.
                    } else {
                        0.
                    };
                    let stroke_alpha = if on_border {
                        border[3] as f32 / 255.
                    } else {
                        0.
                    };
                    let alpha = (base[3] as f32 / 255. * (1. - fill_alpha) + fill_alpha)
                        * (1. - stroke_alpha)
                        + stroke_alpha;
                    coverage_alpha += alpha / 4.;
                    for k in 0..3 {
                        let bg = base[k] as f32 * (base[3] as f32 / 255.);
                        let alpha = if in_button {
                            color[3] as f32 / 255.
                        } else {
                            0.
                        };
                        let filled = bg * (1. - alpha) + color[k] as f32 * alpha;
                        let stroke_alpha = if on_border {
                            border[3] as f32 / 255.
                        } else {
                            0.
                        };
                        rgb[k] +=
                            (filled * (1. - stroke_alpha) + border[k] as f32 * stroke_alpha) / 4.;
                    }
                }
                let i = ((y * width + x) * 4) as usize;
                if coverage_alpha > 0. {
                    for k in 0..3 {
                        out[i + k] = (rgb[k] / coverage_alpha).clamp(0., 255.).round() as u8;
                    }
                }
                out[i + 3] = (coverage_alpha * 255.).round() as u8;
                if b.radius == 0. {
                    let outer = rect_area_coverage(x, y, scale, [b.x, b.y, b.width, b.height]);
                    let inner = rect_area_coverage(
                        x,
                        y,
                        scale,
                        [
                            b.x + border_width,
                            b.y + border_width,
                            b.width - 2. * border_width,
                            b.height - 2. * border_width,
                        ],
                    );
                    out[i..i + 4].copy_from_slice(&rect_area_paint(
                        color,
                        border,
                        outer,
                        outer - inner,
                    ));
                }
            }
        }
        let r = b.radius.min(b.width / 2.).min(b.height / 2.);
        if let Some(t) = &self.template.text {
            text::draw_text_dir_region(
                t.family,
                t.direction,
                &mut text::RasterCanvas::full(&mut out, width, height, scale),
                &t.text,
                t.font_size,
                [
                    b.x + r * 0.3,
                    b.y + r * 0.3,
                    b.width - r * 0.6,
                    b.height - r * 0.6,
                ],
                t.color,
            );
        }
        let mut groups: Vec<(Vec<u8>, [f32; 4], f32)> = Vec::new();
        for c in &self.template.content {
            match c {
                template::Content::Clip { bounds, radius } => {
                    groups.push((
                        std::mem::replace(&mut out, vec![0; width as usize * height as usize * 4]),
                        [bounds[0] + b.x, bounds[1] + b.y, bounds[2], bounds[3]],
                        *radius,
                    ));
                }
                template::Content::ClipEnd => {
                    if let Some((mut parent, bounds, radius)) = groups.pop() {
                        for y in 0..height {
                            for x in 0..width {
                                let i = ((y * width + x) * 4) as usize;
                                let fg = out[i + 3] as f32 / 255.
                                    * round_coverage(x, y, scale, bounds, radius);
                                let retained = parent[i + 3] as f32 / 255. * (1. - fg);
                                let alpha = fg + retained;
                                if alpha > 0. {
                                    for k in 0..3 {
                                        parent[i + k] = ((out[i + k] as f32 * fg
                                            + parent[i + k] as f32 * retained)
                                            / alpha)
                                            .round()
                                            as u8;
                                    }
                                }
                                parent[i + 3] = (alpha * 255.).round() as u8;
                            }
                        }
                        out = parent;
                    }
                }
                template::Content::Text { bounds, text: t } => text::draw_text_dir_region(
                    t.family,
                    t.direction,
                    &mut text::RasterCanvas::full(&mut out, width, height, scale),
                    &t.text,
                    t.font_size,
                    [b.x + bounds[0], b.y + bounds[1], bounds[2], bounds[3]],
                    t.color,
                ),
                template::Content::Shape { points, color } => {
                    shape::draw(&mut out, width, height, scale, points, [b.x, b.y], *color)
                }
            }
        }
        // Clip the complete composed child, including glyphs and border, to the Frame.
        for y in 0..height {
            for x in 0..width {
                let px = (x as f32 + 0.5) / scale;
                let py = (y as f32 + 0.5) / scale;
                let i = ((y * width + x) * 4) as usize;
                if let Some(spec) = &self.template.reveal {
                    let p = [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale];
                    let band = reveal::band_coverage(
                        p,
                        self.reveal_bounds(),
                        spec.target_radius.unwrap_or(b.radius),
                        spec.width,
                        scale,
                    );
                    let mut color = self.reveal_paint().color_at(p);
                    color[3] = (color[3] as f32 * band).round() as u8;
                    raster_cache::over(&mut out[i..i + 4], &color);
                }
                let [vx, vy, vw, vh] = [viewport[0], viewport[1], viewport[2], viewport[3]];
                if self.scene.scroll && (px < vx || py < vy || px >= vx + vw || py >= vy + vh) {
                    out[i..i + 4].copy_from_slice(&base);
                }
                if self.scene.scroll && vw > 0. && vh > 0. {
                    let [cw, ch] = scroll_content_size(&self.scene);
                    let vertical = ch > vh
                        && px >= (vx + vw - 5.).max(vx)
                        && px < vx + vw
                        && py >= vy + self.scroll_y / ch * vh
                        && py < vy + (self.scroll_y + vh) / ch * vh;
                    let horizontal = cw > vw
                        && py >= (vy + vh - 5.).max(vy)
                        && py < vy + vh
                        && px >= vx + self.scroll_x / cw * vw
                        && px < vx + (self.scroll_x + vw) / cw * vw;
                    if vertical || horizontal {
                        out[i..i + 4].copy_from_slice(&[110, 130, 170, 255]);
                    }
                }
                let coverage = round_coverage(
                    x,
                    y,
                    scale,
                    [0., 0., self.width(), self.height()],
                    self.scene.radius,
                );
                if with_background {
                    let bg = self.scene.background;
                    let fg = out[i + 3] as f32 / 255.;
                    let retained = bg[3] as f32 / 255.
                        * (if self.scene.clip { 1. } else { coverage })
                        * (1. - fg);
                    let alpha = fg + retained;
                    if alpha > 0. {
                        for k in 0..3 {
                            out[i + k] = ((out[i + k] as f32 * fg + bg[k] as f32 * retained)
                                / alpha)
                                .round() as u8;
                        }
                    }
                    out[i + 3] = (alpha * 255.).round() as u8;
                }
                if self.scene.clip {
                    out[i + 3] = (out[i + 3] as f32 * coverage).round() as u8;
                    if out[i + 3] == 0 {
                        out[i..i + 4].fill(0);
                    }
                }
            }
        }
        out
    }
}
