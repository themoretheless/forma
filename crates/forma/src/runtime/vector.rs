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
            });
            children.push(child);
        }
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
        }));
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
        // Children are built in content coordinates; the scroll offset is a
        // renderer uniform, so this list survives scrolling unchanged.
        let content_start = list.commands.len();
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
        let canvas_lists: Vec<Option<([f32; 4], Arc<DisplayList>)>> = (0..self.controls.len())
            .map(|i| Some((self.canvas_scene_rect(i)?, self.canvas_placed(i)?)))
            .collect();
        let emit = |list: &mut DisplayList, index: usize| {
            let clip = self.clip(index);
            if let Some(rect) = clip {
                list.push(rect, 0.);
            }
            let child = &children[index];
            let area_start = list.commands.len();
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
            // Application geometry follows its control, clipped to the canvas.
            if let Some((rect, placed)) = &canvas_lists[index] {
                let edge_offset = (list.edges.len() / 4) as f32;
                list.push(*rect, 0.);
                let start = list.commands.len();
                list.commands.extend(&placed.commands);
                for cmd in list.commands[start..].chunks_exact_mut(STRIDE) {
                    cmd[2] += edge_offset;
                }
                list.edges.extend(&placed.edges);
                list.pop();
            }
            let area = self.area_offset(index);
            if area != [0.; 2] {
                list.mark_area_scrolled(area_start, area);
            }
            if clip.is_some() {
                list.pop();
            }
        };
        for index in 0..self.controls.len() {
            if self.modal_of[index].is_none() {
                emit(&mut list, index);
            }
        }
        for thumb in self.area_thumbs() {
            list.command(5., thumb, super::scroll_areas::AREA_THUMB, 0., 0.);
        }
        if self.scene.scroll {
            list.mark_scrolled(content_start);
            list.scrollbars(v, self.content_size());
            list.pop();
        }
        for (m, modal) in self.scene.modals.iter().enumerate() {
            if !self.modal_open[m] {
                continue;
            }
            list.command(
                0.,
                frame,
                modal.backdrop,
                if self.scene.clip {
                    0.
                } else {
                    self.scene.radius
                },
                0.,
            );
            for index in modal.controls.clone() {
                emit(&mut list, index);
            }
        }
        if self.scene.clip {
            list.pop();
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
