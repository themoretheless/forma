use wasm_bindgen::prelude::*;
mod blend;
pub mod frame_stats;
mod geometry;
pub mod markup;
pub mod runtime;
pub use runtime::Runtime;
pub mod binding;
pub mod control_models_wasm;
pub mod control_state;
pub mod display_list;
pub mod form_document;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod mcp;
mod raster_cache;
pub mod reveal;
mod shape;
pub mod template;
pub mod text;
pub mod text_input;
#[cfg(feature = "gpu")]
pub use gpu_compute;
#[cfg(feature = "gpu")]
pub use gpu_shader;
#[cfg(all(feature = "gpu", target_os = "linux"))]
extern crate wgpu_linux as wgpu;
#[cfg(all(feature = "gpu", not(target_os = "linux")))]
pub use wgpu as gpu_runtime;
#[cfg(all(feature = "gpu", target_os = "linux"))]
pub use wgpu_linux as gpu_runtime;
#[cfg(feature = "native")]
pub mod native_accessibility;
#[cfg(feature = "native")]
pub mod native_clipboard;
#[cfg(feature = "native")]
#[doc(hidden)]
pub mod native_gpu;
#[cfg(feature = "native")]
pub mod native_window;

pub const EXAMPLE: &str = include_str!("../examples/Button.ui");
pub const TWO_BUTTONS_EXAMPLE: &str = include_str!("../examples/TwoButtons.ui");
pub const BUTTON_COMPONENT: &str = include_str!("../examples/Button.component.ui");

/// Logical single-line metrics from the same font as the vector text renderer.
#[wasm_bindgen]
pub fn text_metrics(value: &str, font_size: f32) -> Vec<f32> {
    text::measure_line(value, font_size).to_vec()
}

/// Shaped single-line metrics (`[width, height, glyph_count]`) using ligature/BiDi/fallback shaping.
#[wasm_bindgen]
pub fn shaped_text_metrics(value: &str, font_size: f32) -> Vec<f32> {
    let w = text::measure_shaped_text(value, font_size);
    let [_, h] = text::measure_line(value, font_size);
    let glyphs = text::shape_text(value, font_size).len() as f32;
    vec![w, h, glyphs]
}

use geometry::{inside, round_inside};
#[cfg(test)]
use geometry::{rect_area_coverage, rect_area_paint, round_coverage};

#[wasm_bindgen]
pub struct Button {
    scene: markup::Scene,
    template: std::sync::Arc<template::Template>,
    fill: Animation,
    border: Animation,
    reveal: reveal::State,
    reduced_motion: bool,
    hover: bool,
    down: bool,
    keyboard: Option<u8>,
    focused: bool,
    clicks: u32,
    scroll_x: f32,
    scroll_y: f32,
    raster_cache: std::cell::RefCell<Option<raster_cache::Cache>>,
    visual_revision: u32,
    raster_builds: std::cell::Cell<u32>,
    raster_paints: std::cell::Cell<u32>,
    display_cache: std::cell::RefCell<Option<display_list::CachedList>>,
}

struct Animation {
    from: [f32; 4],
    current: [f32; 4],
    target: [f32; 4],
    elapsed: f32,
    duration: f32,
}
impl Animation {
    fn new(color: [u8; 4]) -> Self {
        let c = color.map(|x| x as f32);
        Self {
            from: c,
            current: c,
            target: c,
            elapsed: 0.,
            duration: 0.,
        }
    }
    fn target(&mut self, color: [u8; 4], duration: f32) {
        let c = color.map(|x| x as f32);
        if c != self.target {
            self.from = self.current;
            self.target = c;
            self.duration = duration;
            self.elapsed = 0.;
            if duration == 0. {
                self.current = c;
            }
        }
    }
    fn active(&self) -> bool {
        self.current != self.target && self.elapsed < self.duration
    }
    fn tick(&mut self, dt: f32) -> bool {
        if !self.active() {
            return false;
        }
        self.elapsed = (self.elapsed + dt).min(self.duration);
        let t = self.elapsed / self.duration;
        let eased = t * t * (3. - 2. * t);
        for k in 0..4 {
            self.current[k] = self.from[k] + (self.target[k] - self.from[k]) * eased;
        }
        if t == 1. {
            self.current = self.target;
        }
        self.active()
    }
    fn color(&self) -> [u8; 4] {
        self.current.map(|x| x.clamp(0., 255.) as u8)
    }
}

