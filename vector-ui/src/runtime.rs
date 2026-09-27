//! Shared native/WASM scene runtime. Node IDs are local to one loaded snapshot.
//! Controls own interaction; the renderer sees only geometry and paint slots.
use crate::display_list::{DisplayList, RenderScene, STRIDE};
use crate::{markup, round_coverage, round_inside, Button};
use std::{cell::RefCell, sync::Arc};
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Frame,
    Scroll,
    Modal,
    Control,
}
#[derive(Debug)]
pub struct Node {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: NodeKind,
    pub control: Option<usize>,
}
struct GeometryCache {
    scale: f32,
    background: bool,
    scroll: [f32; 2],
    list: Arc<DisplayList>,
}

#[derive(Clone, Copy, PartialEq)]
struct RasterKey {
    width: u32,
    height: u32,
    scale: f32,
    scroll: [f32; 2],
}
struct RasterCache {
    key: RasterKey,
    // Low five bits contain 0..16 frame coverage; high bits contain the
    // scroll viewport/scrollbar classification, shared by all controls.
    frame: Vec<u8>,
    pixels: Vec<u8>,
    native_pixels: Vec<u32>,
    paint: Option<(u32, bool)>,
}

#[wasm_bindgen]
pub struct Runtime {
    scene: markup::Scene,
    controls: Vec<Button>,
    editors: Vec<Option<crate::text_input::Editor>>,
    range_values: Vec<Option<f32>>,
    nodes: Vec<Node>,
    focused: Option<usize>,
    captured: Option<usize>,
    last_event: Option<usize>,
    // Modal layers: each control belongs to the base scene or to one layer.
    // Only the topmost open layer receives input; lower layers stay visible
    // under its backdrop and the base scene keeps its scroll offset.
    modal_of: Vec<Option<usize>>,
    modal_open: Vec<bool>,
    modal_restore: Vec<Option<usize>>,
    dismissals: u32,
    last_dismissed: Option<usize>,
    scroll: [f32; 2],
    revision: u32,
    geometry_revision: u32,
    layout_revision: u32,
    geometry: RefCell<Option<GeometryCache>>,
    raster_cache: RefCell<Option<RasterCache>>,
}

