//! `Scroll` containers below the root: each scrolls its own controls inside its viewport, on top
//! of whatever the root scroll does.

use super::Runtime;
use crate::form_document::SceneScroll;

/// One scroll area: what it shows, how far its content reaches, and how far it has moved.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ScrollArea {
    /// The visible rectangle in scene coordinates, before any scrolling.
    pub(super) viewport: [f32; 4],
    /// The content size from the viewport's corner, never less than the viewport.
    pub(super) content: [f32; 2],
    pub(super) offset: [f32; 2],
    /// The area that contains this one, if any.
    pub(super) parent: Option<usize>,
}

impl ScrollArea {
    fn limit(&self) -> [f32; 2] {
        [
            (self.content[0] - self.viewport[2]).max(0.),
            (self.content[1] - self.viewport[3]).max(0.),
        ]
    }
}

/// The areas of a scene and, per control, the areas that contain it, outermost first.
pub(super) fn areas_of<'a>(
    controls: impl IntoIterator<Item = &'a [SceneScroll]>,
) -> (Vec<ScrollArea>, Vec<Vec<usize>>) {
    let mut areas: Vec<Option<ScrollArea>> = Vec::new();
    let chains = controls
        .into_iter()
        .map(|chain| {
            let mut parent = None;
            chain
                .iter()
                .map(|scroll| {
                    if areas.len() <= scroll.area {
                        areas.resize(scroll.area + 1, None);
                    }
                    areas[scroll.area].get_or_insert_with(|| ScrollArea {
                        viewport: scroll.viewport.map(|v| v as f32),
                        content: scroll.content.map(|v| v as f32),
                        offset: [0.; 2],
                        parent,
                    });
                    parent = Some(scroll.area);
                    scroll.area
                })
                .collect()
        })
        .collect();
    // An area with no control anywhere below it cannot be reached; keep its slot empty-sized.
    let areas = areas
        .into_iter()
        .map(|area| {
            area.unwrap_or(ScrollArea {
                viewport: [0.; 4],
                content: [0.; 2],
                offset: [0.; 2],
                parent: None,
            })
        })
        .collect();
    (areas, chains)
}