impl Button {
    pub(crate) fn control_view(&self) -> display_list::ControlView<'_> {
        display_list::ControlView {
            scene: &self.scene,
            template: &self.template,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
        }
    }
    pub(crate) fn viewport_rect(&self) -> [f32; 4] {
        self.control_view().viewport_rect()
    }
    pub(crate) fn bounds_rect(&self) -> [f32; 4] {
        let b = &self.scene.button;
        [b.x - self.scroll_x, b.y - self.scroll_y, b.width, b.height]
    }
    pub fn from_source(source: &str) -> Result<Self, String> {
        Self::from_sources(source, BUTTON_COMPONENT)
    }
    pub fn from_sources(source: &str, component: &str) -> Result<Self, String> {
        let mut scene = markup::parse(source)?;
        if scene.buttons.len() != 1 {
            return Err("Use Runtime for multiple controls".into());
        }
        let props = std::mem::take(&mut scene.button);
        Self::from_scene(scene, &props, component)
    }
    // `props` is the spec exactly as the document declared it. The template resolves it,
    // and the control owns that resolved copy, so the scene's own spec would be parsed
    // and thrown away: passing it by reference keeps the loader from a full deep clone.
    fn from_scene(
        mut scene: markup::Scene,
        props: &markup::ButtonSpec,
        component: &str,
    ) -> Result<Self, String> {
        let template = template::parse_cached(component, props)?;
        scene.button = template.props.clone();
        scene.content_width = (scene.button.x + scene.button.width + scene.padding[1])
            .min(f32::MAX)
            .max(scene.width);
        scene.content_height = (scene.button.y + scene.button.height + scene.padding[2])
            .min(f32::MAX)
            .max(scene.height);
        scene.button.radius = template.radius;
        let fill = Animation::new(template.fill.as_ref().map_or([0; 4], |b| {
            if scene.button.disabled {
                b.disabled.unwrap_or(b.color)
            } else {
                b.color
            }
        }));
        let border = Animation::new(template.border.as_ref().map_or([0; 4], |b| {
            if scene.button.disabled {
                b.brush.disabled.unwrap_or(b.brush.color)
            } else {
                b.brush.color
            }
        }));
        Ok(Self {
            scene,
            template,
            fill,
            border,
            reveal: reveal::State::default(),
            reduced_motion: false,
            hover: false,
            down: false,
            keyboard: None,
            focused: false,
            clicks: 0,
            scroll_x: 0.,
            scroll_y: 0.,
            raster_cache: std::cell::RefCell::new(None),
            visual_revision: 0,
            raster_builds: std::cell::Cell::new(0),
            raster_paints: std::cell::Cell::new(0),
            display_cache: std::cell::RefCell::new(None),
        })
    }
    pub fn load_source(&mut self, source: &str) -> Result<(), String> {
        // Commit only a valid scene; editor keeps the last good preview on error.
        let mut next = Self::from_source(source)?;
        next.clicks = self.clicks;
        next.visual_revision = self.visual_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    fn update_colors(&mut self) {
        let before = (self.fill.color(), self.border.color());
        let target = |brush: &template::Brush| {
            if self.scene.button.disabled {
                brush.disabled.unwrap_or(brush.color)
            } else if (self.down && self.hover) || self.keyboard.is_some() {
                brush.pressed.unwrap_or(brush.color)
            } else if self.hover {
                brush
                    .hover
                    .or(if self.focused { brush.focus } else { None })
                    .unwrap_or(brush.color)
            } else if self.focused {
                brush.focus.unwrap_or(brush.color)
            } else {
                brush.color
            }
        };
        if let Some(b) = &self.template.fill {
            self.fill.target(
                target(b),
                if self.reduced_motion {
                    0.
                } else {
                    b.duration_ms
                },
            );
        }
        if let Some(b) = &self.template.border {
            self.border.target(
                target(&b.brush),
                if self.reduced_motion {
                    0.
                } else {
                    b.brush.duration_ms
                },
            );
        }
        if before != (self.fill.color(), self.border.color()) {
            self.visual_revision = self.visual_revision.wrapping_add(1);
        }
    }
    fn reveal_bounds(&self) -> [f32; 4] {
        self.control_view().reveal_bounds()
    }
    fn reveal_paint(&self) -> reveal::Paint {
        self.template
            .reveal
            .as_ref()
            .map_or_else(reveal::Paint::default, |spec| {
                self.reveal
                    .paint(spec, self.reveal_bounds(), self.disabled(), self.focused)
            })
    }
    fn pointer_interaction(&mut self, x: f32, y: f32, kind: u8) {
        self.hover = self.hit(x, y);
        if self.disabled() || !self.template.clickable {
            self.down = false;
            return;
        }
        match kind {
            1 => self.down = self.hover,
            2 => {
                if self.down && self.hover {
                    self.activate();
                }
                self.down = false;
            }
            3 => {
                self.down = false;
                self.hover = false;
            }
            _ => {}
        }
        self.update_colors();
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Button {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::from_source(EXAMPLE).expect("built-in markup must be valid")
    }
    pub fn load(&mut self, source: &str) -> Result<(), JsValue> {
        self.load_source(source).map_err(|e| JsValue::from_str(&e))
    }
    pub fn load_component(&mut self, source: &str, component: &str) -> Result<(), JsValue> {
        let mut next = Self::from_sources(source, component).map_err(|e| JsValue::from_str(&e))?;
        next.clicks = self.clicks;
        next.visual_revision = self.visual_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    pub fn width(&self) -> f32 {
        self.scene.width
    }
    pub fn height(&self) -> f32 {
        self.scene.height
    }
    pub fn overflow(&self) -> String {
        if self.scene.clip {
            "hidden".into()
        } else {
            "visible".into()
        }
    }
    pub fn clipped(&self) -> bool {
        self.scene.clip
    }
    pub fn frame_radius(&self) -> f32 {
        self.scene.radius
    }
    pub fn render_width(&self) -> f32 {
        if !self.scene.clip && !self.scene.scroll {
            self.scene.content_width
        } else {
            self.scene.width
        }
    }
    pub fn render_height(&self) -> f32 {
        if !self.scene.clip && !self.scene.scroll {
            self.scene.content_height
        } else {
            self.scene.height
        }
    }
    pub fn scrollable(&self) -> bool {
        self.scene.scroll
    }
    pub fn viewport(&self) -> Vec<f32> {
        self.viewport_rect().to_vec()
    }
    pub fn scroll_offset(&self) -> Vec<f32> {
        vec![self.scroll_x, self.scroll_y]
    }
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        if self.scene.scroll && dx.is_finite() && dy.is_finite() {
            let before = (self.scroll_x, self.scroll_y);
            let v = self.viewport_rect();
            self.scroll_x =
                (self.scroll_x + dx).clamp(0., (self.scene.button.width - v[2]).max(0.));
            self.scroll_y =
                (self.scroll_y + dy).clamp(0., (self.scene.button.height - v[3]).max(0.));
            if before != (self.scroll_x, self.scroll_y) {
                self.visual_revision = self.visual_revision.wrapping_add(1);
            }
            self.down = false;
            self.hover = false;
            self.update_colors();
        }
    }
    pub fn label(&self) -> String {
        let mut labels: Vec<String> = self.template.text.iter().map(|t| t.text.clone()).collect();
        for c in &self.template.content {
            if let template::Content::Text { text, .. } = c {
                labels.push(text.text.clone());
            }
        }
        if labels.is_empty() {
            self.scene.button.text.clone()
        } else {
            labels.join(" ")
        }
    }
    pub fn is_focused(&self) -> bool {
        self.focused
    }
    pub fn preserve_interaction(&mut self, previous: &Button) {
        if self.scene.button.key != previous.scene.button.key {
            return;
        }
        let before_reveal = self.reveal_paint();
        self.clicks = previous.clicks;
        self.focused = previous.focused && !self.disabled() && self.template.clickable;
        self.hover = previous.hover;
        self.reduced_motion = previous.reduced_motion;
        self.reveal = previous.reveal.clone();
        if let Some(spec) = &self.template.reveal {
            if self.disabled() {
                self.reveal.pointer(None, self.reveal_bounds(), spec);
            } else {
                self.reveal.rebase(self.reveal_bounds(), spec);
            }
        } else {
            self.reveal = reveal::State::default();
            self.reveal.reduced_motion(self.reduced_motion);
        }
        // A replaced subtree must not inherit an unfinished click gesture.
        self.down = false;
        self.keyboard = None;
        self.update_colors();
        if before_reveal != self.reveal_paint() {
            self.visual_revision = self.visual_revision.wrapping_add(1);
        }
    }
    pub fn key(&self) -> String {
        self.scene.button.key.clone()
    }
    pub fn action(&self) -> String {
        if self.template.clickable {
            self.scene.button.action.clone().unwrap_or_default()
        } else {
            String::new()
        }
    }
    pub fn disabled(&self) -> bool {
        self.scene.button.disabled
    }
    pub fn bounds(&self) -> Vec<f32> {
        self.bounds_rect().to_vec()
    }
    pub fn hit(&self, x: f32, y: f32) -> bool {
        let v = self.viewport_rect();
        (!self.scene.clip
            || round_inside(
                x,
                y,
                [0., 0., self.width(), self.height()],
                self.scene.radius,
            ))
            && (!self.scene.scroll
                || (x >= v[0] && y >= v[1] && x < v[0] + v[2] && y < v[1] + v[3]))
            && inside(x + self.scroll_x, y + self.scroll_y, &self.scene.button)
    }
    pub fn focus(&mut self, focused: bool) {
        let before = self.reveal_paint();
        self.focused = focused;
        if !focused {
            self.down = false;
            self.keyboard = None;
        }
        self.update_colors();
        if before != self.reveal_paint() {
            self.visual_revision = self.visual_revision.wrapping_add(1);
        }
    }
    /// Shared web/native keyboard gesture: 1 = Space, 2 = Enter.
    /// Repeats and unmatched releases cannot activate; blur cancels the gesture.
    pub fn key_event(&mut self, key: u8, pressed: bool, repeat: bool) {
        if !matches!(key, 1 | 2) || repeat {
            return;
        }
        if !self.focused || self.disabled() || !self.template.clickable {
            self.keyboard = None;
            return;
        }
        if pressed {
            if self.keyboard.is_none() {
                self.keyboard = Some(key);
                self.down = false;
            }
        } else if self.keyboard == Some(key) {
            self.keyboard = None;
            self.activate();
        }
        self.update_colors();
    }
    pub fn visual_revision(&self) -> u32 {
        self.visual_revision
    }
    pub fn raster_stats(&self) -> Vec<u32> {
        vec![self.raster_builds.get(), self.raster_paints.get()]
    }
    pub fn is_animating(&self) -> bool {
        self.fill.active() || self.border.active() || self.reveal.active()
    }
    pub fn tick(&mut self, delta_ms: f32) -> bool {
        let before = (self.fill.color(), self.border.color(), self.reveal_paint());
        let dt = if delta_ms.is_finite() {
            delta_ms.clamp(0., 1000.)
        } else {
            0.
        };
        self.fill.tick(dt);
        self.border.tick(dt);
        self.reveal.tick(dt);
        if before != (self.fill.color(), self.border.color(), self.reveal_paint()) {
            self.visual_revision = self.visual_revision.wrapping_add(1);
        }
        self.is_animating()
    }
    /// Proximity only: does not change hover, focus, pointer capture or actions.
    pub fn reveal_pointer(&mut self, x: f32, y: f32, present: bool) {
        let before = self.reveal_paint();
        if let Some(spec) = &self.template.reveal {
            let point = (present && !self.disabled() && spec.width > 0. && spec.color[3] > 0)
                .then_some([x, y]);
            self.reveal.pointer(point, self.reveal_bounds(), spec);
        }
        if before != self.reveal_paint() {
            self.visual_revision = self.visual_revision.wrapping_add(1);
        }
    }
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        let before = (self.fill.color(), self.border.color(), self.reveal_paint());
        self.reduced_motion = reduced;
        self.reveal.reduced_motion(reduced);
        if reduced {
            self.fill.current = self.fill.target;
            self.border.current = self.border.target;
        }
        if before != (self.fill.color(), self.border.color(), self.reveal_paint()) {
            self.visual_revision = self.visual_revision.wrapping_add(1);
        }
    }
    pub fn pointer(&mut self, x: f32, y: f32, kind: u8) {
        self.reveal_pointer(x, y, kind != 3);
        self.pointer_interaction(x, y, kind);
    }
    pub fn activate(&mut self) {
        if !self.disabled() && self.template.clickable {
            self.clicks = self.clicks.saturating_add(1);
        }
    }
    pub fn clicks(&self) -> u32 {
        self.clicks
    }
    pub fn pixels(&self, width: u32, height: u32, scale: f32) -> Vec<u8> {
        self.raster_cached(width, height, scale, true)
    }
    /// Studio paints its rounded artboard background separately from children.
    /// This avoids clipping overflowing children just to round the background.
    pub fn content_pixels(&self, width: u32, height: u32, scale: f32) -> Vec<u8> {
        self.raster_cached(width, height, scale, false)
    }
    pub fn background_color(&self) -> String {
        let c = self.scene.background;
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
    #[cfg(test)]
    fn raster_reference(
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
            text::draw_text(
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
                template::Content::Text { bounds, text: t } => text::draw_text(
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
                    let ch = self.scene.button.height;
                    let cw = self.scene.button.width;
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

impl display_list::DisplayList {
    pub fn build(model: &Button, scale: f32, background: bool) -> Self {
        let mut scratch = text::VectorScratch::prewarm();
        Self::build_hinted(model.control_view(), scale, background, None, &mut scratch)
    }
}

impl Button {
    fn ensure_vector_list(
        &self,
        scale: f32,
        background: bool,
        hints: Option<display_list::ListHints>,
        text_scratch: &mut text::VectorScratch,
    ) {
        let mut cache = self.display_cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|c| c.scale != scale || c.background != background)
        {
            *cache = Some(display_list::CachedList {
                scale,
                background,
                list: std::sync::Arc::new(display_list::DisplayList::build_hinted(
                    self.control_view(),
                    scale,
                    background,
                    hints,
                    text_scratch,
                )),
            });
        }
    }

    /// Borrowed view retained for existing Rust callers and WASM serialization.
    pub fn vector_list(
        &self,
        scale: f32,
        background: bool,
    ) -> std::cell::Ref<'_, display_list::DisplayList> {
        let mut scratch = text::VectorScratch::prewarm();
        self.ensure_vector_list(scale, background, None, &mut scratch);
        std::cell::Ref::map(self.display_cache.borrow(), |c| {
            c.as_ref().unwrap().list.as_ref()
        })
    }

    /// Immutable shared geometry identity. Cloning a cached handle does not allocate
    /// or copy commands/edges. Holding the previous handle prevents address reuse,
    /// allowing renderers to detect replacement with `Arc::ptr_eq` without an ABA bug.
    /// Color animations and scroll keep the same snapshot; DPI, background and loading
    /// another source/component replace it. Future geometry setters must invalidate
    /// the display cache instead of mutating a published snapshot.
    pub fn vector_snapshot(
        &self,
        scale: f32,
        background: bool,
    ) -> std::sync::Arc<display_list::DisplayList> {
        let mut scratch = text::VectorScratch::prewarm();
        self.vector_snapshot_hinted(scale, background, None, &mut scratch)
    }

    /// Same snapshot with a capacity head start and a shared flattening buffer. A scene that
    /// asks for hundreds of near-identical control lists uses these to allocate each list at
    /// its final size instead of doubling into it.
    pub(crate) fn vector_snapshot_hinted(
        &self,
        scale: f32,
        background: bool,
        hints: Option<display_list::ListHints>,
        text_scratch: &mut text::VectorScratch,
    ) -> std::sync::Arc<display_list::DisplayList> {
        self.ensure_vector_list(scale, background, hints, text_scratch);
        std::sync::Arc::clone(&self.display_cache.borrow().as_ref().unwrap().list)
    }

    fn cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
        sparse: bool,
    ) -> Option<std::cell::RefMut<'_, raster_cache::Cache>> {
        if !raster_cache::valid_raster_size(width, height) || !scale.is_finite() || scale <= 0. {
            return None;
        }
        let geometry = raster_cache::GeometryKey {
            width,
            height,
            scale,
            scroll: [self.scroll_x, self.scroll_y],
            sparse,
        };
        let paint = raster_cache::PaintKey {
            fill: self.fill.color(),
            border: self.border.color(),
            reveal: self.reveal_paint(),
            background,
        };
        let mut stored = self.raster_cache.borrow_mut();
        if stored.as_ref().is_none_or(|cache| cache.key != geometry) {
            *stored = Some(raster_cache::Cache::build(self.control_view(), geometry));
            self.raster_builds
                .set(self.raster_builds.get().wrapping_add(1));
        }
        let cache = stored.as_mut().unwrap();
        if cache.paint != Some(paint) {
            cache.paint(&self.scene, paint);
            self.raster_paints
                .set(self.raster_paints.get().wrapping_add(1));
        }
        Some(std::cell::RefMut::map(stored, |cache| {
            cache.as_mut().unwrap()
        }))
    }

    pub(crate) fn raster_cached(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
    ) -> Vec<u8> {
        self.cached(width, height, scale, background, false)
            .map_or_else(Vec::new, |cache| cache.pixels.clone())
    }

    /// Copy RGBA output into caller-owned storage, retaining its allocation.
    pub fn paint_rgba(
        &self,
        dst: &mut [u8],
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
    ) -> Result<(), &'static str> {
        if (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(dst.len())
        {
            return Err("Invalid RGBA buffer size");
        }
        let cache = self
            .cached(width, height, scale, background, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        dst.copy_from_slice(&cache.pixels);
        Ok(())
    }

    /// Composite a leaf without allocating/copying a transparent full canvas.
    /// Root frame clipping and scrollbars are applied once by Runtime.
    pub(crate) fn composite_content(&self, dst: &mut [u8], width: u32, height: u32, scale: f32) {
        debug_assert!(!self.scene.clip && !self.scene.scroll);
        let Some(cache) = self.cached(width, height, scale, false, true) else {
            return;
        };
        let mut pixels = cache.pixels.as_slice();
        for span in &cache.spans {
            let bytes = span.len() * 4;
            for (dst, src) in dst[span.start * 4..span.end * 4]
                .chunks_exact_mut(4)
                .zip(pixels[..bytes].chunks_exact(4))
            {
                raster_cache::over(dst, src);
            }
            pixels = &pixels[bytes..];
        }
    }

    /// Paint into the host-owned XRGB buffer without a full-window RGBA copy.
    /// The current runtime has fixed logical layout: resizing the host does not
    /// change it. Keep that scene cached at native DPI and crop/copy on resize.
    /// This intentionally does not stretch content or lower quality while dragging.
    pub fn paint_native(
        &self,
        dst: &mut [u32],
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<(), &'static str> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or("Native buffer size overflow")?;
        if width == 0 || height == 0 || len != dst.len() || !scale.is_finite() || scale <= 0. {
            return Err("Invalid native buffer or DPI");
        }
        let [right, bottom] = self.paint_extent();
        let mut rw = (right * scale).ceil().max(1.) as u32;
        let mut rh = (bottom * scale).ceil().max(1.) as u32;
        // Do not allocate an unbounded offscreen scene. For oversized content,
        // retain only the visible portion (the existing raster safety limit).
        if !raster_cache::valid_raster_size(rw, rh) {
            rw = rw.min(width);
            rh = rh.min(height);
        }
        let mut cache = self
            .cached(rw, rh, scale, true, false)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let raster_cache::Cache {
                native_pixels,
                pixels,
                ..
            } = &mut *cache;
            native_pixels.reserve(pixels.len() / 4);
            native_pixels.extend(pixels.chunks_exact(4).map(|p| {
                let a = p[3] as u32;
                if a == 255 {
                    ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32)
                } else if a == 0 {
                    0x111319
                } else {
                    let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
                    (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25)
                }
            }));
        }
        let pixels = &cache.native_pixels;
        let copy_width = width.min(rw) as usize;
        let copy_height = height.min(rh) as usize;
        for y in 0..copy_height {
            let row = &mut dst[y * width as usize..(y + 1) * width as usize];
            row[..copy_width]
                .copy_from_slice(&pixels[y * rw as usize..y * rw as usize + copy_width]);
            row[copy_width..].fill(0x111319);
        }
        dst[copy_height * width as usize..].fill(0x111319);
        Ok(())
    }

    // Conservative bounds include overflowing template primitives, not just the
    // Button box. Clip/Scroll restrict them to the root viewport. Text already
    // clips itself to its own bounds; an extra pixel covers fractional edges.
    fn paint_extent(&self) -> [f32; 2] {
        let mut extent = [self.width(), self.height()];
        if self.scene.clip || self.scene.scroll {
            return extent;
        }
        let b = &self.scene.button;
        let x = b.x - self.scroll_x;
        let y = b.y - self.scroll_y;
        extent[0] = extent[0].max(x + b.width);
        extent[1] = extent[1].max(y + b.height);
        for content in &self.template.content {
            match content {
                template::Content::Text { bounds, .. } => {
                    extent[0] = extent[0].max(x + bounds[0] + bounds[2] + 1.);
                    extent[1] = extent[1].max(y + bounds[1] + bounds[3] + 1.);
                }
                template::Content::Shape { points, .. } => {
                    for p in points {
                        extent[0] = extent[0].max(x + p[0]);
                        extent[1] = extent[1].max(y + p[1]);
                    }
                }
                _ => {}
            }
        }
        extent
    }
}

