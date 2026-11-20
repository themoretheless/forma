//! A single control: its template, interaction state, animation and the scene it renders.

#[cfg(test)]
use crate::shape;
use crate::{
    BUTTON_COMPONENT, EXAMPLE, display_list, markup, raster_cache, reveal, scroll_content_size,
    template, text,
};
use wasm_bindgen::prelude::*;

use crate::geometry::{inside, round_inside};
#[cfg(test)]
use crate::geometry::{rect_area_coverage, rect_area_paint, round_coverage};

mod animation;
mod gpu;
#[cfg(test)]
mod raster;
mod vector;

pub(crate) use animation::Animation;

#[wasm_bindgen]
pub struct Button {
    pub(crate) scene: markup::Scene,
    pub(crate) template: std::sync::Arc<template::Template>,
    pub(crate) fill: Animation,
    pub(crate) border: Animation,
    pub(crate) reveal: reveal::State,
    pub(crate) reduced_motion: bool,
    pub(crate) hover: bool,
    pub(crate) down: bool,
    pub(crate) keyboard: Option<u8>,
    pub(crate) focused: bool,
    pub(crate) clicks: u32,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
    pub(crate) raster_cache: std::cell::RefCell<Option<raster_cache::Cache>>,
    pub(crate) visual_revision: u32,
    pub(crate) raster_builds: std::cell::Cell<u32>,
    pub(crate) raster_paints: std::cell::Cell<u32>,
    pub(crate) display_cache: std::cell::RefCell<Option<display_list::CachedList>>,
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
    pub(crate) fn from_scene(
        scene: markup::Scene,
        props: &markup::ButtonSpec,
        component: &str,
    ) -> Result<Self, String> {
        let template = template::parse_cached(component, props)?;
        Ok(Self::from_template(scene, template))
    }

    pub(crate) fn from_template(
        mut scene: markup::Scene,
        template: std::sync::Arc<template::Template>,
    ) -> Self {
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
        Self {
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
        }
    }
    pub fn load_source(&mut self, source: &str) -> Result<(), String> {
        // Commit only a valid scene; editor keeps the last good preview on error.
        let mut next = Self::from_source(source)?;
        next.clicks = self.clicks;
        next.visual_revision = self.visual_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    pub(crate) fn update_colors(&mut self) {
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
    pub(crate) fn reveal_bounds(&self) -> [f32; 4] {
        self.control_view().reveal_bounds()
    }
    pub(crate) fn reveal_paint(&self) -> reveal::Paint {
        self.template
            .reveal
            .as_ref()
            .map_or_else(reveal::Paint::default, |spec| {
                self.reveal
                    .paint(spec, self.reveal_bounds(), self.disabled(), self.focused)
            })
    }
    pub(crate) fn pointer_interaction(&mut self, x: f32, y: f32, kind: u8) {
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
            let [content_width, content_height] = scroll_content_size(&self.scene);
            self.scroll_x = (self.scroll_x + dx).clamp(0., (content_width - v[2]).max(0.));
            self.scroll_y = (self.scroll_y + dy).clamp(0., (content_height - v[3]).max(0.));
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
        self.preserve_interaction_matched(previous);
    }

    pub(crate) fn preserve_interaction_matched(&mut self, previous: &Button) {
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
}
