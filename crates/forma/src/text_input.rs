//! Text editing state and visual composition shared by native and WASM hosts.
use crate::{
    control_state::TextEdit,
    template::{Content, Text},
};
pub(crate) const MAX_VALUE_BYTES: usize = 100_000;
fn next_semantic_revision() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    allocate_semantic_revision(&NEXT)
}
fn allocate_semantic_revision(counter: &std::sync::atomic::AtomicU32) -> u32 {
    use std::sync::atomic::Ordering::Relaxed;
    // Zero disables transport caching permanently after the finite key space is exhausted.
    counter
        .fetch_update(Relaxed, Relaxed, |value| Some(value.saturating_add(1)))
        .expect("update always returns Some")
        .checked_add(1)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub bounds: [f32; 4],
    pub value: String,
    pub placeholder: String,
    pub label: String,
    pub read_only: bool,
    pub color: [u8; 4],
    pub placeholder_color: [u8; 4],
    pub font_size: f32,
    pub multiline: bool,
    pub wrap: bool,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            bounds: [0., 0., 100., 30.],
            value: String::new(),
            placeholder: String::new(),
            label: String::new(),
            read_only: false,
            color: [255; 4],
            placeholder_color: [150, 150, 150, 255],
            font_size: 15.,
            multiline: false,
            wrap: false,
        }
    }
}
#[derive(Clone)]
pub(crate) struct Editor {
    pub spec: Input,
    pub model: TextEdit,
    pub base: Vec<Content>,
    pub preedit: String,
    offset: [f32; 2],
    preferred_x: Option<f32>,
    focused: bool,
    reduced: bool,
    phase: f32,
    fades: Vec<(std::ops::Range<usize>, f32)>,
    dirty: bool,
    pub semantic_revision: u32,
}
struct VisualRowContext<'a> {
    out: &'a mut Vec<Content>,
    focused: bool,
    selection: &'a std::ops::Range<usize>,
    px: f32,
    lh: f32,
}