#[wasm_bindgen]
impl Button {
    pub fn gpu_commands(&self, scale: f32, background: bool) -> Vec<f32> {
        self.vector_list(scale, background).commands.clone()
    }
    pub fn gpu_edges(&self, scale: f32, background: bool) -> Vec<f32> {
        self.vector_list(scale, background).edges.clone()
    }
    pub fn gpu_tiles(&self, width: u32, height: u32, scale: f32, background: bool) -> Vec<u32> {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || !scale.is_finite()
            || scale <= 0.
        {
            return vec![];
        }
        self.vector_list(scale, background).tiles(
            width,
            height,
            scale,
            [self.scroll_x, self.scroll_y],
        )
    }
    // viewport/scale/opaque output, scroll; animated fill and border. Geometry is static.
    pub fn gpu_params(&self, width: u32, height: u32, scale: f32, opaque: bool) -> Vec<f32> {
        use display_list::RenderScene as _;
        let mut params = Vec::with_capacity(display_list::PARAMS_LEN);
        self.gpu_params_into(width, height, scale, opaque, &mut params);
        params
    }
    pub fn release_cpu_cache(&self) {
        self.raster_cache.borrow_mut().take();
    }
    pub fn gpu_paints(&self) -> Vec<f32> {
        use display_list::RenderScene as _;
        let mut paints = Vec::with_capacity(if self.template.reveal.is_some() {
            16
        } else {
            8
        });
        self.gpu_paints_into(&mut paints);
        paints
    }
}

