//! `Scroll` containers below the root: each scrolls its own controls inside its viewport, on top
//! of whatever the root scroll does.

use super::Runtime;
use crate::form_document::SceneScroll;

/// The colour of scroll area thumbs, the same as the root's.
pub(super) const AREA_THUMB: [u8; 4] = [110, 130, 170, 255];

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

    /// The offset that shows `rect`, a rectangle in this area's content, moving as little as
    /// possible from where the area is now.
    fn revealing(&self, rect: [f32; 4]) -> [f32; 2] {
        let limit = self.limit();
        let axis = |i: usize| {
            let (start, size) = (rect[i], rect[i + 2]);
            let (view, extent) = (self.viewport[i], self.viewport[i + 2]);
            // Visible content runs from view + offset to view + offset + extent.
            let offset = if start < view + self.offset[i] {
                start - view
            } else if start + size > view + self.offset[i] + extent {
                start + size - view - extent
            } else {
                self.offset[i]
            };
            offset.clamp(0., limit[i])
        };
        [axis(0), axis(1)]
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
        self.attach_area_nodes();
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

    /// Put a `Scroll` node in the tree for every area, under its parent area or where its controls
    /// were, and move each control under its innermost area. Existing node ids stay as they are.
    pub(super) fn attach_area_nodes(&mut self) {
        let control_node = |nodes: &[super::Node], index: usize| {
            nodes.iter().position(|node| node.control == Some(index))
        };
        // Undo a previous attachment: controls go home, area nodes go away.
        if !self.area_nodes.is_empty() {
            let first = self.area_nodes[0];
            for (index, &home) in self.control_homes.iter().enumerate() {
                if let Some(node) = control_node(&self.nodes, index) {
                    self.nodes[node].parent = Some(home);
                    if !self.nodes[home].children.contains(&node) {
                        self.nodes[home].children.push(node);
                    }
                }
            }
            self.nodes.truncate(first);
            for node in &mut self.nodes {
                node.children.retain(|&child| child < first);
            }
            self.area_nodes.clear();
        }
        self.control_homes = (0..self.controls.len())
            .map(|index| {
                control_node(&self.nodes, index)
                    .and_then(|node| self.nodes[node].parent)
                    .unwrap_or(0)
            })
            .collect();
        if self.scroll_areas.is_empty() {
            return;
        }
        let first = self.nodes.len();
        self.area_nodes = (first..first + self.scroll_areas.len()).collect();
        for area in 0..self.scroll_areas.len() {
            let parent = match self.scroll_areas[area].parent {
                Some(outer) => self.area_nodes[outer],
                // Where the first control inside it lived.
                None => self
                    .scroll_chains
                    .iter()
                    .position(|chain| chain.first() == Some(&area))
                    .map_or(0, |index| self.control_homes[index]),
            };
            let id = first + area;
            self.nodes.push(super::Node {
                id,
                parent: Some(parent),
                children: Vec::new(),
                kind: super::NodeKind::Scroll,
                control: None,
                modal: None,
            });
            self.nodes[parent].children.push(id);
        }
        for index in 0..self.controls.len() {
            let Some(&inner) = self.scroll_chains[index].last() else {
                continue;
            };
            let Some(node) = control_node(&self.nodes, index) else {
                continue;
            };
            let home = self.control_homes[index];
            self.nodes[home].children.retain(|&child| child != node);
            let area_node = self.area_nodes[inner];
            self.nodes[node].parent = Some(area_node);
            self.nodes[area_node].children.push(node);
        }
    }

    /// The scroll area whose tree node is `node`, if it is one.
    pub(super) fn area_of_node(&self, node: usize) -> Option<usize> {
        self.area_nodes.iter().position(|&id| id == node)
    }

    /// Where area `index`'s viewport is on screen now: moved by the root and the areas around it.
    pub(super) fn area_screen_viewport(&self, index: usize) -> [f32; 4] {
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
        candidates
            .into_iter()
            .any(|index| self.move_area(index, dx, dy))
    }

    /// Move area `index` by `dx, dy` within its content. Returns whether it moved.
    pub(super) fn move_area(&mut self, index: usize, dx: f32, dy: f32) -> bool {
        let area = &mut self.scroll_areas[index];
        let limit = area.limit();
        let old = area.offset;
        area.offset = [
            (old[0] + dx).clamp(0., limit[0]),
            (old[1] + dy).clamp(0., limit[1]),
        ];
        if area.offset == old {
            return false;
        }
        self.areas_moved();
        true
    }

    /// An area's offset changed: controls take their new places and everything painted or laid
    /// out from the old ones is stale.
    fn areas_moved(&mut self) {
        self.sync_control_scroll();
        *self.geometry.borrow_mut() = None;
        self.revision = self.revision.wrapping_add(1);
        self.geometry_revision = self.geometry_revision.wrapping_add(1);
        self.layout_revision = self.layout_revision.wrapping_add(1);
    }

    /// After a rebuild, give each area the offset the same area had before: same number, same
    /// viewport, limited by its new content.
    pub(super) fn keep_area_scroll(&mut self, previous: &Runtime) {
        let mut moved = false;
        for (area, old) in self.scroll_areas.iter_mut().zip(&previous.scroll_areas) {
            if area.viewport == old.viewport && old.offset != [0.; 2] {
                let limit = area.limit();
                area.offset = [old.offset[0].min(limit[0]), old.offset[1].min(limit[1])];
                moved = true;
            }
        }
        if moved {
            self.areas_moved();
        }
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
            let old = area.offset;
            area.offset = area.revealing(rect);
            changed |= area.offset != old;
            rect[0] -= area.offset[0];
            rect[1] -= area.offset[1];
        }
        if changed {
            self.areas_moved();
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
