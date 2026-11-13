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
            target.instances.push((node, Some(visual)));
            return Ok(());
        }
        if !root_legacy && boolean(&node.props, "clip", false)? {
            return Err("Nested scene clipping is unsupported; use the root Frame clip".into());
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
        Ok(())
    }
}
