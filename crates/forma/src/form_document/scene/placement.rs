//! Measuring the expanded scene and positioning its controls.

use super::*;

impl Document {
    pub(in crate::form_document) fn scene_visual(&self, node: &mut Node) -> Result<Node, String> {
        if is_layout(&node.kind) {
            let visual = Node {
                kind: node.kind.clone(),
                props: node.props.clone(),
                children: node
                    .children
                    .iter_mut()
                    .map(|n| self.scene_visual(n))
                    .collect::<Result<_, _>>()?,
                action: node.action.clone(),
            };
            normalize_layout_owned(visual)
        } else {
            self.instantiate_scene(node).map(|(mut root, p)| {
                let orig = root.props.clone();
                root.props.extend(p);
                root.props
                    .insert("__rect_props".into(), Value::Object(orig));
                root
            })
        }
    }

    pub(in crate::form_document) fn position_scene(
        &self,
        mut node: Node,
        precomputed_visual: Option<Node>,
        bounds: [f64; 4],
        root_legacy: bool,
        target: &mut ScenePositionTarget<'_>,
    ) -> Result<(), String> {
        let scroll_content = target.scroll_content;
        // Oversized scroll content must overflow toward positive offsets; centering
        // it at a negative origin makes the leading half unreachable by scrolling.
        let align = |available: f64, size: f64| {
            let offset = (available - size) / 2.;
            if scroll_content {
                offset.max(0.)
            } else {
                offset
            }
        };
        if is_layout(&node.kind) {
            target.logical_end[0] = target.logical_end[0].max(bounds[0] + bounds[2]);
            target.logical_end[1] = target.logical_end[1].max(bounds[1] + bounds[3]);
        }
        if !is_layout(&node.kind) {
            let visual = match precomputed_visual {
                Some(v) => v,
                None => self.scene_visual(&mut node)?,
            };
            let width = dimension(&visual, 0, bounds[2])?;
            let height = dimension(&visual, 1, bounds[3])?;
            for (key, value) in [
                ("x", bounds[0] + align(bounds[2], width)),
                ("y", bounds[1] + align(bounds[3], height)),
                ("width", width),
                ("height", height),
            ] {
                node.props.insert(key.into(), number(value));
            }
            for key in ["cell", "row", "column"] {
                node.props.remove(key);
            }
            if node.action.is_none() {
                if let Some((action, args)) = &target.bubble {
                    node.action = Some(action.clone());
                    if let Some(args) = args {
                        node.props
                            .entry("__event_args".into())
                            .or_insert_with(|| args.clone());
                    }
                }
            }
            if let Some(clip) = target.clip {
                node.props
                    .insert(CLIP_PROPERTY.into(), Value::List(clip.map(number).to_vec()));
            }
            if !target.scroll_chain.is_empty() {
                let chain = target
                    .scroll_chain
                    .iter()
                    .map(|(area, [x, y, w, h])| {
                        Value::List([*area as f64, *x, *y, *w, *h].map(number).to_vec())
                    })
                    .collect();
                node.props
                    .insert(SCROLL_PROPERTY.into(), Value::List(chain));
            }
            target.instances.push((node, Some(visual)));
            return Ok(());
        }
        if !root_legacy && node.kind == "Scroll" {
            return self.position_scroll_area(node, bounds, target);
        }
        // A container's `clicked` handler takes clicks on its empty space through an invisible
        // pressable area under its children, and clicks on children without a handler of their own.
        let enclosing_bubble = target.bubble.clone();
        let capture = boolean(&node.props, "capture", false)?;
        if capture && node.action.is_none() {
            return Err("capture requires a clicked handler on the same container".into());
        }
        node.props.remove("capture");
        // With `capture: true` the area goes over the children instead, taking every click inside.
        let mut captured_area = None;
        if let Some(action) = node.action.take() {
            let args = node.props.get("__event_args").cloned();
            let mut props = Properties::new();
            props.insert("width".into(), number(bounds[2]));
            props.insert("height".into(), number(bounds[3]));
            if let Some(key) = node.props.get("key") {
                props.insert("key".into(), key.clone());
            }
            if let Some(args) = &args {
                props.insert("__event_args".into(), args.clone());
            }
            for key in ["__node", "__state"] {
                if let Some(value) = node.props.get(key) {
                    props.insert(key.into(), value.clone());
                }
            }
            let area = Node {
                kind: CONTAINER_AREA.into(),
                props,
                children: Vec::new(),
                action: Some(action.clone()),
            };
            if capture {
                captured_area = Some(area);
            } else {
                self.position_scene(area, None, bounds, false, target)?;
            }
            target.bubble = Some((action, args));
        }
        let enclosing_clip = target.clip;
        if !root_legacy && boolean(&node.props, "clip", false)? {
            target.clip = Some(match enclosing_clip {
                Some(outer) => intersect(outer, bounds),
                None => bounds,
            });
        }
        if root_legacy
            && ["Frame", "Scroll"].contains(&node.kind.as_str())
            && !node.props.contains_key("columns")
            && !node.props.contains_key("rows")
        {
            node.kind = "Column".into();
        }
        let mut node = normalize_layout_owned(node)?;
        let visual = match precomputed_visual {
            Some(v) if v.kind == node.kind && v.children.len() == node.children.len() => v,
            _ => self.scene_visual(&mut node)?,
        };
        let [mut x, mut y, mut width, mut height] = bounds;
        // A container consumes its allocated cell; its explicit dimensions are resolved by its parent.
        let pad = insets(node.props.get("padding"))?;
        x += pad[3];
        y += pad[0];
        width = (width - pad[1] - pad[3]).max(0.);
        height = (height - pad[0] - pad[2]).max(0.);
        let cols = sizes(&visual, 0, width)?;
        let rows = sizes(&visual, 1, height)?;
        let gap = gaps(&node.props)?;
        let mut col_offsets = Vec::with_capacity(cols.len());
        let mut col_acc = 0.0;
        for (i, &cw) in cols.iter().enumerate() {
            col_offsets.push(col_acc + i as f64 * gap[1]);
            col_acc += cw;
        }
        let mut row_offsets = Vec::with_capacity(rows.len());
        let mut row_acc = 0.0;
        for (i, &rh) in rows.iter().enumerate() {
            row_offsets.push(row_acc + i as f64 * gap[0]);
            row_acc += rh;
        }
        for (child, child_visual) in node.children.into_iter().zip(visual.children) {
            let ci = cell(&child, 0, cols.len())?;
            let ri = cell(&child, 1, rows.len())?;
            let cw = ci.map_or(width, |i| cols[i]);
            let ch = ri.map_or(height, |i| rows[i]);
            let mut cx = x + ci.map_or(0., |i| col_offsets[i]);
            let mut cy = y + ri.map_or(0., |i| row_offsets[i]);
            if is_layout(&child.kind) {
                let w = dimension(&child_visual, 0, cw)?;
                let h = dimension(&child_visual, 1, ch)?;
                cx += align(cw, w);
                cy += align(ch, h);
                self.position_scene(child, Some(child_visual), [cx, cy, w, h], false, target)?;
            } else {
                self.position_scene(child, Some(child_visual), [cx, cy, cw, ch], false, target)?;
            }
        }
        target.clip = enclosing_clip;
        target.bubble = enclosing_bubble;
        if let Some(area) = captured_area {
            self.position_scene(area, None, bounds, false, target)?;
        }
        Ok(())
    }
}