fn templates<'a>(source: &'a str, count: usize) -> Result<Vec<&'a str>, String> {
    let Some(mut rest) = source.strip_prefix("FORMA-TEMPLATES-1\n") else {
        return Ok(vec![source; count]);
    };
    let mut result = Vec::new();
    while !rest.is_empty() {
        let (length, body) = rest
            .split_once('\n')
            .ok_or("Missing template byte length")?;
        let length = length
            .parse::<usize>()
            .map_err(|_| "Invalid template byte length")?;
        if length == 0 || length > body.len() || !body.is_char_boundary(length) {
            return Err("Invalid template boundary".into());
        }
        result.push(&body[..length]);
        rest = &body[length..];
        if result.len() > count {
            return Err("Too many control templates".into());
        }
    }
    if result.len() != count {
        return Err("Control/template count mismatch".into());
    }
    Ok(result)
}
impl Runtime {
    pub fn from_source(source: &str) -> Result<Self, String> {
        Self::from_sources(source, crate::BUTTON_COMPONENT)
    }
    pub fn from_sources(source: &str, component: &str) -> Result<Self, String> {
        let mut scene = markup::parse(source)?;
        let templates = templates(component, scene.buttons.len())?;
        let mut controls = Vec::with_capacity(scene.buttons.len());
        let modal_of: Vec<Option<usize>> = (0..scene.buttons.len())
            .map(|i| scene.modals.iter().position(|m| m.controls.contains(&i)))
            .collect();
        let mut y = scene.padding[0];
        for (index, (spec, component)) in scene.buttons.iter().zip(templates).enumerate() {
            if let Some(m) = modal_of[index] {
                if scene.modals[m].controls.start == index { y = scene.modals[m].y; }
            }
            // Each leaf needs frame metadata and its own spec, not a temporary
            // deep clone of every sibling (quadratic in the control count).
            let leaf = markup::Scene {
                name: scene.name.clone(), width: scene.width, height: scene.height,
                background: scene.background, overflow: scene.overflow.clone(),
                clip: scene.clip, radius: scene.radius, scroll: scene.scroll,
                padding: scene.padding, content_width: scene.content_width,
                content_height: scene.content_height, gap: scene.gap,
                button: spec.clone(), buttons: vec![spec.clone()], modals: Vec::new(),
            };
            let mut control = Button::from_scene(leaf, component)?;
            // Defaults in the component can change height: lay out after linking.
            if !spec.specified.contains("y") {
                control.scene.button.y = y;
            }
            y = control.scene.button.y + control.scene.button.height + scene.gap;
            controls.push(control);
        }
        let base = modal_of.iter().filter(|m| m.is_none()).count();
        scene.content_width = controls[..base]
            .iter()
            .map(|b| b.scene.button.x + b.scene.button.width + scene.padding[1])
            .fold(scene.width, f32::max);
        scene.content_height = controls[..base]
            .iter()
            .map(|b| b.scene.button.y + b.scene.button.height + scene.padding[2])
            .fold(scene.height, f32::max);
        let mut nodes = vec![Node {
            id: 0,
            parent: None,
            children: vec![],
            kind: NodeKind::Frame,
            control: None,
        }];
        let parent = if scene.scroll {
            nodes[0].children.push(1);
            nodes.push(Node {
                id: 1,
                parent: Some(0),
                children: vec![],
                kind: NodeKind::Scroll,
                control: None,
            });
            1
        } else {
            0
        };
        let mut modal_nodes = Vec::new();
        for (index, b) in controls.iter_mut().enumerate() {
            if scene.buttons.len() > 1 || !scene.modals.is_empty() {
                b.scene.clip = false;
                b.scene.scroll = false;
            }
            let parent = match modal_of[index] {
                None => parent,
                Some(m) => {
                    if modal_nodes.len() <= m {
                        let id = nodes.len();
                        nodes[0].children.push(id);
                        nodes.push(Node { id, parent: Some(0), children: vec![], kind: NodeKind::Modal, control: None });
                        modal_nodes.push(id);
                    }
                    modal_nodes[m]
                }
            };
            let id = nodes.len();
            nodes[parent].children.push(id);
            nodes.push(Node {
                id,
                parent: Some(parent),
                children: vec![],
                kind: NodeKind::Control,
                control: Some(index),
            });
        }
        // Empty layers still exist as nodes so hosts can open and close them.
        for _ in modal_nodes.len()..scene.modals.len() {
            let id = nodes.len();
            nodes[0].children.push(id);
            nodes.push(Node { id, parent: Some(0), children: vec![], kind: NodeKind::Modal, control: None });
            modal_nodes.push(id);
        }
        let editors=controls.iter().map(|c|c.template.inputs.first().map(|input|crate::text_input::Editor::new(input.clone(),c.template.content.clone())).transpose()).collect::<Result<Vec<_>,_>>()?;
        let mut runtime=Self {
            range_values: vec![None; controls.len()],
            editors,
            modal_open: scene.modals.iter().map(|m| m.open).collect(),
            modal_restore: vec![None; scene.modals.len()],
            modal_of,
            dismissals: 0,
            last_dismissed: None,
            scene,
            controls,
            nodes,
            focused: None,
            captured: None,
            last_event: None,
            scroll: [0.; 2],
            revision: 0,
            geometry_revision: 0,
            layout_revision: 0,
            geometry: RefCell::new(None),
            raster_cache: RefCell::new(None),
        };
        runtime.refresh_editors();
        Ok(runtime)
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
    /// Re-evaluation keeps gestures attached to the same uniquely keyed control.
    /// Context replacement is cancelled separately by the binding owner.
    pub(crate) fn preserve_binding_interaction(&mut self, previous:&Runtime) {
        self.preserve_interaction(previous);
        let mut previous_keys=std::collections::HashMap::new();
        for (index,control) in previous.controls.iter().enumerate() {
            let key=control.key();
            if !key.is_empty() { previous_keys.entry(key).and_modify(|value| *value=None).or_insert(Some(index)); }
        }
        for (i,c) in self.controls.iter_mut().enumerate() {
            let Some(old_index)=previous_keys.get(&c.key()).copied().flatten() else {continue};
            let old=&previous.controls[old_index];
            if !c.disabled() && c.template.clickable {
                c.down=old.down;c.keyboard=old.keyboard;c.update_colors();
                if previous.captured==Some(old_index){self.captured=Some(i);}
            }
        }
        let layout_changed=self.width()!=previous.width()||self.height()!=previous.height()||self.controls.len()!=previous.controls.len()||self.controls.iter().zip(&previous.controls).any(|(a,b)|a.bounds()!=b.bounds());
        self.revision=previous.revision.wrapping_add(1);
        self.geometry_revision=previous.geometry_revision.wrapping_add(1);
        self.layout_revision=previous.layout_revision.wrapping_add(u32::from(layout_changed));
    }
    /// Apply model text without creating an undo entry. Replacing a binding or
    /// context resets editing history even when the two records contain equal text.
    pub fn set_binding_value(&mut self, index: usize, value: &str, reset: bool) -> Result<(), String> {
        let editor = self.editors.get_mut(index).and_then(Option::as_mut)
            .ok_or_else(|| format!("Control {index} has no editable value"))?;
        if !reset && editor.model.value() == value { return Ok(()); }
        let mut spec = editor.spec.clone(); spec.value = value.into();
        let mut next = crate::text_input::Editor::new(spec, editor.base.clone())?;
        next.reduced_motion(self.controls[index].reduced_motion);
        if !reset {
            let mut caret = editor.model.caret().min(value.len());
            while !value.is_char_boundary(caret) { caret -= 1; }
            next.model.set_selection(caret, caret).map_err(|e| e.to_string())?;
        }
        *editor = next;
        self.refresh_editors();
        Ok(())
    }
    /// Text bindings target a single text primitive; ambiguous rich content is
    /// rejected instead of silently replacing captions or icons as well.
    pub fn set_binding_text(&mut self, index: usize, value: &str) -> Result<(), String> {
        let control = self.controls.get_mut(index).ok_or_else(|| format!("Unknown control {index}"))?;
        let count = usize::from(control.template.text.is_some()) + control.template.content.iter()
            .filter(|c| matches!(c, crate::template::Content::Text { .. })).count();
        if count != 1 || self.editors[index].is_some() { return Err(format!("Control {index}: text binding requires exactly one non-editable text primitive")); }
        let text = if let Some(text) = &mut control.template.text { text } else {
            control.template.content.iter_mut().find_map(|c| match c {
                crate::template::Content::Text { text, .. } => Some(text), _ => None,
            }).unwrap()
        };
        if text.text == value { return Ok(()); }
        text.text = value.into(); control.scene.button.text = value.into();
        *control.raster_cache.borrow_mut() = None;
        *control.display_cache.borrow_mut() = None;
        control.visual_revision = control.visual_revision.wrapping_add(1);
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub fn set_binding_disabled(&mut self, index: usize, disabled: bool) -> Result<(), String> {
        let control = self.controls.get_mut(index).ok_or_else(|| format!("Unknown control {index}"))?;
        if control.scene.button.disabled == disabled { return Ok(()); }
        control.scene.button.disabled = disabled;
        control.template.props.disabled = disabled;
        if disabled { control.down = false; control.keyboard = None; }
        control.update_colors();
        if disabled && self.captured == Some(index) { self.captured = None; }
        if disabled && self.focused == Some(index) { self.set_focus(None); }
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub(crate) fn cancel_binding_interaction(&mut self, index: usize) {
        let before = self.child_revision();
        if let Some(control) = self.controls.get_mut(index) {
            control.down = false; control.keyboard = None; control.update_colors();
        }
        if self.captured == Some(index) { self.captured = None; }
        if let Some(Some(editor)) = self.editors.get_mut(index) { editor.set_preedit(""); }
        self.refresh_editors();
        self.update_revision(before);
    }
    fn refresh_editors(&mut self) {
        for (i,editor) in self.editors.iter_mut().enumerate() {
            let content=if let Some(editor)=editor {
                let focused=self.focused==Some(i)&&!self.controls[i].disabled();
                if !focused {editor.set_preedit("");}
                if !editor.needs_content(focused) {continue;}
                editor.content(focused)
            } else if let Some(range)=&self.controls[i].template.range {
                if self.range_values[i] == Some(range.value) {continue;}
                self.range_values[i] = Some(range.value);
                range.content()
            } else {continue};
            let control=&mut self.controls[i];
            if content!=control.template.content {
                control.template.content=content;
                *control.raster_cache.borrow_mut()=None;
                *control.display_cache.borrow_mut()=None;
                *self.geometry.borrow_mut()=None;
                self.geometry_revision=self.geometry_revision.wrapping_add(1);
                control.visual_revision=control.visual_revision.wrapping_add(1);
                self.revision=self.revision.wrapping_add(1);
            }
        }
    }
    fn active(&self) -> usize {
        self.last_event.or(self.focused).unwrap_or(0)
    }
    fn viewport_rect(&self) -> [f32; 4] {
        let p = self.scene.padding;
        if self.scene.scroll {
            [p[3], p[0], (self.width() - p[1] - p[3]).max(0.), (self.height() - p[0] - p[2]).max(0.)]
        } else {
            [0., 0., self.width(), self.height()]
        }
    }
    /// Topmost open layer, if any. It alone receives pointer and keyboard input.
    fn active_modal_index(&self) -> Option<usize> {
        (0..self.modal_open.len()).rev().find(|&m| self.modal_open[m])
    }
    fn visible(&self, index: usize) -> bool {
        self.modal_of[index].is_none_or(|m| self.modal_open[m])
    }
    /// Reachable controls belong to the active layer; everything else is
    /// blocked by the backdrop and neither hit, hovered nor focusable.
    fn reachable(&self, index: usize) -> bool {
        self.modal_of[index] == self.active_modal_index()
    }
    fn enabled(&self, index: usize) -> bool {
        self.reachable(index) && !self.controls[index].disabled() && self.controls[index].template.clickable
    }
    fn invalidate_layers(&mut self) {
        *self.geometry.borrow_mut() = None;
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.layout_revision = self.layout_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }
    fn cancel_gestures(&mut self) {
        let before = self.child_revision();
        self.captured = None;
        let active = self.active_modal_index();
        for (i, c) in self.controls.iter_mut().enumerate() {
            let blocked = self.modal_of[i] != active;
            c.keyboard = None;
            if blocked { c.pointer_interaction(-1., -1., 3); c.reveal_pointer(0., 0., false); } else { c.down = false; c.update_colors(); }
        }
        self.update_revision(before);
    }
    fn set_focus(&mut self, index: Option<usize>) {
        self.focused = index.filter(|i| self.enabled(*i));
        for (i, c) in self.controls.iter_mut().enumerate() {
            c.focus(Some(i) == self.focused);
        }
        self.refresh_editors();
    }
    fn update_revision(&mut self, previous: u32) {
        if self
            .controls
            .iter()
            .map(Button::visual_revision)
            .fold(0, u32::wrapping_add)
            != previous
        {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    fn child_revision(&self) -> u32 {
        self.controls
            .iter()
            .map(Button::visual_revision)
            .fold(0, u32::wrapping_add)
    }
    pub fn vector_snapshot(&self, scale: f32, background: bool) -> Arc<DisplayList> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
            return self.controls[0].vector_snapshot(scale, background);
        }
        let mut cache = self.geometry.borrow_mut();
        if let Some(c) = cache.as_ref() {
            if c.scale == scale && c.background == background && c.scroll == self.scroll {
                return Arc::clone(&c.list);
            }
        }
        let mut list = DisplayList {
            commands: Vec::new(),
            edges: Vec::new(),
        };
        let frame = [0., 0., self.width(), self.height()];
        if self.scene.clip {
            list.push(frame, self.scene.radius);
        }
        if background {
            list.command(
                0.,
                frame,
                self.scene.background,
                if self.scene.clip {
                    0.
                } else {
                    self.scene.radius
                },
                0.,
            );
        }
        let v = self.viewport_rect();
        if self.scene.scroll {
            list.command(2., [v[0], v[1], v[2], v[3]], [0; 4], 0., 1.);
        }
        // Paint slots are indexed by control, not by emission order: reveal
        // slots follow all fill/border pairs in control order.
        let mut reveal_slots = Vec::with_capacity(self.controls.len());
        let mut reveal_slot = self.controls.len() * 2;
        for c in &self.controls {
            reveal_slots.push(reveal_slot);
            if c.template.reveal.is_some() { reveal_slot += 2; }
        }
        let emit = |list: &mut DisplayList, index: usize| {
            let c = &self.controls[index];
            let child = c.vector_snapshot(scale, false);
            let edge_offset = list.edges.len() / 4;
            for command in child.commands.chunks_exact(STRIDE) {
                let start = list.commands.len();
                list.commands.extend(command);
                let cmd = &mut list.commands[start..];
                if cmd[0] == 1. {
                    cmd[2] += edge_offset as f32;
                }
                if cmd[0] == 4. {
                    cmd[14] = (index * 2) as f32;
                    cmd[15] = (index * 2 + 1) as f32;
                }
                if cmd[0] == 6. {
                    cmd[14] = reveal_slots[index] as f32;
                }
            }
            list.edges.extend(&child.edges);
        };
        for index in 0..self.controls.len() {
            if self.modal_of[index].is_none() { emit(&mut list, index); }
        }
        if self.scene.scroll {
            let [cw, ch] = self.content_size();
            if ch > v[3] && v[3] > 0. {
                list.command(
                    5.,
                    [
                        (v[0] + v[2] - 5.).max(v[0]),
                        v[1] + self.scroll[1] / ch * v[3],
                        5f32.min(v[2]),
                        v[3] * v[3] / ch,
                    ],
                    [110, 130, 170, 255],
                    0.,
                    0.,
                );
            }
            if cw > v[2] && v[2] > 0. {
                list.command(
                    5.,
                    [
                        v[0] + self.scroll[0] / cw * v[2],
                        (v[1] + v[3] - 5.).max(v[1]),
                        v[2] * v[2] / cw,
                        5f32.min(v[3]),
                    ],
                    [110, 130, 170, 255],
                    0.,
                    0.,
                );
            }
            list.pop();
        }
        for (m, modal) in self.scene.modals.iter().enumerate() {
            if !self.modal_open[m] { continue; }
            list.command(0., frame, modal.backdrop, if self.scene.clip { 0. } else { self.scene.radius }, 0.);
            for index in modal.controls.clone() { emit(&mut list, index); }
        }
        if self.scene.clip {
            list.pop();
        }
        let list = Arc::new(list);
        *cache = Some(GeometryCache {
            scale,
            background,
            scroll: self.scroll,
            list: Arc::clone(&list),
        });
        list
    }
    fn content_size(&self) -> [f32; 2] {
        [
            self.scene.content_width - self.scene.padding[1] - self.scene.padding[3],
            self.scene.content_height - self.scene.padding[0] - self.scene.padding[2],
        ]
    }
    fn raster_cached(&self, w: u32, h: u32, s: f32, background: bool)
        -> Option<std::cell::RefMut<'_, RasterCache>>
    {
        if w == 0 || h == 0 || w > 4096 || h > 4096
            || w as u64 * h as u64 > 8_388_608 || !s.is_finite() || s <= 0.
        {
            return None;
        }
        let key = RasterKey { width: w, height: h, scale: s, scroll: self.scroll };
        let len = (w * h) as usize;
        let mut stored = self.raster_cache.borrow_mut();
        let cache = stored.get_or_insert_with(|| RasterCache {
            key, frame: Vec::new(), pixels: Vec::new(), native_pixels: Vec::new(), paint: None,
        });
        if cache.key != key || cache.frame.len() != len {
            cache.key = key;
            cache.frame.resize(len, 0);
            cache.pixels.resize(len * 4, 0);
            cache.paint = None;
            let v = self.viewport_rect();
            let [cw, ch] = self.content_size();
            for y in 0..h {
                for x in 0..w {
                    let px = (x as f32 + 0.5) / s;
                    let py = (y as f32 + 0.5) / s;
                    let mut viewport = 1;
                    if self.scene.scroll {
                        if px < v[0] || py < v[1] || px >= v[0] + v[2] || py >= v[1] + v[3] {
                            viewport = 0;
                        } else {
                            let vertical = ch > v[3]
                                && px >= (v[0] + v[2] - 5.).max(v[0])
                                && py >= v[1] + self.scroll[1] / ch * v[3]
                                && py < v[1] + (self.scroll[1] + v[3]) / ch * v[3];
                            let horizontal = cw > v[2]
                                && py >= (v[1] + v[3] - 5.).max(v[1])
                                && px >= v[0] + self.scroll[0] / cw * v[2]
                                && px < v[0] + (self.scroll[0] + v[2]) / cw * v[2];
                            if vertical || horizontal { viewport = 2; }
                        }
                    }
                    let coverage = round_coverage(x, y, s,
                        [0., 0., self.width(), self.height()], self.scene.radius);
                    cache.frame[(y * w + x) as usize] = (coverage * 16.) as u8 | viewport << 5;
                }
            }
        }
        let paint = (self.revision, background);
        if cache.paint != Some(paint) {
            cache.pixels.fill(0);
            for (i, control) in self.controls.iter().enumerate() {
                if self.modal_of[i].is_none() { control.composite_content(&mut cache.pixels, w, h, s); }
            }
            if self.scene.scroll {
                for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                    match frame >> 5 {
                        0 => p.fill(0),
                        2 => p.copy_from_slice(&[110, 130, 170, 255]),
                        _ => {}
                    }
                }
            }
            // Layers sit above the scroll viewport and its bars; the frame's
            // rounded coverage still trims every layer below.
            for (m, modal) in self.scene.modals.iter().enumerate() {
                if !self.modal_open[m] { continue; }
                if modal.backdrop[3] > 0 {
                    for p in cache.pixels.chunks_exact_mut(4) { crate::raster_cache::over(p, &modal.backdrop); }
                }
                for index in modal.controls.clone() { self.controls[index].composite_content(&mut cache.pixels, w, h, s); }
            }
            for (p, &frame) in cache.pixels.chunks_exact_mut(4).zip(&cache.frame) {
                let coverage = (frame & 31) as f32 / 16.;
                if background {
                    let mut bg = self.scene.background;
                    if !self.scene.clip {
                        bg[3] = (bg[3] as f32 * coverage).round() as u8;
                    }
                    crate::raster_cache::over(&mut bg, p);
                    p.copy_from_slice(&bg);
                }
                if self.scene.clip {
                    p[3] = (p[3] as f32 * coverage).round() as u8;
                    if p[3] == 0 { p.fill(0); }
                }
            }
            cache.paint = Some(paint);
            cache.native_pixels.clear();
        }
        Some(std::cell::RefMut::map(stored, |cache| cache.as_mut().unwrap()))
    }
    fn raster(&self, w: u32, h: u32, s: f32, background: bool) -> Vec<u8> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
            return if background {
                self.controls[0].pixels(w, h, s)
            } else {
                self.controls[0].content_pixels(w, h, s)
            };
        }
        self.raster_cached(w, h, s, background).map_or_else(Vec::new, |cache| cache.pixels.clone())
    }
    pub fn paint_native(
        &self,
        out: &mut [u32],
        w: u32,
        h: u32,
        s: f32,
    ) -> Result<(), &'static str> {
        if self.controls.len() == 1 && self.scene.modals.is_empty() {
            return self.controls[0].paint_native(out, w, h, s);
        }
        if (w as usize).checked_mul(h as usize) != Some(out.len()) {
            return Err("Invalid native buffer size");
        }
        let mut cache = self.raster_cached(w, h, s, true)
            .ok_or("Visible scene exceeds CPU raster budget")?;
        if cache.native_pixels.is_empty() {
            let RasterCache { native_pixels, pixels, .. } = &mut *cache;
            native_pixels.extend(pixels.chunks_exact(4).map(|src| {
                let a = src[3] as u32;
                let rgb = [17u32, 19, 25];
                let c = |k: usize| (src[k] as u32 * a + rgb[k] * (255 - a) + 127) / 255;
                c(0) << 16 | c(1) << 8 | c(2)
            }));
        }
        out.copy_from_slice(&cache.native_pixels);
        Ok(())
    }

}
#[wasm_bindgen]
impl Runtime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::from_source(crate::EXAMPLE).expect("valid built-in scene")
    }
    pub fn load_component(&mut self, source: &str, component: &str) -> Result<(), JsValue> {
        let mut next = Self::from_sources(source, component).map_err(|e| JsValue::from_str(&e))?;
        // JS can keep the same WASM object across load(). Its cache identity is
        // unchanged, so replacing source must not reset version keys to zero.
        next.revision = self.revision.wrapping_add(1);
        next.geometry_revision = self.geometry_revision.wrapping_add(1);
        next.layout_revision = self.layout_revision.wrapping_add(1);
        *self = next;
        Ok(())
    }
    pub fn load(&mut self, source: &str) -> Result<(), JsValue> {
        self.load_component(source, crate::BUTTON_COMPONENT)
    }
    pub fn geometry_revision(&self)->u32 {self.geometry_revision}
    /// Control bounds change on scroll or source replacement. Paint and
    /// text-content changes do not invalidate these bounds.
    pub fn layout_revision(&self)->u32 {self.layout_revision}
    pub fn range_value(&self,i:usize)->f32 {self.controls.get(i).and_then(|c|c.template.range.as_ref()).map_or(f32::NAN,|r|r.value)}
    pub fn range_key(&mut self,key:&str)->bool {
        let Some(i)=self.focused else{return false};
        if self.controls[i].disabled(){return false;}
        let Some(range)=&mut self.controls[i].template.range else{return false};
        range.value=match key {
            "Home"=>0.,"End"=>1.,
            "ArrowLeft"|"ArrowDown"=>(range.value-0.01).max(0.),
            "ArrowRight"|"ArrowUp"=>(range.value+0.01).min(1.),
            "PageDown"=>(range.value-0.1).max(0.),"PageUp"=>(range.value+0.1).min(1.),
            _=>return false,
        };
        self.refresh_editors();true
    }
    pub fn text_editing(&self)->bool {self.focused.is_some_and(|i|self.editors[i].is_some()&&self.enabled(i))}
    pub fn control_editable(&self,i:usize)->bool {self.editors.get(i).is_some_and(Option::is_some)}
    pub fn text_value(&self,i:usize)->String {self.editors.get(i).and_then(Option::as_ref).map_or_else(String::new,|e|e.model.value().into())}
    pub fn text_selected(&self)->String {self.focused.and_then(|i|self.editors[i].as_ref()).map_or_else(String::new,|e|e.model.selected_text().into())}
    pub fn text_insert(&mut self,text:&str)->bool {
        if !self.text_editing(){return false;}
        self.editors[self.focused.unwrap()].as_mut().unwrap().insert(text); self.refresh_editors();true
    }
    pub fn text_preedit(&mut self,text:&str) {
        if self.text_editing(){self.editors[self.focused.unwrap()].as_mut().unwrap().set_preedit(text); self.refresh_editors();}
    }
    pub fn text_key(&mut self,key:&str,shift:bool,command:bool)->bool {
        if !self.text_editing(){return false;}
        let handled=self.editors[self.focused.unwrap()].as_mut().unwrap().key(key,shift,command);self.refresh_editors();handled
    }
    pub fn text_caret_bounds(&self)->Vec<f32> {
        let Some(i)=self.focused else{return vec![]};let Some(e)=&self.editors[i] else{return vec![]};
        let mut rect=e.caret_rect();let b=self.controls[i].bounds_rect();rect[0]+=b[0];rect[1]+=b[1];rect.to_vec()
    }
    pub fn control_count(&self) -> usize {
        self.controls.len()
    }
    pub fn control_key(&self, i: usize) -> String {
        self.controls.get(i).map(Button::key).unwrap_or_default()
    }
    pub fn control_label(&self, i: usize) -> String {
        self.controls.get(i).map(Button::label).unwrap_or_default()
    }
    pub fn control_action(&self, i: usize) -> String {
        self.controls.get(i).map(Button::action).unwrap_or_default()
    }
    pub fn control_disabled(&self, i: usize) -> bool {
        self.controls.get(i).is_none_or(Button::disabled)
    }
    pub fn control_interactive(&self, i: usize) -> bool {
        self.controls.get(i).is_some_and(|c| c.template.clickable)
    }
    pub fn control_clicks(&self, i: usize) -> u32 {
        self.controls.get(i).map_or(0, Button::clicks)
    }
    pub fn control_bounds(&self, i: usize) -> Vec<f32> {
        self.controls.get(i).map(Button::bounds).unwrap_or_default()
    }
    pub fn control_hovered(&self, i: usize) -> bool {
        self.controls.get(i).is_some_and(|b| b.hover)
    }
    pub fn control_focused(&self, i: usize) -> bool {
        self.focused == Some(i)
    }
    /// Layer index of a control, -1 for the base scene.
    pub fn control_modal(&self, i: usize) -> i32 {
        self.modal_of.get(i).copied().flatten().map_or(-1, |m| m as i32)
    }
    /// Hidden by a closed layer or covered by another layer's backdrop.
    pub fn control_blocked(&self, i: usize) -> bool {
        i >= self.controls.len() || !self.visible(i) || !self.reachable(i)
    }
    pub fn modal_key(&self, m: usize) -> String {
        self.scene.modals.get(m).map(|s| s.key.clone()).unwrap_or_default()
    }
    pub fn modal_is_open(&self, m: usize) -> bool {
        self.modal_open.get(m).copied().unwrap_or(false)
    }
    pub fn modal_dismissible(&self, m: usize) -> bool {
        self.scene.modals.get(m).is_some_and(|s| s.dismiss)
    }
    pub fn modal_action(&self, m: usize) -> String {
        self.scene.modals.get(m).and_then(|s| s.action.clone()).unwrap_or_default()
    }
    pub fn modal_bounds(&self, m: usize) -> Vec<f32> {
        self.scene.modals.get(m).map(|s| vec![s.x, s.y, s.width, s.height]).unwrap_or_default()
    }
    pub fn active_modal(&self) -> i32 {
        self.active_modal_index().map_or(-1, |m| m as i32)
    }
    pub fn modal_count(&self) -> usize { self.scene.modals.len() }
    pub fn open_modal(&mut self, m: usize) -> bool {
        if m >= self.modal_open.len() || self.modal_open[m] { return false; }
        self.modal_open[m] = true;
        self.modal_restore[m] = self.focused;
        self.cancel_gestures();
        let first = self.scene.modals[m].controls.clone().find(|&i| self.enabled(i));
        self.set_focus(first);
        self.invalidate_layers();
        true
    }
    pub fn close_modal(&mut self, m: usize) -> bool {
        if m >= self.modal_open.len() || !self.modal_open[m] { return false; }
        self.modal_open[m] = false;
        let restore = self.modal_restore[m].take();
        self.cancel_gestures();
        let focused = self.focused.filter(|&i| self.enabled(i));
        self.set_focus(focused.or(restore));
        self.invalidate_layers();
        true
    }
    /// Escape or a press on the backdrop: closes the layer and records a
    /// dismissal so hosts can run the layer's `dismissed` handler.
    pub fn dismiss_modal(&mut self, m: usize) -> bool {
        if m >= self.scene.modals.len() || !self.scene.modals[m].dismiss || !self.close_modal(m) { return false; }
        self.dismissals = self.dismissals.wrapping_add(1);
        self.last_dismissed = Some(m);
        true
    }
    pub fn dismiss_count(&self) -> u32 { self.dismissals }
    pub fn dismissed_index(&self) -> i32 { self.last_dismissed.map_or(-1, |m| m as i32) }
    pub fn event_index(&self) -> i32 {
        self.last_event.map_or(-1, |i| i as i32)
    }
    pub fn focused_index(&self) -> i32 {
        self.focused.map_or(-1, |i| i as i32)
    }
    pub fn width(&self) -> f32 {
        self.scene.width
    }
    pub fn height(&self) -> f32 {
        self.scene.height
    }
    pub fn clipped(&self) -> bool {
        self.scene.clip
    }
    pub fn frame_radius(&self) -> f32 {
        self.scene.radius
    }
    pub fn overflow(&self) -> String {
        if self.clipped() { "hidden" } else { "visible" }.into()
    }
    pub fn background_color(&self) -> String {
         { let c=self.scene.background; format!("#{:02x}{:02x}{:02x}{:02x}",c[0],c[1],c[2],c[3]) }
    }
    pub fn render_width(&self) -> f32 {
        if self.scene.clip || self.scene.scroll {
            self.width()
        } else {
            self.scene.content_width
        }
    }
    pub fn render_height(&self) -> f32 {
        if self.scene.clip || self.scene.scroll {
            self.height()
        } else {
            self.scene.content_height
        }
    }
    pub fn scrollable(&self) -> bool {
        self.scene.scroll
    }
    pub fn viewport(&self) -> Vec<f32> {
        self.viewport_rect().to_vec()
    }
    pub fn scroll_offset(&self) -> Vec<f32> {
        self.scroll.to_vec()
    }
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        if !self.scene.scroll || !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let v = self.viewport_rect();
        let size = self.content_size();
        let old = self.scroll;
        self.scroll = [
            (old[0] + dx).clamp(0., (size[0] - v[2]).max(0.)),
            (old[1] + dy).clamp(0., (size[1] - v[3]).max(0.)),
        ];
        if old != self.scroll {
            self.revision = self.revision.wrapping_add(1);
            self.layout_revision = self.layout_revision.wrapping_add(1);
        }
        for (i, c) in self.controls.iter_mut().enumerate() {
            if self.modal_of[i].is_some() { continue; }
            c.scroll_x = self.scroll[0];
            c.scroll_y = self.scroll[1];
        }
        self.pointer(-1., -1., 3);
    }
    pub fn hit_index(&self, x: f32, y: f32) -> i32 {
        if !x.is_finite() || !y.is_finite() {
            return -1;
        }
        if self.scene.clip
            && !round_inside(
                x,
                y,
                [0., 0., self.width(), self.height()],
                self.scene.radius,
            )
        {
            return -1;
        }
        let v = self.viewport_rect();
        let outside_viewport = self.scene.scroll && (x < v[0] || y < v[1] || x >= v[0] + v[2] || y >= v[1] + v[3]);
        self.controls
            .iter()
            .enumerate()
            .rev()
            .find(|(i, c)| self.reachable(*i) && !(outside_viewport && self.modal_of[*i].is_none()) && c.hit(x, y))
            .map_or(-1, |(i, _)| i as i32)
    }
    pub fn hit(&self, x: f32, y: f32) -> bool {
        self.hit_index(x, y) >= 0
    }
    pub fn focus(&mut self, value: bool) {
        let before = self.child_revision();
        self.set_focus(if value {
            self.focused
                .or_else(|| (0..self.controls.len()).find(|i| self.enabled(*i)))
        } else {
            None
        });
        if !value {
            self.captured = None;
        }
        self.update_revision(before);
    }
    pub fn focus_control(&mut self, index: usize) -> bool {
        if index >= self.controls.len() || !self.enabled(index) { return false; }
        let before = self.child_revision();
        self.captured = None;
        for c in &mut self.controls { c.down = false; c.keyboard = None; }
        self.set_focus(Some(index));
        self.update_revision(before);
        true
    }
    /// Move within the scene; false lets the browser transfer focus outside it.
    pub fn focus_next(&mut self, reverse: bool) -> bool {
        let mut next = if reverse {
            (0..self.focused.unwrap_or(self.controls.len()))
                .rev()
                .find(|i| self.enabled(*i))
        } else {
            (self.focused.map_or(0, |i| i + 1)..self.controls.len()).find(|i| self.enabled(*i))
        };
        // An open layer traps focus: Tab wraps inside it instead of leaving
        // the scene, so the host never moves focus behind the backdrop.
        if next.is_none() && self.active_modal_index().is_some() {
            next = if reverse {
                (0..self.controls.len()).rev().find(|i| self.enabled(*i))
            } else {
                (0..self.controls.len()).find(|i| self.enabled(*i))
            };
        }
        if next.is_none() {
            return false;
        }
        let before = self.child_revision();
        self.captured = None;
        self.set_focus(next);
        self.update_revision(before);
        true
    }
    /// Keys: 1 = Space, 2 = Enter, 3 = Escape. Escape dismisses the active
    /// layer even while its text field is being edited.
    pub fn key_event(&mut self, key: u8, pressed: bool, repeat: bool) {
        if key == 3 {
            if pressed && !repeat { if let Some(m) = self.active_modal_index() { self.dismiss_modal(m); } }
            return;
        }
        if self.text_editing(){return;}
        if let Some(i) = self.focused {
            let before = self.child_revision();
            let clicks = self.controls[i].clicks();
            self.controls[i].key_event(key, pressed, repeat);
            if self.controls[i].clicks() != clicks {
                self.last_event = Some(i);
            }
            self.update_revision(before);
        }
    }
    pub fn pointer(&mut self, x: f32, y: f32, kind: u8) {
        let before = self.child_revision();
        let active = self.active_modal_index();
        for (i, c) in self.controls.iter_mut().enumerate() { c.reveal_pointer(x, y, kind != 3 && self.modal_of[i] == active); }
        let hit = usize::try_from(self.hit_index(x, y)).ok();
        if kind == 1 {
            for c in &mut self.controls {
                c.keyboard = None;
            }
            self.captured = hit.filter(|i| self.enabled(*i));
            self.set_focus(self.captured);
            if hit.is_none() && x.is_finite() && y.is_finite() {
                if let Some(m) = active {
                    let s = &self.scene.modals[m];
                    let inside = x >= s.x && y >= s.y && x < s.x + s.width && y < s.y + s.height;
                    if !inside { self.dismiss_modal(m); }
                }
            }
        }
        for (i, c) in self.controls.iter_mut().enumerate() {
            let clicks = c.clicks();
            let (px, py) = if hit == Some(i) { (x, y) } else { (-1., -1.) };
            // Only the captured control receives release. Others cannot inherit it.
            let event = if kind == 2 && self.captured != Some(i) {
                0
            } else {
                kind
            };
            c.pointer_interaction(px, py, event);
            if c.clicks() != clicks {
                self.last_event = Some(i);
            }
        }
        if kind==1 || (kind==0 && self.captured.is_some()) {
            if let Some(i)=self.captured {
                let b=self.controls[i].bounds_rect();
                if let Some(editor)=&mut self.editors[i] {editor.hit(x-b[0],y-b[1],kind==0);}
                if let Some(range)=&mut self.controls[i].template.range {range.value=((x-b[0]-8.)/(b[2]-16.).max(1.)).clamp(0.,1.);}
            }
            self.refresh_editors();
        }
        if kind == 2 || kind == 3 {
            self.captured = None;
        }
        self.update_revision(before);
    }
    pub fn activate(&mut self) {
        if let Some(i) = self
            .focused
            .or_else(|| (0..self.controls.len()).find(|i| self.enabled(*i)))
        {
            self.activate_control(i);
        }
    }
    pub fn activate_control(&mut self, i: usize) {
        if i < self.controls.len() && !self.reachable(i) { return; }
        if let Some(c) = self.controls.get_mut(i) {
            let count = c.clicks();
            c.activate();
            if c.clicks() != count {
                self.last_event = Some(i);
            }
        }
    }
    pub fn clicks(&self) -> u32 {
        self.controls
            .iter()
            .map(Button::clicks)
            .fold(0, u32::saturating_add)
    }
    pub fn key(&self) -> String {
        self.controls.get(self.active()).map(Button::key).unwrap_or_default()
    }
    pub fn label(&self) -> String {
        self.controls.get(self.active()).map(Button::label).unwrap_or_default()
    }
    pub fn action(&self) -> String {
        self.controls.get(self.active()).map(Button::action).unwrap_or_default()
    }
    pub fn disabled(&self) -> bool {
        self.controls.iter().all(Button::disabled)
    }
    pub fn bounds(&self) -> Vec<f32> {
        self.controls.get(self.focused.unwrap_or(0)).map(Button::bounds).unwrap_or_else(||vec![0.,0.,self.width(),self.height()])
    }
    pub fn is_focused(&self) -> bool {
        self.focused.is_some()
    }
    pub fn visual_revision(&self) -> u32 {
        self.revision
    }
    pub fn is_animating(&self) -> bool {
        self.controls.iter().any(Button::is_animating) || self.editors.iter().flatten().any(crate::text_input::Editor::is_animating)
    }
    /// Proximity reaches sibling controls even when the pointer is outside all hits.
    pub fn reveal_pointer(&mut self, x: f32, y: f32, present: bool) {
        let before = self.child_revision();
        let active = self.active_modal_index();
        for (i, c) in self.controls.iter_mut().enumerate() { c.reveal_pointer(x, y, present && self.modal_of[i] == active); }
        self.update_revision(before);
    }
    pub fn set_reduced_motion(&mut self, reduced: bool) {
        let before = self.child_revision();
        for c in &mut self.controls { c.set_reduced_motion(reduced); }
        for editor in self.editors.iter_mut().flatten() {editor.reduced_motion(reduced);}
        self.refresh_editors();
        self.update_revision(before);
    }
    pub fn tick(&mut self, dt: f32) -> bool {
        let before = self.child_revision();
        for c in &mut self.controls {
            c.tick(dt);
        }
        for editor in self.editors.iter_mut().flatten() {editor.tick(dt);}
        self.refresh_editors();
        self.update_revision(before);
        self.is_animating()
    }
    pub fn preserve_interaction(&mut self, previous: &Runtime) {
        use std::collections::HashMap;
        // Reconcile only unambiguous identities; never transfer a gesture by index.
        let mut previous_keys: HashMap<&str, Option<usize>> = HashMap::with_capacity(previous.controls.len());
        for (i, c) in previous.controls.iter().enumerate() {
            let key = c.scene.button.key.as_str();
            if !key.is_empty() {
                previous_keys.entry(key).and_modify(|i| *i = None).or_insert(Some(i));
            }
        }
        let matches: Vec<Option<usize>> = {
            let mut current_counts: HashMap<&str, usize> = HashMap::with_capacity(self.controls.len());
            for c in &self.controls { *current_counts.entry(&c.scene.button.key).or_default() += 1; }
            self.controls.iter().map(|c| {
                let key=c.scene.button.key.as_str();
                if current_counts.get(key) == Some(&1) { previous_keys.get(key).copied().flatten() } else { None }
            }).collect()
        };
        self.scroll(previous.scroll[0] - self.scroll[0], previous.scroll[1] - self.scroll[1]);
        // A layer keeps its runtime open/closed state and focus-restore target
        // while its source still declares the same initial state under one key.
        let mut layers_changed = false;
        for (m, spec) in self.scene.modals.iter().enumerate() {
            if spec.key.is_empty() { continue; }
            let Some(old) = previous.scene.modals.iter().position(|p| p.key == spec.key && p.open == spec.open) else { continue };
            if self.modal_open[m] != previous.modal_open[old] { self.modal_open[m] = previous.modal_open[old]; layers_changed = true; }
            self.modal_restore[m] = previous.modal_restore[old].and_then(|target| matches.iter().position(|x| *x == Some(target)));
        }
        self.dismissals = previous.dismissals;
        if layers_changed { self.invalidate_layers(); }
        let before = self.child_revision();
        let mut focused = None;
        for (i, c) in self.controls.iter_mut().enumerate() {
            if let Some(old) = matches[i] {
                c.preserve_interaction(&previous.controls[old]);
                if previous.focused == Some(old) && !c.disabled() && c.template.clickable { focused = Some(i); }
            }
        }
        for i in 0..self.controls.len() {
            if let Some(old)=matches[i] {
                if let (Some(next),Some(prev))=(&mut self.editors[i],&previous.editors[old]) {
                    if (next.spec.value==prev.spec.value || next.spec.value==prev.model.value()) && next.spec.multiline==prev.spec.multiline {
                        next.model=prev.model.clone();next.invalidate_content();
                    }
                }
            }
        }
        self.captured = None;
        self.last_event = None;
        self.set_focus(focused);
        self.update_revision(before);
    }
    pub fn pixels(&self, w: u32, h: u32, s: f32) -> Vec<u8> {
        self.raster(w, h, s, true)
    }
    pub fn content_pixels(&self, w: u32, h: u32, s: f32) -> Vec<u8> {
        self.raster(w, h, s, false)
    }
    pub fn raster_stats(&self) -> Vec<u32> {
        let mut stats = vec![0u32; 2];
        for c in &self.controls {
            let s = [c.raster_builds.get(), c.raster_paints.get()];
            for k in 0..2 {
                stats[k] = stats[k].saturating_add(s[k]);
            }
        }
        stats
    }
    /// Discard reconstructible CPU pixels after the GPU takes over. For multiple
    /// controls, leaf rasters are created only with the root raster, so its
    /// presence also gates this traversal on subsequent GPU frames.
    pub fn release_cpu_cache(&self) {
        let had_root = self.raster_cache.borrow_mut().take().is_some();
        if had_root || self.controls.len() == 1 {
            for control in &self.controls { control.release_cpu_cache(); }
        }
    }
    pub fn gpu_commands(&self, s: f32, b: bool) -> Vec<f32> {
        self.vector_snapshot(s, b).commands.clone()
    }
    pub fn gpu_edges(&self, s: f32, b: bool) -> Vec<f32> {
        self.vector_snapshot(s, b).edges.clone()
    }
    pub fn gpu_tiles(&self, w: u32, h: u32, s: f32, b: bool) -> Vec<u32> {
        if w == 0 || h == 0 || w > 8192 || h > 8192 || !s.is_finite() || s <= 0. {
            return vec![];
        }
        self.vector_snapshot(s, b).tiles(w, h, s)
    }
    pub fn gpu_paints(&self) -> Vec<f32> {
        let mut paints = Vec::new();
        self.gpu_paints_into(&mut paints);
        paints
    }
    pub fn gpu_params(&self, w: u32, h: u32, s: f32, o: bool) -> Vec<f32> {
        let mut params = Vec::new();
        self.gpu_params_into(w, h, s, o, &mut params);
        params
    }
}
impl RenderScene for Runtime {
    fn release_cpu_cache(&self) { Runtime::release_cpu_cache(self); }
    fn vector_snapshot(&self, s: f32, b: bool) -> Arc<DisplayList> {
        Runtime::vector_snapshot(self, s, b)
    }
    fn gpu_params(&self, w: u32, h: u32, s: f32, o: bool) -> Vec<f32> {
        Runtime::gpu_params(self, w, h, s, o)
    }
    fn gpu_paints(&self) -> Vec<f32> {
        Runtime::gpu_paints(self)
    }
    fn gpu_paints_into(&self, out: &mut Vec<f32>) {
        out.clear();
        let reveal_count = self.controls.iter().filter(|c| c.template.reveal.is_some()).count();
        out.reserve(self.controls.len() * 8 + reveal_count * 8);
        for c in &self.controls {
            out.extend(c.fill.color().into_iter().chain(c.border.color()).map(|v| v as f32 / 255.));
        }
        for c in &self.controls {
            if c.template.reveal.is_some() { out.extend(c.reveal_paint().gpu()); }
        }
    }
    fn gpu_params_into(&self, w: u32, h: u32, s: f32, o: bool, out: &mut Vec<f32>) {
        out.clear();
        out.extend_from_slice(&[w as f32, h as f32, s, if o { 1. } else { 0. }, 0., 0., 0., 0., 0., 0., 0., 0.]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TWO:&str="component Demo { Frame { width:120; height:120; padding:10; gap:10; clip:true; Button { key:'first'; width:80; height:35; text:'Первая'; clicked -> actions.first(); } Button { key:'second'; width:80; height:35; text:'Вторая'; clicked -> actions.second(); } } }";
    const VISUAL:&str="component Button { Rectangle { radius:4; background:Brush { color:#ff0000; hover:#00ff00; pressed:#0000ff; transition:0ms; }; Border { width:2; background:Brush { color:#ffffff; focus:#ffff00; }; } PointerArea { clicked -> events.clicked(); } } }";
    fn model() -> Runtime {
        Runtime::from_sources(TWO, VISUAL).unwrap()
    }
    const EDITOR:&str="component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }";
    #[test]
    fn editor_refresh_tracks_mutations_focus_and_preedit_without_touching_siblings() {
        let mut r=Runtime::from_sources(TWO,EDITOR).unwrap();
        let untouched=r.controls[1].vector_snapshot(1.,false);
        r.focus_control(0);r.set_reduced_motion(true);
        let focused=r.vector_snapshot(1.,false);let revision=r.geometry_revision();
        r.tick(16.);
        assert_eq!(r.geometry_revision(),revision);
        assert!(Arc::ptr_eq(&focused,&r.vector_snapshot(1.,false)));
        r.text_preedit("я");let preedit=r.geometry_revision();
        assert!(preedit>revision);
        r.text_preedit("я");assert_eq!(r.geometry_revision(),preedit);
        r.text_insert("🙂");assert!(r.text_value(0).contains('🙂'));
        assert!(r.geometry_revision()>preedit);
        assert!(r.controls[0].template.content.iter().any(|c|matches!(c,crate::template::Content::Text{text,..} if text.text.contains('🙂'))));
        r.text_key("z",false,true);assert_eq!(r.text_value(0),"seed");
        r.text_key("y",false,true);assert!(r.text_value(0).contains('🙂'));
        assert!(Arc::ptr_eq(&untouched,&r.controls[1].vector_snapshot(1.,false)));
        r.text_preedit("pending");r.focus(false);
        assert!(r.editors[0].as_ref().unwrap().preedit.is_empty());
        assert!(!r.is_animating());
    }
    #[test]
    fn editor_reconciliation_uses_the_same_unique_keys_as_control_state() {
        let mut previous=Runtime::from_sources(TWO,EDITOR).unwrap();
        previous.focus_control(0);previous.text_insert("edited");
        let edited=previous.text_value(0);
        let swapped=TWO.replace("'first'","'temporary'").replace("'second'","'first'").replace("'temporary'","'second'");
        let mut next=Runtime::from_sources(&swapped,EDITOR).unwrap();
        next.preserve_interaction(&previous);
        assert_eq!(next.text_value(1),edited);assert_eq!(next.text_value(0),"seed");
        assert!(next.controls[1].template.content.iter().any(|c|matches!(c,crate::template::Content::Text{text,..} if text.text==edited)));
        let unnamed=TWO.replace("key:'first';","").replace("key:'second';","");
        let mut next=Runtime::from_sources(&unnamed,EDITOR).unwrap();
        next.preserve_interaction(&previous);
        assert_eq!(next.text_value(0),"seed");assert_eq!(next.text_value(1),"seed");
        let mut previous=Runtime::from_sources(&unnamed,EDITOR).unwrap();
        previous.focus_control(0);previous.text_insert("ambiguous");
        let mut next=Runtime::from_sources(&unnamed,EDITOR).unwrap();next.preserve_interaction(&previous);
        assert_eq!(next.text_value(0),"seed");assert_eq!(next.text_value(1),"seed");
    }
    #[test]
    fn sibling_states_capture_and_events_are_independent() {
        let mut r = model();
        assert_eq!(r.control_count(), 2);
        assert_eq!(r.nodes()[0].children, vec![1, 2]);
        assert_eq!(r.nodes()[2].parent, Some(0));
        assert_eq!(r.control_bounds(1), vec![10., 55., 80., 35.]);
        r.pointer(30., 25., 0);
        assert!(r.control_hovered(0));
        assert!(!r.control_hovered(1));
        assert_eq!(&r.gpu_paints()[..4], &[0., 1., 0., 1.]);
        assert_eq!(&r.gpu_paints()[8..12], &[1., 0., 0., 1.]);
        r.pointer(30., 25., 1);
        assert_eq!(r.focused_index(), 0);
        r.pointer(30., 65., 0);
        r.pointer(30., 65., 2);
        assert_eq!(r.clicks(), 0);
        assert!(r.control_hovered(1));
        assert!(!r.control_hovered(0));
        r.pointer(30., 65., 1);
        r.pointer(30., 65., 2);
        assert_eq!(r.control_clicks(0), 0);
        assert_eq!(r.control_clicks(1), 1);
        assert_eq!(r.event_index(), 1);
        assert_eq!(r.action(), "actions.second");
        assert!(r.control_focused(1));
        assert!(!r.control_focused(0));
        r.pointer(30., 25., 1);
        r.pointer(-1., -1., 3);
        r.pointer(30., 25., 2);
        assert_eq!(r.clicks(), 1);
    }
    #[test]
    fn keyboard_focus_skips_disabled_and_blur_cancels() {
        let mut r = Runtime::from_sources(
            &TWO.replace("key:'first';", "key:'first'; disabled:true;"),
            VISUAL,
        )
        .unwrap();
        r.focus(true);
        assert_eq!(r.focused_index(), 1);
        assert!(!r.focus_next(false));
        assert!(!r.focus_next(true));
        r.key_event(1, true, false);
        r.key_event(1, true, true);
        r.key_event(1, false, false);
        assert_eq!(r.control_clicks(1), 1);
        r.key_event(2, true, false);
        r.focus(false);
        r.key_event(2, false, false);
        assert_eq!(r.clicks(), 1);
        let mut r = model();
        r.focus(true);
        r.key_event(1, true, false);
        assert!(r.focus_next(false));
        r.key_event(1, false, false);
        assert_eq!(r.clicks(), 0);
        r.key_event(2, true, false);
        r.key_event(2, false, false);
        assert_eq!(r.event_index(), 1);
        assert!(r.focus_next(true));
        assert_eq!(r.focused_index(), 0);
    }
    #[test]
    fn reconciliation_uses_keys_and_never_resumes_an_unfinished_gesture() {
        let mut previous = model();
        previous.activate_control(0);
        previous.activate_control(0);
        previous.activate_control(1);
        assert!(previous.focus_control(1));
        previous.key_event(1, true, false);
        let swapped = TWO.replace("'first'", "'temporary'").replace("'second'", "'first'").replace("'temporary'", "'second'");
        let mut next = Runtime::from_sources(&swapped, VISUAL).unwrap();
        next.preserve_interaction(&previous);
        assert_eq!(next.focused_index(), 0);
        assert_eq!(next.control_clicks(0), 1);
        assert_eq!(next.control_clicks(1), 2);
        next.key_event(1, false, false);
        next.pointer(30., 25., 2);
        assert_eq!(next.clicks(), 3);
        assert!(!next.focus_control(99));
        assert_eq!(next.focused_index(), 0);

        let mut disabled = Runtime::from_sources(&TWO.replace("key:'second';", "key:'second'; disabled:true;"), VISUAL).unwrap();
        disabled.preserve_interaction(&previous);
        assert_eq!(disabled.focused_index(), -1);
        assert_eq!(disabled.control_clicks(1), 1);
        assert!(!disabled.focus_control(1));
        let mut passive = Runtime::from_sources(TWO, "component Button { Rectangle {} }").unwrap();
        passive.preserve_interaction(&previous);
        assert_eq!(passive.focused_index(), -1);
        assert!(!passive.focus_control(1));

        let scroll = TWO.replace("height:120;", "height:70;")
            .replace("Button { key:'first'", "Scroll { Button { key:'first'")
            .replace("} } }", "} } } }");
        let mut previous = Runtime::from_sources(&scroll, VISUAL).unwrap();
        previous.scroll(0., 1000.);
        assert_eq!(previous.scroll_offset(), vec![0., 30.]);
        let mut next = Runtime::from_sources(&scroll.replace("height:70;", "height:95;"), VISUAL).unwrap();
        next.preserve_interaction(&previous);
        assert_eq!(next.scroll_offset(), vec![0., 5.]);
    }
    #[test]
    fn reveal_proximity_updates_sibling_paints_without_hover_or_geometry_changes() {
        let template = VISUAL.replace("Border {", "Reveal { color:#ff8800; } Border {");
        let mut r = Runtime::from_sources(TWO, &template).unwrap();
        let geometry = r.vector_snapshot(1., false);
        let slots: Vec<_> = geometry.commands.chunks_exact(STRIDE).filter(|c| c[0] == 6.).map(|c| c[14]).collect();
        assert_eq!(slots, vec![4., 6.]);
        let old = r.gpu_paints();
        r.reveal_pointer(5., 50., true);
        assert_eq!(r.hit_index(5., 50.), -1);
        assert!(!r.control_hovered(0) && !r.control_hovered(1));
        assert_eq!(r.focused_index(), -1);
        let paints = r.gpu_paints();
        assert_eq!(&old[..16], &paints[..16]);
        assert_ne!(&old[16..24], &paints[16..24]);
        assert_ne!(&old[24..32], &paints[24..32]);
        assert!(Arc::ptr_eq(&geometry, &r.vector_snapshot(1., false)));
        r.pointer(5., 50., 3);
        assert_eq!(r.gpu_paints(), old);
        r.pointer(5., 50., 0);
        assert_eq!(r.gpu_paints(), paints, "native pointer uses the same proximity state");
        assert_eq!(r.clicks(), 0);
    }
    #[test]
    fn passive_controls_are_reported_but_never_focused_or_activated() {
        let passive = VISUAL.replace("PointerArea { clicked -> events.clicked(); }", "");
        let packed = format!(
            "FORMA-TEMPLATES-1\n{}\n{}{}\n{}",
            passive.len(),
            passive,
            VISUAL.len(),
            VISUAL
        );
        let mut r = Runtime::from_sources(TWO, &packed).unwrap();
        assert!(!r.control_interactive(0));
        assert!(r.control_interactive(1));
        assert!(!r.control_interactive(2));
        assert!(!r.control_disabled(0));
        // Passive geometry still participates in designer hit testing.
        assert_eq!(r.hit_index(30., 25.), 0);
        r.focus(true);
        assert_eq!(r.focused_index(), 1);
        assert!(!r.focus_next(true));
        r.key_event(1, true, false);
        r.key_event(1, false, false);
        assert_eq!(r.control_clicks(1), 1);
        r.pointer(30., 25., 1);
        r.pointer(30., 25., 2);
        r.activate_control(0);
        assert_eq!(r.focused_index(), -1);
        assert_eq!(r.control_clicks(0), 0);
        assert_eq!(r.clicks(), 1);
        assert!(r.focus_next(false));
        assert_eq!(r.focused_index(), 1);

        let mut r = Runtime::from_sources(TWO, &passive).unwrap();
        r.focus(true);
        assert_eq!(r.focused_index(), -1);
        assert!(!r.focus_next(false));
        assert!(!r.focus_next(true));
        r.activate();
        r.key_event(2, true, false);
        r.key_event(2, false, false);
        assert_eq!(r.clicks(), 0);

        let mut r = Runtime::from_sources(
            &TWO.replace("key:'second';", "key:'second'; disabled:true;"),
            &packed,
        )
        .unwrap();
        assert!(
            r.control_interactive(1),
            "disabled buttons retain their semantics"
        );
        assert!(r.control_disabled(1));
        assert!(!r.focus_next(false));
        r.activate();
        assert_eq!(r.clicks(), 0);
    }
    #[test]
    fn paint_slots_do_not_rebuild_geometry_and_cpu_pixels_are_independent() {
        let mut r = model();
        let a = r.vector_snapshot(1., true);
        let pixels = r.pixels(120, 120, 1.);
        let shapes: Vec<_> = a
            .commands
            .chunks_exact(STRIDE)
            .filter(|c| c[0] == 4.)
            .collect();
        assert_eq!(&shapes[0][14..16], &[0., 1.]);
        assert_eq!(&shapes[1][14..16], &[2., 3.]);
        r.pointer(30., 65., 0);
        let b = r.vector_snapshot(1., true);
        assert!(Arc::ptr_eq(&a, &b));
        let hovered = r.pixels(120, 120, 1.);
        let pixel = |x: usize, y: usize| (y * 120 + x) * 4;
        assert_eq!(
            &pixels[pixel(30, 25)..pixel(30, 25) + 4],
            &hovered[pixel(30, 25)..pixel(30, 25) + 4]
        );
        assert_eq!(
            &hovered[pixel(30, 65)..pixel(30, 65) + 4],
            &[0, 255, 0, 255]
        );
        assert!(!Arc::ptr_eq(&a, &r.vector_snapshot(1.25, true)));
    }
    #[test]
    fn reverse_painter_order_blocks_click_through_disabled_controls() {
        let source = TWO.replace("key:'second';", "key:'second'; x:10; y:10; disabled:true;");
        let mut r = Runtime::from_sources(&source, VISUAL).unwrap();
        assert_eq!(r.hit_index(30., 25.), 1);
        r.pointer(30., 25., 1);
        r.pointer(30., 25., 2);
        assert_eq!(r.clicks(), 0);
        assert_eq!(r.focused_index(), -1);
    }
    #[test]
    fn scroll_and_rounded_clip_apply_to_all_children() {
        let source = TWO
            .replace("height:120;", "height:70; radius:12;")
            .replace("Button { key:'first'", "Scroll { Button { key:'first'")
            .replace("} } }", "} } } }");
        let mut r = Runtime::from_sources(&source, VISUAL).unwrap();
        assert_eq!(r.nodes()[1].kind, NodeKind::Scroll);
        assert_eq!(r.nodes()[1].children, vec![2, 3]);
        assert_eq!(r.hit_index(30., 65.), -1);
        let before = r.vector_snapshot(1., true);
        r.scroll(0., 1000.);
        assert_eq!(r.scroll_offset(), vec![0., 30.]);
        assert_eq!(r.hit_index(30., 40.), 1);
        assert_eq!(r.hit_index(5., 40.), -1);
        assert!(!Arc::ptr_eq(&before, &r.vector_snapshot(1., true)));
        assert_eq!(r.pixels(120, 70, 1.)[3], 0);
    }
    #[test]
    fn template_framing_is_strict_and_unicode_safe() {
        let other = VISUAL.replace("#ff0000", "#00ffff");
        let packed = format!(
            "FORMA-TEMPLATES-1\n{}\n{}{}\n{}",
            VISUAL.len(),
            VISUAL,
            other.len(),
            other
        );
        let r = Runtime::from_sources(TWO, &packed).unwrap();
        assert_eq!(&r.gpu_paints()[8..12], &[0., 1., 1., 1.]);
        for invalid in [
            "FORMA-TEMPLATES-1\n",
            "FORMA-TEMPLATES-1\n2\nяx",
            "FORMA-TEMPLATES-1\n999\nx",
        ] {
            assert!(Runtime::from_sources(TWO, invalid).is_err());
        }
        assert!(
            Runtime::from_sources(&TWO.replace("key:'second'", "key:'first'"), VISUAL).is_err()
        );
        assert!(Button::from_sources(TWO, VISUAL).is_err());
    }
    const LAYERED:&str="component Demo { Frame { width:200; height:160; padding:10; gap:10; clip:true; Button { key:'base'; width:80; height:30; clicked -> actions.base(); } Modal { key:'ask'; x:20; y:40; width:160; height:80; backdrop:#00000080; dismissed -> actions.cancel(); Button { key:'yes'; width:60; height:24; clicked -> actions.yes(); } Button { key:'no'; width:60; height:24; clicked -> actions.no(); } } } }";
    #[test]
    fn open_layer_blocks_the_base_scene_traps_focus_and_dismisses_on_escape_or_backdrop() {
        let mut r = Runtime::from_sources(LAYERED, VISUAL).unwrap();
        assert_eq!((r.control_count(), r.modal_count(), r.active_modal()), (3, 1, 0));
        assert_eq!(r.nodes().iter().filter(|n| n.kind == NodeKind::Modal).count(), 1);
        assert_eq!(r.nodes()[2].children, vec![3, 4]);
        assert_eq!((r.control_modal(0), r.control_modal(1), r.control_modal(2)), (-1, 0, 0));
        assert!(r.control_blocked(0) && !r.control_blocked(1));
        assert_eq!(r.control_bounds(2), vec![20., 74., 60., 24.]);
        assert_eq!(r.hit_index(30., 20.), -1, "base controls are covered by the backdrop");
        assert_eq!(r.hit_index(30., 50.), 1);
        r.pointer(30., 20., 0);
        assert!(!r.control_hovered(0));
        // A press on the surface beside its controls neither clicks nor dismisses.
        r.pointer(150., 100., 1); r.pointer(150., 100., 2);
        assert_eq!((r.active_modal(), r.dismiss_count(), r.clicks()), (0, 0, 0));
        r.focus(true);
        assert_eq!(r.focused_index(), 1);
        assert!(r.focus_next(false)); assert_eq!(r.focused_index(), 2);
        assert!(r.focus_next(false), "Tab wraps inside the layer instead of leaving the scene");
        assert_eq!(r.focused_index(), 1);
        assert!(r.focus_next(true)); assert_eq!(r.focused_index(), 2);
        r.key_event(2, true, false); r.key_event(2, false, false);
        assert_eq!((r.control_clicks(2), r.action().as_str()), (1, "actions.no"));
        r.activate_control(0);
        assert_eq!(r.control_clicks(0), 0, "hosts cannot activate a covered control");
        r.key_event(3, true, true);
        assert_eq!(r.active_modal(), 0, "key repeat never dismisses");
        r.key_event(3, true, false);
        assert_eq!((r.active_modal(), r.dismiss_count(), r.dismissed_index()), (-1, 1, 0));
        assert_eq!(r.modal_action(0), "actions.cancel");
        assert!(!r.modal_is_open(0) && r.control_blocked(1) && !r.control_blocked(0));
        assert_eq!(r.focused_index(), -1);
        assert_eq!(r.hit_index(30., 20.), 0);
        assert_eq!(r.hit_index(30., 50.), -1, "closed layers are hidden");
        r.key_event(3, true, false);
        assert_eq!(r.dismiss_count(), 1);
        assert!(r.focus_control(0));
        assert!(r.open_modal(0)); assert!(!r.open_modal(0));
        assert_eq!(r.focused_index(), 1, "opening moves focus into the layer");
        assert!(!r.control_hovered(0));
        r.pointer(5., 5., 1);
        assert_eq!((r.active_modal(), r.dismiss_count()), (-1, 2));
        assert_eq!(r.focused_index(), 0, "closing restores the previously focused control");
        r.pointer(5., 5., 2);
        assert_eq!(r.clicks(), 1);

        let fixed = LAYERED.replace("backdrop:#00000080;", "backdrop:#00000080; dismiss:false;");
        let mut r = Runtime::from_sources(&fixed, VISUAL).unwrap();
        assert!(!r.modal_dismissible(0));
        r.key_event(3, true, false); r.pointer(5., 5., 1); r.pointer(5., 5., 2);
        assert_eq!((r.active_modal(), r.dismiss_count()), (0, 0));
        assert!(r.close_modal(0)); assert!(!r.close_modal(0));
        assert_eq!((r.active_modal(), r.dismiss_count()), (-1, 0));
    }
    #[test]
    fn layers_paint_a_backdrop_above_the_base_scene_and_vanish_when_closed() {
        let mut r = Runtime::from_sources(LAYERED, VISUAL).unwrap();
        let open = r.vector_snapshot(1., true);
        let count = |list: &DisplayList, kind: f32| list.commands.chunks_exact(STRIDE).filter(|c| c[0] == kind).count();
        let backdrop = open.commands.chunks_exact(STRIDE).find(|c| c[0] == 0. && c[8] == 0. && c[11] < 1.).expect("backdrop fill");
        assert_eq!(&backdrop[4..8], &[0., 0., 200., 160.]);
        assert_eq!(count(&open, 4.), 3);
        let base_shape = open.commands.chunks_exact(STRIDE).position(|c| c[0] == 4.).unwrap();
        let backdrop_at = open.commands.chunks_exact(STRIDE).position(|c| c[0] == 0. && c[11] < 1.).unwrap();
        assert!(backdrop_at > base_shape, "the backdrop covers the base scene");
        let pixels = r.pixels(200, 160, 1.);
        let at = |p: &[u8], x: usize, y: usize| p[(y * 200 + x) * 4..][..4].to_vec();
        let mut expected = [24, 30, 42, 255];
        crate::raster_cache::over(&mut expected, &[0, 0, 0, 128]);
        assert_eq!(at(&pixels, 150, 100), expected.to_vec());
        assert!(r.close_modal(0));
        let closed = r.vector_snapshot(1., true);
        assert!(!Arc::ptr_eq(&open, &closed));
        assert_eq!(count(&closed, 4.), 1);
        assert_eq!(at(&r.pixels(200, 160, 1.), 150, 100), vec![24, 30, 42, 255]);
        assert_eq!(at(&r.pixels(200, 160, 1.), 30, 50), vec![24, 30, 42, 255], "hidden layer controls are not painted");
        assert!(r.open_modal(0));
        r.focus(false);
        assert_eq!(r.pixels(200, 160, 1.), pixels);
    }
    #[test]
    fn layer_state_survives_reevaluation_under_the_same_key_and_declared_state() {
        let mut previous = Runtime::from_sources(LAYERED, VISUAL).unwrap();
        previous.key_event(3, true, false);
        assert!(!previous.modal_is_open(0));
        let mut next = Runtime::from_sources(LAYERED, VISUAL).unwrap();
        next.preserve_interaction(&previous);
        assert!(!next.modal_is_open(0), "a dismissed layer stays closed while its source is unchanged");
        assert_eq!(next.dismiss_count(), 1);
        let closed_source = LAYERED.replace("backdrop:#00000080;", "backdrop:#00000080; open:false;");
        let mut declared = Runtime::from_sources(&closed_source, VISUAL).unwrap();
        assert!(!declared.modal_is_open(0));
        declared.focus_control(0);
        assert!(declared.open_modal(0));
        let mut next = Runtime::from_sources(&closed_source, VISUAL).unwrap();
        next.preserve_interaction(&declared);
        assert!(next.modal_is_open(0));
        assert_eq!(next.focused_index(), 1);
        assert!(next.close_modal(0));
        assert_eq!(next.focused_index(), 0, "the focus-restore target follows control keys");
        let mut reopened = Runtime::from_sources(LAYERED, VISUAL).unwrap();
        reopened.preserve_interaction(&previous);
        let mut flipped = Runtime::from_sources(&closed_source, VISUAL).unwrap();
        flipped.preserve_interaction(&reopened);
        assert!(!flipped.modal_is_open(0), "a changed declaration wins over runtime state");
    }
    #[test]
    fn scrolling_moves_only_the_base_scene_under_a_layer() {
        let source = "component Demo { Frame { width:200; height:60; padding:10; clip:true; Scroll { Button { key:'base'; width:80; height:200; clicked -> actions.base(); } } Modal { key:'ask'; x:20; y:40; width:160; height:80; Button { key:'yes'; width:60; height:24; clicked -> actions.yes(); } } } }";
        let mut r = Runtime::from_sources(source, VISUAL).unwrap();
        assert!(r.scrollable());
        assert_eq!(r.render_height(), 60.);
        r.close_modal(0);
        r.scroll(0., 1000.);
        assert!(r.scroll_offset()[1] > 0.);
        assert_eq!(r.control_bounds(1), vec![20., 40., 60., 24.]);
        assert!(r.control_bounds(0)[1] < 10.);
        r.open_modal(0);
        assert_eq!(r.hit_index(30., 50.), 1, "layer controls ignore the scroll viewport");
    }
    #[test]
    fn layout_uses_component_defaults_and_single_scene_is_compatible() {
        let r = Runtime::from_sources(
            "component Demo { Frame { gap:8; Button {} Button {} } }",
            "component Button { height:20; Rectangle {} }",
        )
        .unwrap();
        assert_eq!(r.control_bounds(1)[1], 28.);
        let old = Button::new();
        let new = Runtime::new();
        assert_eq!(old.pixels(400, 200, 1.), new.pixels(400, 200, 1.));
        assert_eq!(old.gpu_commands(1., true), new.gpu_commands(1., true));
        assert_eq!(old.gpu_paints(), new.gpu_paints());
    }
}

#[cfg(test)]
mod raster_tests {
    use super::*;
    #[test]
    fn cpu_cache_release_preserves_geometry_and_rebuilds_identical_fallback_pixels() {
        for count in [0, 1, 2] {
            let source = format!("component Demo {{ Frame {{ width:128; height:96; Scroll {{ {} }} }} }}",
                "Button { width:150; height:60; }".repeat(count));
            let runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
            let rgba = runtime.pixels(160, 120, 1.25);
            let mut native = vec![0; 160 * 120];
            runtime.paint_native(&mut native, 160, 120, 1.25).unwrap();
            assert!(runtime.controls.iter().all(|c| c.raster_cache.borrow().is_some()));
            let geometry = runtime.vector_snapshot(1.25, true);
            let revision = runtime.visual_revision();
            for _ in 0..2 {
                runtime.release_cpu_cache();
                assert!(runtime.raster_cache.borrow().is_none());
                assert!(runtime.controls.iter().all(|c| c.raster_cache.borrow().is_none()));
            }
            assert!(Arc::ptr_eq(&geometry, &runtime.vector_snapshot(1.25, true)));
            assert_eq!(runtime.visual_revision(), revision);
            let mut fallback = vec![0; native.len()];
            runtime.paint_native(&mut fallback, 160, 120, 1.25).unwrap();
            assert_eq!(fallback, native);
            assert_eq!(runtime.pixels(160, 120, 1.25), rgba);
        }
    }

    #[test]
    fn layout_versions_track_scroll_and_same_object_load_but_not_paint() {
        let source = "component Demo { Frame { width:128; height:96; Scroll { Button { width:150; height:160; } } } }";
        let mut runtime = Runtime::from_sources(source, TEMPLATE).unwrap();
        let layout = runtime.layout_revision();
        runtime.pointer(30., 25., 0);
        runtime.tick(16.);
        assert_eq!(runtime.layout_revision(), layout);
        runtime.scroll(0., 0.125);
        assert_ne!(runtime.layout_revision(), layout);
        let layout = runtime.layout_revision();
        runtime.scroll(0., 0.);
        assert_eq!(runtime.layout_revision(), layout);
        let visual = runtime.visual_revision();
        let geometry = runtime.geometry_revision();
        runtime.load_component(source, TEMPLATE).unwrap();
        assert_ne!(runtime.layout_revision(), layout);
        assert_ne!(runtime.visual_revision(), visual);
        assert_ne!(runtime.geometry_revision(), geometry);
        assert_eq!(runtime.scroll_offset(), vec![0., 0.]);
    }
    // Previous full-canvas algorithm is an independent oracle for packed leaf
    // compositing, root frame coverage, scrollbars, and background rounding.
    impl Runtime {
    fn full_canvas_reference(&self, w: u32, h: u32, s: f32, background: bool) -> Vec<u8> {
        if self.controls.len() == 1 {
            return if background {
                self.controls[0].pixels(w, h, s)
            } else {
                self.controls[0].content_pixels(w, h, s)
            };
        }
        if w == 0
            || h == 0
            || w > 4096
            || h > 4096
            || w as u64 * h as u64 > 8_388_608
            || !s.is_finite()
            || s <= 0.
        {
            return vec![];
        }
        let mut out = vec![0; w as usize * h as usize * 4];
        for (i, c) in self.controls.iter().enumerate() {
            if self.modal_of[i].is_some() { continue; }
            let pixels = c.content_pixels(w, h, s);
            for (dst, src) in out.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
                crate::raster_cache::over(dst, src);
            }
        }
        let v = self.viewport_rect();
        let [cw, ch] = self.content_size();
        for y in 0..h {
            for x in 0..w {
                let p = &mut out[((y * w + x) * 4) as usize..][..4];
                let px = (x as f32 + 0.5) / s;
                let py = (y as f32 + 0.5) / s;
                if self.scene.scroll {
                    if px < v[0] || py < v[1] || px >= v[0] + v[2] || py >= v[1] + v[3] {
                        p.fill(0);
                    } else {
                        let vertical = ch > v[3]
                            && px >= (v[0] + v[2] - 5.).max(v[0])
                            && py >= v[1] + self.scroll[1] / ch * v[3]
                            && py < v[1] + (self.scroll[1] + v[3]) / ch * v[3];
                        let horizontal = cw > v[2]
                            && py >= (v[1] + v[3] - 5.).max(v[1])
                            && px >= v[0] + self.scroll[0] / cw * v[2]
                            && px < v[0] + (self.scroll[0] + v[2]) / cw * v[2];
                        if vertical || horizontal {
                            p.copy_from_slice(&[110, 130, 170, 255]);
                        }
                    }
                }
            }
        }
        for (m, modal) in self.scene.modals.iter().enumerate() {
            if !self.modal_open[m] { continue; }
            for p in out.chunks_exact_mut(4) { crate::raster_cache::over(p, &modal.backdrop); }
            for i in modal.controls.clone() {
                let pixels = self.controls[i].content_pixels(w, h, s);
                for (dst, src) in out.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
                    crate::raster_cache::over(dst, src);
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let p = &mut out[((y * w + x) * 4) as usize..][..4];
                let coverage = round_coverage(
                    x,
                    y,
                    s,
                    [0., 0., self.width(), self.height()],
                    self.scene.radius,
                );
                if background {
                    let mut bg = self.scene.background;
                    if !self.scene.clip {
                        bg[3] = (bg[3] as f32 * coverage).round() as u8;
                    }
                    crate::raster_cache::over(&mut bg, p);
                    p.copy_from_slice(&bg);
                }
                if self.scene.clip {
                    p[3] = (p[3] as f32 * coverage).round() as u8;
                    if p[3] == 0 {
                        p.fill(0);
                    }
                }
            }
        }
        out
    }
    }

    const TEMPLATE: &str = "component Button { Rectangle { radius:6.3; background:Brush { color:#273f6077; hover:#a4e173bb; transition:100ms; }; Border { width:1.4; background:#e7cf8199; } Reveal { width:2.3; color:#dd8822aa; } Text { text:'Я'; fontSize:16; color:#ffffffa0; } ContentText { x:-5.8; y:-10.2; width:52.1; height:13.7; text:'Overflow Я'; fontSize:12.5; color:#f8aaffb0; } ContentShape { points:'-7 -3 64.2 4.1 72.5 21.8 -4.3 29.2'; color:#fc345a61; } ContentClip { x:3.7; y:8.3; width:48.6; height:25.2; radius:4.2; } ContentShape { points:'-20 -30 70 -30 70 50 -20 50'; color:#3467af80; } ContentClip { x:7.2; y:12.1; width:23.4; height:19.7; radius:3.1; } ContentShape { points:'-20 -30 70 -30 70 50 -20 50'; color:#ddeeff90; } ContentClipEnd {} ContentClipEnd {} PointerArea { clicked -> events.clicked(); } } }";

    fn assert_reference(runtime: &Runtime, width: u32, height: u32, scale: f32) {
        for background in [false, true] {
            let expected = runtime.full_canvas_reference(width, height, scale, background);
            let actual = runtime.raster(width, height, scale, background);
            if expected != actual {
                let first = expected.chunks_exact(4).zip(actual.chunks_exact(4)).position(|(a,b)| a != b).unwrap();
                panic!("pixel ({}, {}) differs at DPI {scale}, background {background}: {:?} != {:?}",
                    first % width as usize, first / width as usize, &expected[first*4..first*4+4], &actual[first*4..first*4+4]);
            }
            assert_eq!(runtime.raster(width, height, scale, background), actual);
            if background {
                let mut native = vec![0; (width * height) as usize];
                runtime.paint_native(&mut native, width, height, scale).unwrap();
                for (&actual, expected) in native.iter().zip(expected.chunks_exact(4)) {
                    let a = expected[3] as u32;
                    let c = |k: usize, bg: u32| (expected[k] as u32 * a + bg * (255 - a) + 127) / 255;
                    assert_eq!(actual, c(0, 17) << 16 | c(1, 19) << 8 | c(2, 25));
                }
            }
        }
    }

    #[test]
    fn packed_multicontrol_matches_full_canvas_for_overflow_clips_scroll_and_dpi() {
        for mode in ["overflow:visible;", "clip:true;", "scroll"] {
            let (props, open, close) = if mode == "scroll" { ("clip:true;", "Scroll {", "}") } else { (mode, "", "") };
            let source = format!("component Demo {{ Frame {{ width:83.7; height:62.4; padding:5.3; radius:8.1; background:#10203080; {props} {open} Button {{ x:3.7; y:4.2; width:32.2; height:19.7; }} Button {{ x:23.6; y:39.7; width:76.3; height:39.2; }} {close} }} }}");
            let mut runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
            for scale in [1., 1.25, 2.] {
                assert_reference(&runtime, 176, 152, scale);
                runtime.pointer(12.2, 9.4, 0);
                runtime.tick(23.);
                assert_reference(&runtime, 176, 152, scale);
                runtime.pointer(45.7, 43.2, 0);
                runtime.tick(39.);
                runtime.scroll(12.3, 15.7);
                assert_reference(&runtime, 176, 152, scale);
                // Crop/expand the physical host independently of logical layout.
                assert_reference(&runtime, 57, 44, scale);
            }
        }
    }

    #[test]
    fn layers_composite_above_the_scroll_viewport_and_match_the_reference() {
        let source = "component Demo { Frame { width:120; height:90; padding:10; clip:true; radius:6; background:#10203080; Scroll { Button { x:0; y:0; width:80; height:200; } } Modal { key:'m'; backdrop:#ff000080; Button { key:'top'; x:2; y:2; width:40; height:20; } Button { key:'edge'; x:90; y:60; width:60; height:40; } } } }";
        let mut runtime = Runtime::from_sources(source, TEMPLATE).unwrap();
        for scale in [1., 1.25] {
            assert_reference(&runtime, 150, 113, scale);
            runtime.pointer(12., 12., 0);
            runtime.tick(23.);
            runtime.scroll(0., 15.);
            assert_reference(&runtime, 150, 113, scale);
        }
        let open = runtime.content_pixels(120, 90, 1.);
        assert!(open[(5 * 120 + 5) * 4 + 3] > 0, "layer content paints over the padding outside the viewport");
        runtime.close_modal(0);
        assert_reference(&runtime, 150, 113, 1.);
        assert_eq!(runtime.content_pixels(120, 90, 1.)[(5 * 120 + 5) * 4 + 3], 0);
        assert_ne!(runtime.content_pixels(120, 90, 1.), open);
    }

    #[test]
    fn composite_cache_refreshes_after_text_editing_and_validates_native_size() {
        let source = "component Demo { Frame { width:180; height:90; padding:0; Button { width:80; height:30; } Button { width:80; height:30; } } }";
        let template = "component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }";
        let mut runtime = Runtime::from_sources(source, template).unwrap();
        let before = runtime.pixels(180, 90, 1.);
        runtime.focus_control(0);
        runtime.text_insert("Я");
        let after = runtime.pixels(180, 90, 1.);
        assert_ne!(before, after);
        assert_reference(&runtime, 180, 90, 1.);
        assert!(runtime.paint_native(&mut [0; 3], 180, 90, 1.).is_err());
        assert!(runtime.paint_native(&mut [], 0, 0, 1.).is_err());
        assert!(runtime.pixels(u32::MAX, 4, 1.).is_empty());
    }

    #[test]
    fn empty_frame_raster_and_native_have_no_child_access() {
        for content in ["", "Scroll {}"] {
            let source = format!("component Demo {{ Frame {{ width:24; height:18; padding:2; radius:3.3; background:#27486980; {content} }} }}");
            let runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
            assert_eq!(runtime.control_count(), 0);
            assert_reference(&runtime, 40, 30, 1.25);
            assert_reference(&runtime, 12, 10, 2.);
        }
    }

    #[test]
    fn warmed_composite_refreshes_when_reconciliation_only_changes_reveal() {
        let source = "component Demo { Frame { width:180; height:100; padding:10; gap:10; Button { key:'a'; width:100; height:30; } Button { key:'b'; width:100; height:30; } } }";
        let template = "component Button { Rectangle { radius:6; background:#112233; Reveal { width:2; color:#ffaabb; } PointerArea { clicked -> events.clicked(); } } }";
        let mut previous = Runtime::from_sources(source, template).unwrap();
        previous.reveal_pointer(9., 40., true);
        let expected = previous.pixels(180, 100, 1.);
        let mut current = Runtime::from_sources(source, template).unwrap();
        let before = current.pixels(180, 100, 1.);
        assert_ne!(before, expected);
        let revision = current.visual_revision();
        current.preserve_interaction(&previous);
        assert!(current.visual_revision() > revision);
        assert_eq!(current.pixels(180, 100, 1.), expected);
        assert_reference(&current, 180, 100, 1.);
    }
}
