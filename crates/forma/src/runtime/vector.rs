//! The vector snapshot of the whole scene.

use super::*;

impl Runtime {
    pub fn vector_snapshot(&self, scale: f32, background: bool) -> Arc<DisplayList> {
        if self.single_leaf() {
            return self.controls[0].vector_snapshot(scale, background);
        }
        let mut cache = self.geometry.borrow_mut();
        if let Some(c) = cache.as_ref() {
            if c.scale == scale && c.background == background {
                return Arc::clone(&c.list);
            }
        }
        use crate::display_list::{
            ClipId, DisplayItem, DisplayItemKind, SCROLL_AREA_THUMB_X, SCROLL_AREA_THUMB_Y,
            SCROLL_EXTRA, SCROLL_FACTOR, SCROLL_FIXED, SCROLL_MODE, STRIDE, Transform2D,
        };
        use crate::resource_cache::ResourceId;
        // Every control builds its own list before the merge runs. The scene repeats nearly
        // identical controls, so each one after the first can start at a sibling's final
        // capacity instead of growing into it by doubling, and the merged buffers are exact.
        let mut children = Vec::with_capacity(self.controls.len());
        let mut text_scratch = crate::text::VectorScratch::prewarm();
        let mut hints = None;
        for control in &self.controls {
            let child = control.vector_snapshot_hinted(scale, false, hints, &mut text_scratch);
            hints = Some(ListHints {
                commands: child.commands.len(),
                edges: child.edges.len(),
                items: child.items.len(),
                clips: child.clips.len(),
            });
            children.push(child);
        }
        let canvas_lists: Vec<Option<([f32; 4], Arc<DisplayList>)>> = (0..self.controls.len())
            .map(|i| Some((self.canvas_scene_rect(i)?, self.canvas_placed(i)?)))
            .collect();
        let extra_clips = usize::from(self.scene.clip)
            + usize::from(self.scene.scroll)
            + self.clips.iter().filter(|c| c.is_some()).count()
            + canvas_lists.iter().filter(|c| c.is_some()).count();
        let mut list = DisplayList::with_hints(Some(ListHints {
            commands: children
                .iter()
                .map(|child| child.commands.len())
                .sum::<usize>()
                + STRIDE * 8,
            edges: children
                .iter()
                .map(|child| child.edges.len())
                .sum::<usize>(),
            items: children
                .iter()
                .map(|child| child.items.len())
                .sum::<usize>()
                + 8,
            clips: children
                .iter()
                .map(|child| child.clips.len())
                .sum::<usize>()
                + extra_clips,
        }));
        let mut clip_top: Option<ClipId> = None;
        let frame = [0., 0., self.width(), self.height()];
        if self.scene.clip {
            list.push_clip_node(frame, self.scene.radius, false, &mut clip_top);
        }
        if background {
            let bg_radius = if self.scene.clip {
                0.
            } else {
                self.scene.radius
            };
            list.command(0., frame, self.scene.background, bg_radius, 0.);
            list.items.push(DisplayItem {
                kind: DisplayItemKind::SolidRect,
                bounds: frame,
                transform: Transform2D::IDENTITY,
                clip_id: clip_top,
                resource_id: None,
                color: self.scene.background,
                radius: bg_radius,
                stroke: 0.,
                edge_range: [0, 0],
                nonzero: false,
                paint_slots: [0, 0],
                scroll_mode: SCROLL_FIXED,
                scroll_factor: 0.,
                scroll_extra: [0.; 2],
                area_slot: None,
                control_index: None,
            });
        }
        let v = self.viewport_rect();
        if self.scene.scroll {
            list.push_clip_node([v[0], v[1], v[2], v[3]], 0., true, &mut clip_top);
        }
        // Children are built in content coordinates; the scroll offset is a
        // renderer uniform, so this list survives scrolling unchanged.
        let content_start = list.commands.len();
        let content_item_start = list.items.len();
        let content_clip_start = list.clips.len();
        // Paint slots are indexed by control, not by emission order: reveal
        // slots follow all fill/border pairs in control order.
        let mut reveal_slots = Vec::with_capacity(self.controls.len());
        let mut reveal_slot = self.controls.len() * 2;
        for c in &self.controls {
            reveal_slots.push(reveal_slot);
            if c.template.reveal.is_some() {
                reveal_slot += 2;
            }
        }
        let area_base = reveal_slot as u32;
        let emit = |list: &mut DisplayList, index: usize, parent_clip: Option<ClipId>| {
            let mut local_top = parent_clip;
            let clip = self.clip(index);
            if let Some(rect) = clip {
                list.push_clip_node(rect, 0., false, &mut local_top);
            }
            let child = &children[index];
            let area_start = list.commands.len();
            let area_item_start = list.items.len();
            let area_clip_start = list.clips.len();
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
            let clip_base = list.clips.len() as u32;
            for clip_node in &child.clips {
                let mut cloned = clip_node.clone();
                cloned.id = ClipId(clip_base + clip_node.id.0);
                cloned.parent = match clip_node.parent {
                    Some(p) => Some(ClipId(clip_base + p.0)),
                    None => local_top,
                };
                list.clips.push(cloned);
            }
            for item in &child.items {
                let mut cloned = item.clone();
                cloned.clip_id = match item.clip_id {
                    Some(id) => Some(ClipId(clip_base + id.0)),
                    None => local_top,
                };
                cloned.control_index = Some(index as u32);
                match cloned.kind {
                    DisplayItemKind::VectorPath => {
                        cloned.edge_range[0] += edge_offset as u32;
                    }
                    DisplayItemKind::StyledBox => {
                        let s = (index * 2) as u32;
                        cloned.paint_slots = [s, s + 1];
                        cloned.resource_id = Some(ResourceId::paint_slot(s));
                    }
                    DisplayItemKind::RevealBorder => {
                        let s = reveal_slots[index] as u32;
                        cloned.paint_slots = [s, s + 1];
                        cloned.resource_id = Some(ResourceId::paint_slot(s));
                    }
                    _ => {}
                }
                list.items.push(cloned);
            }
            list.edges.extend(&child.edges);
            // Application geometry follows its control, clipped to the canvas.
            if let Some((rect, placed)) = &canvas_lists[index] {
                let canvas_edge_offset = (list.edges.len() / 4) as u32;
                let mut canvas_top = local_top;
                let canvas_clip = list.push_clip_node(*rect, 0., false, &mut canvas_top);
                let start = list.commands.len();
                list.commands.extend(&placed.commands);
                for cmd in list.commands[start..].chunks_exact_mut(STRIDE) {
                    cmd[2] += canvas_edge_offset as f32;
                }
                for item in &placed.items {
                    let mut cloned = item.clone();
                    cloned.clip_id = Some(canvas_clip);
                    cloned.edge_range[0] += canvas_edge_offset;
                    cloned.control_index = Some(index as u32);
                    cloned.resource_id = Some(ResourceId::canvas_slot(index as u32));
                    list.items.push(cloned);
                }
                list.edges.extend(&placed.edges);
                list.pop_clip_node(&mut canvas_top);
            }
            if let Some(&inner_area) = self.scroll_chains.get(index).and_then(|c| c.last()) {
                list.mark_area_slot_scrolled(
                    area_start,
                    area_item_start,
                    area_clip_start,
                    area_base + inner_area as u32,
                );
            }
            if clip.is_some() {
                list.pop_clip_node(&mut local_top);
            }
        };
        for index in 0..self.controls.len() {
            if self.modal_of[index].is_none() {
                emit(&mut list, index, clip_top);
            }
        }
        for (area_idx, area) in self.scroll_areas.iter().enumerate() {
            let [vx, vy, vw, vh] = area.viewport;
            let [cw, ch] = area.content;
            let own_slot = (area_base + area_idx as u32) as f32;
            let parent_slot = area.parent.map_or(-1., |p| (area_base + p as u32) as f32);
            if ch > vh && vh > 0. {
                let bounds = [(vx + vw - 5.).max(vx), vy, 5f32.min(vw), vh * vh / ch];
                let factor = vh / ch;
                list.command(5., bounds, super::scroll_areas::AREA_THUMB, 0., 0.);
                let offset = list.commands.len() - STRIDE;
                list.commands[offset + SCROLL_MODE] = SCROLL_AREA_THUMB_Y;
                list.commands[offset + SCROLL_FACTOR] = factor;
                list.commands[offset + SCROLL_EXTRA] = own_slot;
                list.commands[offset + SCROLL_EXTRA + 1] = parent_slot;
                list.items.push(DisplayItem {
                    kind: DisplayItemKind::ScrollbarThumb,
                    bounds,
                    transform: Transform2D::IDENTITY,
                    clip_id: clip_top,
                    resource_id: None,
                    color: super::scroll_areas::AREA_THUMB,
                    radius: 0.,
                    stroke: 0.,
                    edge_range: [0, 0],
                    nonzero: false,
                    paint_slots: [0, 0],
                    scroll_mode: SCROLL_AREA_THUMB_Y,
                    scroll_factor: factor,
                    scroll_extra: [own_slot, parent_slot],
                    area_slot: Some(area_base + area_idx as u32),
                    control_index: None,
                });
            }
            if cw > vw && vw > 0. {
                let bounds = [vx, (vy + vh - 5.).max(vy), vw * vw / cw, 5f32.min(vh)];
                let factor = vw / cw;
                list.command(5., bounds, super::scroll_areas::AREA_THUMB, 0., 0.);
                let offset = list.commands.len() - STRIDE;
                list.commands[offset + SCROLL_MODE] = SCROLL_AREA_THUMB_X;
                list.commands[offset + SCROLL_FACTOR] = factor;
                list.commands[offset + SCROLL_EXTRA] = own_slot;
                list.commands[offset + SCROLL_EXTRA + 1] = parent_slot;
                list.items.push(DisplayItem {
                    kind: DisplayItemKind::ScrollbarThumb,
                    bounds,
                    transform: Transform2D::IDENTITY,
                    clip_id: clip_top,
                    resource_id: None,
                    color: super::scroll_areas::AREA_THUMB,
                    radius: 0.,
                    stroke: 0.,
                    edge_range: [0, 0],
                    nonzero: false,
                    paint_slots: [0, 0],
                    scroll_mode: SCROLL_AREA_THUMB_X,
                    scroll_factor: factor,
                    scroll_extra: [own_slot, parent_slot],
                    area_slot: Some(area_base + area_idx as u32),
                    control_index: None,
                });
            }
        }
        if self.scene.scroll {
            list.mark_scrolled(content_start);
            list.mark_ir_scrolled(content_item_start, content_clip_start);
            list.pop_clip_node(&mut clip_top);
            list.scrollbars(v, self.content_size(), clip_top);
        }
        for (m, modal) in self.scene.modals.iter().enumerate() {
            if !self.modal_open[m] {
                continue;
            }
            let backdrop_radius = if self.scene.clip {
                0.
            } else {
                self.scene.radius
            };
            list.command(0., frame, modal.backdrop, backdrop_radius, 0.);
            list.items.push(DisplayItem {
                kind: DisplayItemKind::SolidRect,
                bounds: frame,
                transform: Transform2D::IDENTITY,
                clip_id: clip_top,
                resource_id: None,
                color: modal.backdrop,
                radius: backdrop_radius,
                stroke: 0.,
                edge_range: [0, 0],
                nonzero: false,
                paint_slots: [0, 0],
                scroll_mode: SCROLL_FIXED,
                scroll_factor: 0.,
                scroll_extra: [0.; 2],
                area_slot: None,
                control_index: None,
            });
            for index in modal.controls.clone() {
                emit(&mut list, index, clip_top);
            }
        }
        if self.scene.clip {
            list.pop_clip_node(&mut clip_top);
        }
        let list = Arc::new(list);
        *cache = Some(GeometryCache {
            scale,
            background,
            list: Arc::clone(&list),
        });
        list
    }
}