impl Runtime {
    /// Replace the scroll areas around every control, outermost first, starting them unscrolled.
    pub(super) fn set_scroll_areas<'a>(
        &mut self,
        chains: impl IntoIterator<Item = &'a [SceneScroll]>,
    ) {
        (self.scroll_areas, self.scroll_chains) = areas_of(chains);
        let clips = std::mem::take(&mut self.clips);
        self.set_clips(clips);
    }

    /// How far the areas around control `index` have moved it, on top of the root scroll.
    pub(super) fn area_offset(&self, index: usize) -> [f32; 2] {
        self.scroll_chains.get(index).map_or([0.; 2], |chain| {
            chain.iter().fold([0.; 2], |sum, &area| {
                let offset = self.scroll_areas[area].offset;
                [sum[0] + offset[0], sum[1] + offset[1]]
            })
        })
    }

    /// The root scroll that applies to control `index`: none inside a modal layer.
    pub(super) fn root_scroll_of(&self, index: usize) -> [f32; 2] {
        if self.modal_of[index].is_some() {
            [0.; 2]
        } else {
            self.scroll
        }
    }

    /// Give every control the scroll offset it is drawn and hit at.
    pub(super) fn sync_control_scroll(&mut self) {
        for index in 0..self.controls.len() {
            let root = self.root_scroll_of(index);
            let area = self.area_offset(index);
            let control = &mut self.controls[index];
            control.scroll_x = root[0] + area[0];
            control.scroll_y = root[1] + area[1];
        }
    }

    /// Where area `index`'s viewport is on screen now: moved by the root and the areas around it.
    fn area_screen_viewport(&self, index: usize) -> [f32; 4] {
        let mut shift = self.scroll;
        let mut parent = self.scroll_areas[index].parent;
        while let Some(p) = parent {
            shift[0] += self.scroll_areas[p].offset[0];
            shift[1] += self.scroll_areas[p].offset[1];
            parent = self.scroll_areas[p].parent;
        }
        let [x, y, w, h] = self.scroll_areas[index].viewport;
        [x - shift[0], y - shift[1], w, h]
    }

    fn area_depth(&self, index: usize) -> usize {
        let mut depth = 0;
        let mut parent = self.scroll_areas[index].parent;
        while let Some(p) = parent {
            depth += 1;
            parent = self.scroll_areas[p].parent;
        }
        depth
    }

    /// Scroll the innermost area under the pointer that can still move this way. Returns
    /// whether one did; otherwise the wheel belongs to the root.
    pub(super) fn scroll_area_under_pointer(&mut self, dx: f32, dy: f32) -> bool {
        let Some([x, y]) = self.pointer_at else {
            return false;
        };
        let mut candidates: Vec<usize> = (0..self.scroll_areas.len())
            .filter(|&i| {
                let [vx, vy, vw, vh] = self.area_screen_viewport(i);
                x >= vx && y >= vy && x < vx + vw && y < vy + vh
            })
            .collect();
        candidates.sort_by_key(|&i| std::cmp::Reverse(self.area_depth(i)));
        for index in candidates {
            let area = &mut self.scroll_areas[index];
            let limit = area.limit();
            let old = area.offset;
            area.offset = [
                (old[0] + dx).clamp(0., limit[0]),
                (old[1] + dy).clamp(0., limit[1]),
            ];
            if area.offset != old {
                self.sync_control_scroll();
                *self.geometry.borrow_mut() = None;
                self.revision = self.revision.wrapping_add(1);
                self.geometry_revision = self.geometry_revision.wrapping_add(1);
                self.layout_revision = self.layout_revision.wrapping_add(1);
                return true;
            }
        }
        false
    }

    /// Scroll the areas around control `index`, innermost first, just far enough to show it.
    pub(super) fn reveal_in_scroll_areas(&mut self, index: usize) {
        let Some(chain) = self.scroll_chains.get(index).cloned() else {
            return;
        };
        let b = &self.controls[index].scene.button;
        // The control's rectangle as each area sees it: moved by the areas inside that one.
        let mut rect = [b.x, b.y, b.width, b.height];
        let mut changed = false;
        for &area in chain.iter().rev() {
            let area = &mut self.scroll_areas[area];
            let [vx, vy, vw, vh] = area.viewport;
            let limit = area.limit();
            let old = area.offset;
            let reveal =
                |offset: f32, start: f32, size: f32, view: f32, extent: f32, limit: f32| {
                    // Visible content runs from view + offset to view + offset + extent.
                    let offset = if start < view + offset {
                        start - view
                    } else if start + size > view + offset + extent {
                        start + size - view - extent
                    } else {
                        offset
                    };
                    offset.clamp(0., limit)
                };
            area.offset = [
                reveal(old[0], rect[0], rect[2], vx, vw, limit[0]),
                reveal(old[1], rect[1], rect[3], vy, vh, limit[1]),
            ];
            changed |= area.offset != old;
            rect[0] -= area.offset[0];
            rect[1] -= area.offset[1];
        }
        if changed {
            self.sync_control_scroll();
            *self.geometry.borrow_mut() = None;
            self.revision = self.revision.wrapping_add(1);
            self.geometry_revision = self.geometry_revision.wrapping_add(1);
            self.layout_revision = self.layout_revision.wrapping_add(1);
        }
    }

    /// The scroll offset of area `index`.
    #[cfg(test)]
    pub(super) fn area_scroll(&self, index: usize) -> Option<[f32; 2]> {
        self.scroll_areas.get(index).map(|area| area.offset)
    }

    /// Scrollbar thumbs of every area whose content overflows it, in root content coordinates:
    /// where they are drawn before the root scroll moves them.
    pub(super) fn area_thumbs(&self) -> Vec<[f32; 4]> {
        let mut thumbs = Vec::new();
        for index in 0..self.scroll_areas.len() {
            let area = &self.scroll_areas[index];
            let [x, y, w, h] = self.area_screen_viewport(index);
            let (x, y) = (x + self.scroll[0], y + self.scroll[1]);
            let [cw, ch] = area.content;
            if ch > h && h > 0. {
                thumbs.push([
                    (x + w - 5.).max(x),
                    y + area.offset[1] / ch * h,
                    5f32.min(w),
                    h * h / ch,
                ]);
            }
            if cw > w && w > 0. {
                thumbs.push([
                    x + area.offset[0] / cw * w,
                    (y + h - 5.).max(y),
                    w * w / cw,
                    5f32.min(h),
                ]);
            }
        }
        thumbs
    }
}
