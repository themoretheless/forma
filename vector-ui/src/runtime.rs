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
    scroll: [f32; 2],
    revision: u32,
    geometry_revision: u32,
    geometry: RefCell<Option<GeometryCache>>,
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
        let mut y = scene.padding[0];
        for (spec, component) in scene.buttons.iter().zip(templates) {
            // Each leaf needs frame metadata and its own spec, not a temporary
            // deep clone of every sibling (quadratic in the control count).
            let leaf = markup::Scene {
                name: scene.name.clone(), width: scene.width, height: scene.height,
                background: scene.background, overflow: scene.overflow.clone(),
                clip: scene.clip, radius: scene.radius, scroll: scene.scroll,
                padding: scene.padding, content_width: scene.content_width,
                content_height: scene.content_height, gap: scene.gap,
                button: spec.clone(), buttons: vec![spec.clone()],
            };
            let mut control = Button::from_scene(leaf, component)?;
            // Defaults in the component can change height: lay out after linking.
            if !spec.specified.contains("y") {
                control.scene.button.y = y;
            }
            y = control.scene.button.y + control.scene.button.height + scene.gap;
            controls.push(control);
        }
        scene.content_width = controls
            .iter()
            .map(|b| b.scene.button.x + b.scene.button.width + scene.padding[1])
            .fold(scene.width, f32::max);
        scene.content_height = controls
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
        for (index, b) in controls.iter_mut().enumerate() {
            if scene.buttons.len() > 1 {
                b.scene.clip = false;
                b.scene.scroll = false;
            }
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
        let editors=controls.iter().map(|c|c.template.inputs.first().map(|input|crate::text_input::Editor::new(input.clone(),c.template.content.clone())).transpose()).collect::<Result<Vec<_>,_>>()?;
        let mut runtime=Self {
            range_values: vec![None; controls.len()],
            editors,
            scene,
            controls,
            nodes,
            focused: None,
            captured: None,
            last_event: None,
            scroll: [0.; 2],
            revision: 0,
            geometry_revision: 0,
            geometry: RefCell::new(None),
        };
        runtime.refresh_editors();
        Ok(runtime)
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
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
    fn enabled(&self, index: usize) -> bool {
        !self.controls[index].disabled() && self.controls[index].template.clickable
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
        if self.controls.len() == 1 {
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
        let mut reveal_slot = self.controls.len() * 2;
        for (index, c) in self.controls.iter().enumerate() {
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
                    cmd[14] = reveal_slot as f32;
                }
            }
            list.edges.extend(&child.edges);
            if c.template.reveal.is_some() { reveal_slot += 2; }
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
    fn raster(&self, w: u32, h: u32, s: f32, background: bool) -> Vec<u8> {
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
        for c in &self.controls {
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
    pub fn paint_native(
        &self,
        out: &mut [u32],
        w: u32,
        h: u32,
        s: f32,
    ) -> Result<(), &'static str> {
        if self.controls.len() == 1 {
            return self.controls[0].paint_native(out, w, h, s);
        }
        let p = self.pixels(w, h, s);
        if p.is_empty() || p.len() != out.len() * 4 {
            return Err("Visible scene exceeds CPU raster budget");
        }
        for (dst, src) in out.iter_mut().zip(p.chunks_exact(4)) {
            let a = src[3] as u32;
            let rgb = [17u32, 19, 25];
            let c = |k: usize| (src[k] as u32 * a + rgb[k] * (255 - a) + 127) / 255;
            *dst = c(0) << 16 | c(1) << 8 | c(2);
        }
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
        let next = Self::from_sources(source, component).map_err(|e| JsValue::from_str(&e))?;
        *self = next;
        Ok(())
    }
    pub fn load(&mut self, source: &str) -> Result<(), JsValue> {
        self.load_component(source, crate::BUTTON_COMPONENT)
    }
    pub fn geometry_revision(&self)->u32 {self.geometry_revision}
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
        self.controls[0].background_color()
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
        }
        for c in &mut self.controls {
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
        if self.scene.scroll && (x < v[0] || y < v[1] || x >= v[0] + v[2] || y >= v[1] + v[3]) {
            return -1;
        }
        self.controls
            .iter()
            .rposition(|c| c.hit(x, y))
            .map_or(-1, |i| i as i32)
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
        let next = if reverse {
            (0..self.focused.unwrap_or(self.controls.len()))
                .rev()
                .find(|i| self.enabled(*i))
        } else {
            (self.focused.map_or(0, |i| i + 1)..self.controls.len()).find(|i| self.enabled(*i))
        };
        if next.is_none() {
            return false;
        }
        let before = self.child_revision();
        self.captured = None;
        self.set_focus(next);
        self.update_revision(before);
        true
    }
    pub fn key_event(&mut self, key: u8, pressed: bool, repeat: bool) {
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
        for c in &mut self.controls { c.reveal_pointer(x, y, kind != 3); }
        let hit = usize::try_from(self.hit_index(x, y)).ok();
        if kind == 1 {
            for c in &mut self.controls {
                c.keyboard = None;
            }
            self.captured = hit.filter(|i| self.enabled(*i));
            self.set_focus(self.captured);
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
        self.controls[self.active()].key()
    }
    pub fn label(&self) -> String {
        self.controls[self.active()].label()
    }
    pub fn action(&self) -> String {
        self.controls[self.active()].action()
    }
    pub fn disabled(&self) -> bool {
        self.controls.iter().all(Button::disabled)
    }
    pub fn bounds(&self) -> Vec<f32> {
        self.controls[self.focused.unwrap_or(0)].bounds()
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
        for c in &mut self.controls { c.reveal_pointer(x, y, present); }
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
                    if next.spec.value==prev.spec.value && next.spec.multiline==prev.spec.multiline {
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