impl display_list::RenderScene for Button {
    fn release_cpu_cache(&self) {
        Button::release_cpu_cache(self);
    }
    fn gpu_scroll(&self) -> [f32; 2] {
        [self.scroll_x, self.scroll_y]
    }
    fn vector_snapshot(&self, s: f32, b: bool) -> std::sync::Arc<display_list::DisplayList> {
        Button::vector_snapshot(self, s, b)
    }
    fn gpu_params(&self, w: u32, h: u32, s: f32, o: bool) -> Vec<f32> {
        Button::gpu_params(self, w, h, s, o)
    }
    fn gpu_paints(&self) -> Vec<f32> {
        Button::gpu_paints(self)
    }
    fn gpu_params_into(
        &self,
        width: u32,
        height: u32,
        scale: f32,
        opaque: bool,
        out: &mut Vec<f32>,
    ) {
        out.clear();
        out.extend([
            width as f32,
            height as f32,
            scale,
            if opaque { 1. } else { 0. },
        ]);
        out.extend(self.fill.color().map(|v| v as f32 / 255.));
        out.extend(self.border.color().map(|v| v as f32 / 255.));
        out.extend([self.scroll_x, self.scroll_y, 0., 0.]);
    }
    fn gpu_paints_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend(
            self.fill
                .color()
                .into_iter()
                .chain(self.border.color())
                .map(|v| v as f32 / 255.),
        );
        if self.template.reveal.is_some() {
            out.extend(self.reveal_paint().gpu());
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(target_os = "android")]
pub mod android_jni {
    use std::sync::{Mutex, OnceLock};

