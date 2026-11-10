//! Virtualized list windowing with overscan and keyboard selection.

use super::*;

/// Interactive state model for `VirtualList`: virtualized viewport windowing with overscan,
/// scroll clamping, and keyboard item selection.
#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListModel {
    item_count: usize,
    item_height: f64,
    viewport_height: f64,
    overscan: usize,
    scroll_y: f64,
    selected: Option<usize>,
}

impl VirtualListModel {
    pub fn new(
        item_count: usize,
        item_height: f64,
        viewport_height: f64,
        overscan: usize,
    ) -> Result<Self, ModelError> {
        if !item_height.is_finite()
            || item_height <= 0.0
            || !viewport_height.is_finite()
            || viewport_height < 0.0
        {
            return Err(ModelError::NonFiniteValue);
        }
        Ok(Self {
            item_count,
            item_height,
            viewport_height,
            overscan,
            scroll_y: 0.0,
            selected: None,
        })
    }

    pub fn total_height(&self) -> f64 {
        (self.item_count as f64) * self.item_height
    }

    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    pub fn set_scroll_y(&mut self, scroll_y: f64) -> Result<f64, ModelError> {
        if !scroll_y.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        let max_scroll = (self.total_height() - self.viewport_height).max(0.0);
        self.scroll_y = scroll_y.clamp(0.0, max_scroll);
        Ok(self.scroll_y)
    }

    pub fn visible_range(&self) -> Range<usize> {
        if self.item_count == 0 || self.viewport_height <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_y / self.item_height).floor().max(0.0) as usize;
        let visible = (self.viewport_height / self.item_height).ceil().max(1.0) as usize;
        let start = first.saturating_sub(self.overscan).min(self.item_count);
        let end = (first + visible + self.overscan).min(self.item_count);
        start..end
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, index: usize) -> Result<usize, ModelError> {
        if index >= self.item_count {
            return Err(ModelError::UnknownItem(index));
        }
        self.selected = Some(index);
        self.ensure_visible(index);
        Ok(index)
    }

    pub fn move_by(&mut self, delta: isize) -> Option<usize> {
        if self.item_count == 0 {
            return None;
        }
        let next = match self.selected {
            Some(cur) => cur.saturating_add_signed(delta).min(self.item_count - 1),
            None => {
                if delta >= 0 {
                    0
                } else {
                    self.item_count - 1
                }
            }
        };
        let _ = self.select(next);
        self.selected
    }

    pub fn ensure_visible(&mut self, index: usize) {
        if index >= self.item_count {
            return;
        }
        let top = (index as f64) * self.item_height;
        let bottom = top + self.item_height;
        if top < self.scroll_y {
            let _ = self.set_scroll_y(top);
        } else if bottom > self.scroll_y + self.viewport_height {
            let _ = self.set_scroll_y((bottom - self.viewport_height).max(0.0));
        }
    }
}