impl Document {
    /// Positions a `Scroll` below the root: its children are laid out like a column's, free to
    /// run past its bounds, which become a viewport that clips them and scrolls on its own.
    fn position_scroll_area(
        &self,
        mut node: Node,
        bounds: [f64; 4],
        target: &mut ScenePositionTarget<'_>,
    ) -> Result<(), String> {
        let area = target.scroll_content_sizes.len();
        target.scroll_content_sizes.push([bounds[2], bounds[3]]);
        let outer = (
            target.clip,
            target.scroll_content,
            target.logical_end.to_owned(),
        );
        target.clip = Some(match outer.0 {
            Some(clip) => intersect(clip, bounds),
            None => bounds,
        });
        target.scroll_content = true;
        *target.logical_end = [bounds[0] + bounds[2], bounds[1] + bounds[3]];
        target.scroll_chain.push((area, bounds));
        node.kind = "Column".into();
        node.props.remove("clip");
        let placed = self.position_scene(node, None, bounds, false, target);
        target.scroll_chain.pop();
        let end = *target.logical_end;
        target.scroll_content_sizes[area] = [
            (end[0] - bounds[0]).max(bounds[2]),
            (end[1] - bounds[1]).max(bounds[3]),
        ];
        (target.clip, target.scroll_content) = (outer.0, outer.1);
        *target.logical_end = outer.2;
        // The area itself occupies its bounds in whatever contains it.
        target.logical_end[0] = target.logical_end[0].max(bounds[0] + bounds[2]);
        target.logical_end[1] = target.logical_end[1].max(bounds[1] + bounds[3]);
        placed
    }
}

/// The part of `a` that `b` also covers; empty rectangles keep zero size at a valid origin.
fn intersect(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let x = a[0].max(b[0]);
    let y = a[1].max(b[1]);
    let right = (a[0] + a[2]).min(b[0] + b[2]);
    let bottom = (a[1] + a[3]).min(b[1] + b[3]);
    [x, y, (right - x).max(0.), (bottom - y).max(0.)]
}