    struct AndroidState {
        runtime: crate::Runtime,
        backend: Option<&'static str>,
        chooser: Option<crate::Runtime>,
        last_action: String,
    }

    static ANDROID_STATE: OnceLock<Mutex<AndroidState>> = OnceLock::new();

    fn chooser_ui_for_width(vw: f32, vh: f32) -> String {
        let w = vw.max(320.0).round() as i32;
        let h = vh.max(360.0).round() as i32;
        let pad_x = 24;
        let btn_w = (w - pad_x * 2).max(200);
        format!(
            r#"component BackendLauncher {{
    Frame {{
        width: {w};
        height: {h};
        padding: 88 {pad_x} 24 {pad_x};
        gap: 20;
        background: #141923;
        radius: 0;
        clip: true;
        Button {{
            key: 'choose_egui';
            width: {btn_w};
            height: 76;
            text: '[1] Запустить на egui (Immediate-Mode)';
            background: #5b8af5;
            hoverBackground: #7ba3ff;
            pressedBackground: #426cd4;
            clicked -> actions.choose_egui();
        }}
        Button {{
            key: 'choose_svet';
            width: {btn_w};
            height: 76;
            text: '[2] Запустить на svet (Reactive GPU/UMA)';
            background: #58c496;
            hoverBackground: #76dfb1;
            pressedBackground: #3fa177;
            clicked -> actions.choose_svet();
        }}
    }}
}}"#
        )
    }

    fn two_buttons_ui_for_width(vw: f32, vh: f32) -> String {
        let w = vw.max(320.0).round() as i32;
        let h = vh.max(360.0).round() as i32;
        let pad_x = 24;
        let btn_w = (w - pad_x * 2).max(200);
        format!(
            r#"component TwoButtons {{
    Frame {{
        width: {w};
        height: {h};
        padding: 88 {pad_x} 24 {pad_x};
        gap: 20;
        background: #181e2a;
        radius: 0;
        clip: true;
        Button {{
            key: 'first';
            width: {btn_w};
            height: 76;
            text: 'Первая кнопка';
            clicked -> actions.first();
        }}
        Button {{
            key: 'second';
            width: {btn_w};
            height: 76;
            text: 'Вторая кнопка';
            background: #aad9bd;
            hoverBackground: #c7eed7;
            pressedBackground: #78b493;
            clicked -> actions.second();
        }}
    }}
}}"#
        )
    }

    fn get_state() -> &'static Mutex<AndroidState> {
        ANDROID_STATE.get_or_init(|| {
            let src = two_buttons_ui_for_width(412.0, 915.0);
            let runtime = crate::Runtime::from_sources(&src, crate::BUTTON_COMPONENT)
                .expect("compile TwoButtons.ui for Android");
            let csrc = chooser_ui_for_width(412.0, 915.0);
            let chooser = crate::Runtime::from_sources(&csrc, crate::BUTTON_COMPONENT).ok();
            Mutex::new(AndroidState {
                runtime,
                backend: None,
                chooser,
                last_action: String::from("Выберите бэкенд: [1] egui / [2] svet"),
            })
        })
    }

    #[repr(C)]
    pub struct JNINativeInterface {
        _reserved: [*const core::ffi::c_void; 230],
        pub get_direct_buffer_address:
            unsafe extern "C" fn(*mut JNIEnv, *mut core::ffi::c_void) -> *mut u8,
        pub get_direct_buffer_capacity:
            unsafe extern "C" fn(*mut JNIEnv, *mut core::ffi::c_void) -> i64,
    }

    pub type JNIEnv = *const JNINativeInterface;

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativeInit(
        _env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
        backend_mode: i32,
    ) {
        let mut st = get_state().lock().unwrap();
        let src = two_buttons_ui_for_width(412.0, 915.0);
        st.runtime = crate::Runtime::from_sources(&src, crate::BUTTON_COMPONENT).unwrap();
        match backend_mode {
            1 => {
                st.backend = Some("egui");
                st.chooser = None;
                st.last_action = "Активен бэкенд: egui (Immediate-Mode)".into();
            }
            2 => {
                st.backend = Some("svet");
                st.chooser = None;
                st.last_action = "Активен бэкенд: svet (Reactive GPU/UMA)".into();
            }
            _ => {
                st.backend = None;
                let csrc = chooser_ui_for_width(412.0, 915.0);
                st.chooser = crate::Runtime::from_sources(&csrc, crate::BUTTON_COMPONENT).ok();
                st.last_action = "Выберите бэкенд: [1] egui / [2] svet".into();
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativePointer(
        _env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
        x: f32,
        y: f32,
        kind: i32,
    ) -> i32 {
        let mut st = get_state().lock().unwrap();
        if let Some(chooser) = &mut st.chooser {
            let before = chooser.clicks();
            chooser.pointer(x, y, kind as u8);
            if chooser.clicks() != before {
                let chosen = if chooser.key() == "choose_egui" {
                    "egui"
                } else {
                    "svet"
                };
                st.backend = Some(chosen);
                st.chooser = None;
                st.last_action = format!("Активен бэкенд: {chosen}");
                return if chosen == "egui" { 101 } else { 102 };
            }
            return 0;
        }
        let before = st.runtime.clicks();
        st.runtime.pointer(x, y, kind as u8);
        if st.runtime.clicks() != before {
            st.last_action = format!("{} → {}", st.runtime.key(), st.runtime.action());
            return st.runtime.clicks() as i32;
        }
        0
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativeToggleBackend(
        _env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
    ) -> i32 {
        let mut st = get_state().lock().unwrap();
        let next = match st.backend {
            Some("egui") => "svet",
            _ => "egui",
        };
        st.backend = Some(next);
        st.chooser = None;
        if next == "egui" { 1 } else { 2 }
    }

    #[no_mangle]
    pub unsafe extern "C" fn Java_dev_forma_app_MainActivity_nativeRender(
        env: *mut JNIEnv,
        _this: *mut core::ffi::c_void,
        buffer_obj: *mut core::ffi::c_void,
        width: i32,
        height: i32,
        scale: f32,
        dt_ms: f32,
    ) {
        if env.is_null() || buffer_obj.is_null() || width <= 0 || height <= 0 {
            return;
        }
        let iface = &*(*env);
        let ptr = (iface.get_direct_buffer_address)(env, buffer_obj) as *mut u32;
        let cap = (iface.get_direct_buffer_capacity)(env, buffer_obj);
        let needed = (width as usize) * (height as usize);
        if ptr.is_null() || (cap as usize) < needed * 4 {
            return;
        }
        let slice = core::slice::from_raw_parts_mut(ptr, needed);
        let mut st = get_state().lock().unwrap();
        let logical_w = (width as f32 / scale.max(1.0)).round();
        let logical_h = (height as f32 / scale.max(1.0)).round();
        if (st.runtime.render_width() - logical_w).abs() > 2.0 {
            let next_src = two_buttons_ui_for_width(logical_w, logical_h);
            if let Ok(mut next_rt) =
                crate::Runtime::from_sources(&next_src, crate::BUTTON_COMPONENT)
            {
                next_rt.preserve_interaction(&st.runtime);
                st.runtime = next_rt;
            }
        }
        if let Some(chooser) = &st.chooser {
            if (chooser.render_width() - logical_w).abs() > 2.0 {
                let next_csrc = chooser_ui_for_width(logical_w, logical_h);
                if let Ok(mut next_c) =
                    crate::Runtime::from_sources(&next_csrc, crate::BUTTON_COMPONENT)
                {
                    next_c.preserve_interaction(chooser);
                    st.chooser = Some(next_c);
                }
            }
        }
        let is_egui = st.backend == Some("egui");
        let rt = if let Some(chooser) = &mut st.chooser {
            chooser
        } else {
            &mut st.runtime
        };
        rt.tick(dt_ms.clamp(0.0, 100.0));
        let _ = rt.paint_native(slice, width as u32, height as u32, scale.max(1.0));
        // Convert 0x00RRGGBB (softbuffer ARGB8888) to 0xFFRRGGBB for Android ARGB_8888 Bitmap
        for px in slice.iter_mut() {
            let r = (*px >> 16) & 0xFF;
            let g = (*px >> 8) & 0xFF;
            let b = *px & 0xFF;
            // Android Bitmap.copyPixelsFromBuffer with ARGB_8888 on little-endian expects ABGR (0xAABBGGRR)
            *px = 0xFF00_0000 | (b << 16) | (g << 8) | r;
        }
        if is_egui && height >= 6 {
            let bar_h = (4.0 * scale.max(1.0)) as usize;
            for y in 0..bar_h.min(height as usize) {
                let row = y * (width as usize);
                for x in 0..(width as usize) {
                    slice[row + x] = 0xFFF5_8A5B; // #5b8af5 in ABGR
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native_document_graph;