impl Editor {
    pub fn new(spec: Input, base: Vec<Content>) -> Result<Self, String> {
        let model = TextEdit::with_multiline(spec.value.clone(), spec.multiline)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            spec,
            model,
            base,
            preedit: String::new(),
            offset: [0.; 2],
            preferred_x: None,
            focused: false,
            reduced: false,
            phase: 0.,
            fades: Vec::new(),
            dirty: true,
            semantic_revision: next_semantic_revision(),
        })
    }
    pub fn needs_content(&self, focused: bool) -> bool {
        self.dirty || self.focused != focused
    }
    pub fn invalidate_content(&mut self) {
        self.dirty = true;
    }
    pub fn set_preedit(&mut self, text: &str) {
        if self.spec.read_only && !text.is_empty() {
            return;
        }
        if self.preedit != text {
            self.preedit.clear();
            self.preedit.push_str(text);
            self.dirty = true;
        }
    }
    pub fn is_animating(&self) -> bool {
        !self.reduced && (self.focused || !self.fades.is_empty())
    }
    pub fn reduced_motion(&mut self, reduced: bool) {
        self.dirty |= self.reduced != reduced;
        self.reduced = reduced;
        if reduced {
            self.fades.clear();
            self.phase = 0.;
        }
    }
    pub fn tick(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0. || self.reduced {
            return;
        }
        let before = (self.spec.color[3] as f32 * self.caret_alpha()).round() as u8;
        if self.focused {
            self.phase = (self.phase + dt) % 1100.;
        }
        self.dirty |= !self.fades.is_empty()
            || (self.focused
                && before != (self.spec.color[3] as f32 * self.caret_alpha()).round() as u8);
        for (_, age) in &mut self.fades {
            *age += dt;
        }
        self.fades.retain(|(_, age)| *age < 220.);
    }
    fn caret_alpha(&self) -> f32 {
        if self.reduced {
            1.
        } else {
            (1. + (self.phase / 1100. * std::f32::consts::TAU).cos()) * 0.5
        }
    }
    fn width(&self, s: &str) -> f32 {
        crate::text::measure_line(s, self.spec.font_size)[0]
    }
    fn line_height(&self) -> f32 {
        self.spec.font_size * 1.5
    }
    fn caret_point(&self) -> [f32; 2] {
        let wrapped = (self.spec.multiline && self.spec.wrap).then(|| {
            crate::text::wrap_text_ranges(
                self.model.value(),
                self.spec.font_size,
                self.spec.bounds[2].max(1.),
            )
        });
        self.caret_point_with_ranges(wrapped.as_deref())
    }
    fn caret_point_with_ranges(
        &self,
        wrapped: Option<&[(std::ops::Range<usize>, f32)]>,
    ) -> [f32; 2] {
        if let Some(lines) = wrapped {
            let caret = self.model.caret();
            for (row, (byte_range, _)) in lines.iter().enumerate().rev() {
                if caret >= byte_range.start && (caret <= byte_range.end || row + 1 == lines.len())
                {
                    let end = caret.min(byte_range.end);
                    return [
                        self.width(&self.model.value()[byte_range.start..end]),
                        row as f32 * self.line_height(),
                    ];
                }
            }
        }
        let prefix = &self.model.value()[..self.model.caret()];
        [
            self.width(prefix.rsplit('\n').next().unwrap_or("")),
            prefix.bytes().filter(|b| *b == b'\n').count() as f32 * self.line_height(),
        ]
    }
    fn top(&self) -> f32 {
        if self.spec.multiline {
            0.
        } else {
            (self.spec.bounds[3] - self.line_height()) * 0.5
        }
    }
    pub fn hit(&mut self, x: f32, y: f32, extend: bool) {
        if self.spec.multiline && self.spec.wrap {
            self.hit_soft_wrapped(x, y, extend);
            return;
        }
        self.semantic_revision = next_semantic_revision();
        self.preferred_x = None;
        self.phase = 0.;
        self.dirty = true;
        let x = x - self.spec.bounds[0] + self.offset[0];
        let row = ((y - self.spec.bounds[1] - self.top() + self.offset[1]) / self.line_height())
            .floor()
            .max(0.) as usize;
        let mut lines = self.model.value().split('\n');
        let mut line = lines.next().unwrap_or("");
        let mut start = 0;
        for next in lines.take(row) {
            start += line.len() + 1;
            line = next;
        }
        // split('\n') leaves CR attached to its row, but CRLF is one cluster.
        // Exclude the CR from hit targets only when a following LF exists.
        if start + line.len() < self.model.value().len() {
            line = line.strip_suffix('\r').unwrap_or(line);
        }
        let pos = crate::text::hit_grapheme(line, self.spec.font_size, x);
        let anchor = if extend {
            self.model.anchor()
        } else {
            start + pos
        };
        let _ = self.model.set_selection(anchor, start + pos);
    }
    pub fn select_utf16(&mut self, anchor: u32, caret: u32) -> bool {
        self.select_utf16_in_value(anchor, caret, true)
    }
    pub fn select_original_utf16(&mut self, anchor: u32, caret: u32) -> bool {
        self.select_utf16_in_value(anchor, caret, false)
    }
    fn select_utf16_in_value(&mut self, anchor: u32, caret: u32, normalized: bool) -> bool {
        let byte = |offset: u32, ceil: bool| {
            let mut units = 0;
            let mut chars = self.model.value().char_indices().peekable();
            while let Some((at, ch)) = chars.next() {
                if units >= offset {
                    return at;
                }
                units += ch.len_utf16() as u32;
                let mut end = at + ch.len_utf8();
                if normalized && ch == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    end = chars.next().unwrap().0 + 1;
                }
                if units >= offset {
                    return if units > offset && !ceil { at } else { end };
                }
            }
            self.model.value().len()
        };
        let (a, c) = (byte(anchor, anchor >= caret), byte(caret, caret >= anchor));
        let old = (self.model.anchor(), self.model.caret());
        self.model
            .set_selection(a, c)
            .expect("mapped scalar boundaries");
        let range = self.model.grapheme_selection();
        let (a, c) = if anchor > caret {
            (range.end, range.start)
        } else {
            (range.start, range.end)
        };
        self.model.set_selection(a, c).expect("grapheme boundaries");
        let changed = old != (a, c);
        if changed {
            self.semantic_revision = next_semantic_revision();
            self.phase = 0.;
            self.preferred_x = None;
            self.dirty = true;
        }
        changed
    }
    pub fn word_key(&mut self, key: &str, shift: bool) -> bool {
        if !matches!(key, "ArrowLeft" | "ArrowRight" | "Backspace" | "Delete") {
            return false;
        }
        if self.spec.read_only && matches!(key, "Backspace" | "Delete") {
            return true;
        }
        self.preferred_x = None;
        self.phase = 0.;
        self.dirty = true;
        match key {
            "ArrowLeft" => {
                self.model.move_words_dictionary(-1, shift);
            }
            "ArrowRight" => {
                self.model.move_words_dictionary(1, shift);
            }
            "Backspace" => {
                self.fades.clear();
                self.model.backspace_word_dictionary();
            }
            "Delete" => {
                self.fades.clear();
                self.model.delete_word_dictionary();
            }
            _ => unreachable!(),
        }
        self.semantic_revision = next_semantic_revision();
        self.preedit.clear();
        true
    }
    pub fn key(&mut self, key: &str, shift: bool, command: bool) -> bool {
        if self.spec.read_only
            && (matches!(key, "Backspace" | "Delete" | "Enter")
                || command && matches!(key.to_ascii_lowercase().as_str(), "z" | "y"))
        {
            return true;
        }
        self.phase = 0.;
        self.dirty = true;
        if command {
            if matches!(key.to_lowercase().as_str(), "z" | "y") {
                self.fades.clear();
            }
            match key.to_lowercase().as_str() {
                "backspace" => {
                    self.fades.clear();
                    self.model.delete_to_line_edge(false);
                }
                "delete" => {
                    self.fades.clear();
                    self.model.delete_to_line_edge(true);
                }
                "a" => {
                    self.model.select_all();
                }
                "z" => {
                    if shift {
                        self.model.redo();
                    } else {
                        self.model.undo();
                    }
                }
                "y" => {
                    self.model.redo();
                }
                _ => return false,
            }
            self.preferred_x = None;
            self.semantic_revision = next_semantic_revision();
            self.preedit.clear();
            return true;
        }
        match key {
            "Backspace" => {
                self.fades.clear();
                self.model.backspace_grapheme();
            }
            "Delete" => {
                self.fades.clear();
                self.model.delete_grapheme();
            }
            "ArrowLeft" => {
                self.model.move_graphemes(-1, shift);
            }
            "ArrowRight" => {
                self.model.move_graphemes(1, shift);
            }
            "Home" | "End" => {
                let pos = self.model.caret();
                let value = self.model.value();
                let target = if key == "Home" {
                    value[..pos].rfind('\n').map_or(0, |i| i + 1)
                } else {
                    value[pos..].find('\n').map_or(value.len(), |i| {
                        let end = pos + i;
                        if value[..end].ends_with('\r') {
                            end - 1
                        } else {
                            end
                        }
                    })
                };
                let anchor = if shift { self.model.anchor() } else { target };
                let _ = self.model.set_selection(anchor, target);
            }
            "ArrowUp" | "ArrowDown" => {
                let p = self.caret_point();
                let desired = self.preferred_x.unwrap_or(p[0]);
                let y = p[1]
                    + if key == "ArrowUp" {
                        -self.line_height()
                    } else {
                        self.line_height()
                    };
                self.hit(
                    self.spec.bounds[0] + desired - self.offset[0],
                    self.spec.bounds[1] + self.top() + y - self.offset[1]
                        + self.line_height() * 0.5,
                    shift,
                );
                self.preferred_x = Some(desired);
            }
            "Enter" => {
                if self.spec.multiline {
                    self.insert("\n");
                }
            }
            _ => return false,
        }
        if !matches!(key, "ArrowUp" | "ArrowDown") {
            self.preferred_x = None;
        }
        self.semantic_revision = next_semantic_revision();
        self.preedit.clear();
        true
    }
    pub fn insert(&mut self, text: &str) {
        if self.spec.read_only {
            return;
        }
        self.semantic_revision = next_semantic_revision();
        self.preferred_x = None;
        self.dirty = true;
        self.preedit.clear();
        let clean = text.replace("\r\n", "\n").replace('\r', "\n");
        let clean = if self.spec.multiline {
            clean
        } else {
            clean.replace('\n', " ")
        };
        if self.model.value().len() - self.model.grapheme_selection().len() + clean.len()
            <= MAX_VALUE_BYTES
        {
            let selected = self.model.grapheme_selection();
            // Existing characters keep their age when an insertion shifts their byte offsets.
            self.fades
                .retain(|(range, _)| range.end <= selected.start || range.start >= selected.end);
            for (range, _) in &mut self.fades {
                if range.start >= selected.end {
                    range.start = range.start - selected.len() + clean.len();
                    range.end = range.end - selected.len() + clean.len();
                }
            }
            let _ = self.model.insert_graphemes(&clean);
            if !self.reduced && !clean.is_empty() {
                self.fades
                    .push((selected.start..selected.start + clean.len(), 0.));
            }
            self.phase = 0.;
        }
    }
    fn push_visual_row(&self, ctx: &mut VisualRowContext<'_>, py: f32, start: usize, line: &str) {
        if ctx.focused && !ctx.selection.is_empty() {
            let a = ctx.selection.start.max(start);
            let b = ctx.selection.end.min(start + line.len());
            if a <= b && ctx.selection.end > start && ctx.selection.start <= start + line.len() {
                let left = self.width(&line[..a.saturating_sub(start).min(line.len())]);
                let right = self.width(&line[..b.saturating_sub(start).min(line.len())]);
                ctx.out.push(rect(
                    ctx.px + left,
                    py,
                    (right - left).max(if ctx.selection.end > start + line.len() {
                        5.
                    } else {
                        0.
                    }),
                    ctx.lh,
                    [222, 106, 25, 90],
                ));
            }
        }
        if self.fades.is_empty() {
            ctx.out.push(Content::Text {
                bounds: [ctx.px, py, self.width(line), ctx.lh],
                text: Text {
                    text: line.into(),
                    font_size: self.spec.font_size,
                    color: self.spec.color,
                },
            });
        } else {
            let mut cuts = vec![0, line.len()];
            for (range, _) in &self.fades {
                if range.start < start + line.len() && range.end > start {
                    cuts.push(range.start.saturating_sub(start));
                    cuts.push((range.end - start).min(line.len()));
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            for run in cuts.windows(2) {
                let text = &line[run[0]..run[1]];
                let mut color = self.spec.color;
                if let Some((_, age)) = self
                    .fades
                    .iter()
                    .find(|(range, _)| range.contains(&(start + run[0])))
                {
                    let t = (*age / 220.).clamp(0., 1.);
                    color[3] = (color[3] as f32 * t * t * (3. - 2. * t)).round() as u8;
                }
                ctx.out.push(Content::Text {
                    bounds: [
                        ctx.px + self.width(&line[..run[0]]),
                        py,
                        self.width(text),
                        ctx.lh,
                    ],
                    text: Text {
                        text: text.into(),
                        font_size: self.spec.font_size,
                        color,
                    },
                });
            }
        }
    }
    pub fn content(&mut self, focused: bool) -> Vec<Content> {
        if focused != self.focused {
            self.phase = 0.;
            self.focused = focused;
            self.preferred_x = None;
        }
        let mut out = self.base.clone();
        let [x, y, w, h] = self.spec.bounds;
        let lh = self.line_height();
        let wrapped = (self.spec.multiline && self.spec.wrap).then(|| {
            crate::text::wrap_text_ranges(
                self.model.value(),
                self.spec.font_size,
                self.spec.bounds[2].max(1.),
            )
        });
        let p = self.caret_point_with_ranges(wrapped.as_deref());
        if focused {
            if wrapped.is_some() {
                self.offset[0] = 0.;
            } else {
                self.offset[0] = self.offset[0]
                    .min(p[0])
                    .max((p[0] + self.width(&self.preedit) + 2. - w).max(0.));
            }
            self.offset[1] = self.offset[1].min(p[1]).max((p[1] + lh - h).max(0.));
        }
        out.push(Content::Clip {
            bounds: [x, y, w, h],
            radius: 0.,
        });
        let value = self.model.value();
        let selection = self.model.selection();
        let px = x - self.offset[0];
        {
            let mut row_ctx = VisualRowContext {
                out: &mut out,
                focused,
                selection: &selection,
                px,
                lh,
            };
            if let Some(wrapped) = &wrapped {
                for (row, (byte_range, _)) in wrapped.iter().enumerate() {
                    let py = y + self.top() + row as f32 * lh - self.offset[1];
                    self.push_visual_row(
                        &mut row_ctx,
                        py,
                        byte_range.start,
                        &value[byte_range.clone()],
                    );
                }
            } else {
                let mut start = 0;
                for (row, line) in value.split('\n').enumerate() {
                    let raw_len = line.len();
                    let line = if start + raw_len < value.len() {
                        line.strip_suffix('\r').unwrap_or(line)
                    } else {
                        line
                    };
                    let py = y + self.top() + row as f32 * lh - self.offset[1];
                    self.push_visual_row(&mut row_ctx, py, start, line);
                    start += raw_len + 1;
                }
            }
        }
        if value.is_empty() && self.preedit.is_empty() {
            out.push(Content::Text {
                bounds: [x, y + self.top(), self.width(&self.spec.placeholder), lh],
                text: Text {
                    text: self.spec.placeholder.clone(),
                    font_size: self.spec.font_size,
                    color: self.spec.placeholder_color,
                },
            });
        }
        if focused {
            let cx = x + p[0] - self.offset[0];
            let cy = y + self.top() + p[1] - self.offset[1];
            let pre = self.width(&self.preedit);
            if !self.preedit.is_empty() {
                out.push(Content::Text {
                    bounds: [cx, cy, pre, lh],
                    text: Text {
                        text: self.preedit.clone(),
                        font_size: self.spec.font_size,
                        color: self.spec.color,
                    },
                });
                out.push(rect(cx, cy + lh - 1., pre, 1., self.spec.color));
            }
            let mut color = self.spec.color;
            color[3] = (color[3] as f32 * self.caret_alpha()).round() as u8;
            out.push(rect(cx + pre, cy, 1., lh, color));
        }
        out.push(Content::ClipEnd);
        self.dirty = false;
        out
    }
    pub fn caret_rect(&self) -> [f32; 4] {
        let p = self.caret_point();
        [
            self.spec.bounds[0] + p[0] - self.offset[0],
            self.spec.bounds[1] + self.top() + p[1] - self.offset[1],
            1.,
            self.line_height(),
        ]
    }
    /// Computes visual soft-wrapped lines for this editor's current value constrained by its bounding width.
    pub fn soft_wrapped_lines(&self) -> Vec<crate::text::SoftWrappedLine> {
        crate::text::wrap_shaped_text(
            self.model.value(),
            self.spec.font_size,
            self.spec.bounds[2].max(1.),
        )
    }
    /// Hit-tests `(x, y)` in visual soft-wrapped layout coordinates and updates the editor selection/caret.
    pub fn hit_soft_wrapped(&mut self, x: f32, y: f32, extend: bool) {
        self.semantic_revision = next_semantic_revision();
        self.preferred_x = None;
        self.phase = 0.;
        self.dirty = true;
        let lx = x - self.spec.bounds[0];
        let ly = y - self.spec.bounds[1] - self.top() + self.offset[1];
        let target = crate::text::hit_wrapped_text(
            self.model.value(),
            self.spec.font_size,
            self.spec.bounds[2].max(1.),
            self.line_height(),
            lx,
            ly,
        );
        let anchor = if extend { self.model.anchor() } else { target };
        let _ = self.model.set_selection(anchor, target);
    }
}
fn rect(x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) -> Content {
    Content::Shape {
        points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
        color,
    }
}

#[cfg(test)]
mod animation_tests;

#[cfg(test)]
mod semantic_epoch_tests;
