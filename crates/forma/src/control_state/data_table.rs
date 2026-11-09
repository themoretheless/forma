//! Data table state: sorting, column widths, filtering, selection and row windowing.

use super::*;

/// Column sort direction for [`DataTableModel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    None,
    Ascending,
    Descending,
}

/// Column descriptor for [`DataTableModel`].
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    pub id: String,
    pub width: f64,
    pub min_width: f64,
    pub max_width: f64,
}

/// Interactive state model for `DataTable`: multi-column sorting, column width resizing,
/// text filtering, single/multi row selection, keyboard navigation, and row virtualization.
#[derive(Clone, Debug, PartialEq)]
pub struct DataTableModel {
    columns: Vec<TableColumn>,
    rows: Vec<Vec<String>>,
    sort_column: Option<usize>,
    sort_direction: SortDirection,
    filter: String,
    visible_rows: Vec<usize>,
    selected_rows: BTreeSet<usize>,
    active_row: Option<usize>,
    scroll_y: f64,
    row_height: f64,
    viewport_height: f64,
}

impl DataTableModel {
    pub fn new(
        columns: Vec<TableColumn>,
        rows: Vec<Vec<String>>,
        row_height: f64,
        viewport_height: f64,
    ) -> Result<Self, ModelError> {
        if !row_height.is_finite()
            || row_height <= 0.0
            || !viewport_height.is_finite()
            || viewport_height < 0.0
        {
            return Err(ModelError::NonFiniteValue);
        }
        let mut model = Self {
            columns,
            rows,
            sort_column: None,
            sort_direction: SortDirection::None,
            filter: String::new(),
            visible_rows: Vec::new(),
            selected_rows: BTreeSet::new(),
            active_row: None,
            scroll_y: 0.0,
            row_height,
            viewport_height,
        };
        model.recompute_view();
        Ok(model)
    }

    fn recompute_view(&mut self) {
        let query = self.filter.to_lowercase();
        let mut indices: Vec<usize> = (0..self.rows.len())
            .filter(|&r| {
                query.is_empty()
                    || self.rows[r]
                        .iter()
                        .any(|cell| cell.to_lowercase().contains(&query))
            })
            .collect();
        if let Some(col) = self.sort_column {
            match self.sort_direction {
                SortDirection::Ascending => {
                    indices.sort_by(|&a, &b| {
                        let ca = self.rows[a].get(col).map(String::as_str).unwrap_or("");
                        let cb = self.rows[b].get(col).map(String::as_str).unwrap_or("");
                        match (ca.parse::<f64>(), cb.parse::<f64>()) {
                            (Ok(na), Ok(nb)) => na.total_cmp(&nb),
                            _ => ca.cmp(cb),
                        }
                    });
                }
                SortDirection::Descending => {
                    indices.sort_by(|&a, &b| {
                        let ca = self.rows[a].get(col).map(String::as_str).unwrap_or("");
                        let cb = self.rows[b].get(col).map(String::as_str).unwrap_or("");
                        match (ca.parse::<f64>(), cb.parse::<f64>()) {
                            (Ok(na), Ok(nb)) => nb.total_cmp(&na),
                            _ => cb.cmp(ca),
                        }
                    });
                }
                SortDirection::None => {}
            }
        }
        self.visible_rows = indices;
        if let Some(active) = self.active_row {
            if !self.visible_rows.contains(&active) {
                self.active_row = self.visible_rows.first().copied();
            }
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let max_scroll =
            ((self.visible_rows.len() as f64) * self.row_height - self.viewport_height).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);
    }

    pub fn visible_rows(&self) -> &[usize] {
        &self.visible_rows
    }

    pub fn active_row(&self) -> Option<usize> {
        self.active_row
    }

    pub fn sort_column(&self) -> Option<usize> {
        self.sort_column
    }

    pub fn sort_direction(&self) -> SortDirection {
        self.sort_direction
    }

    pub fn toggle_sort(&mut self, column: usize) -> Result<SortDirection, ModelError> {
        if column >= self.columns.len() {
            return Err(ModelError::UnknownItem(column));
        }
        if self.sort_column == Some(column) {
            self.sort_direction = match self.sort_direction {
                SortDirection::None => SortDirection::Ascending,
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::None,
            };
            if self.sort_direction == SortDirection::None {
                self.sort_column = None;
            }
        } else {
            self.sort_column = Some(column);
            self.sort_direction = SortDirection::Ascending;
        }
        self.recompute_view();
        Ok(self.sort_direction)
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.recompute_view();
    }

    pub fn resize_column(&mut self, column: usize, width: f64) -> Result<f64, ModelError> {
        if !width.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        let col = self
            .columns
            .get_mut(column)
            .ok_or(ModelError::UnknownItem(column))?;
        col.width = width.clamp(col.min_width, col.max_width);
        Ok(col.width)
    }

    pub fn column_width(&self, column: usize) -> Option<f64> {
        self.columns.get(column).map(|c| c.width)
    }

    pub fn select_row(&mut self, row_id: usize, additive: bool) -> Result<(), ModelError> {
        if row_id >= self.rows.len() {
            return Err(ModelError::UnknownItem(row_id));
        }
        if !additive {
            self.selected_rows.clear();
        }
        self.selected_rows.insert(row_id);
        self.active_row = Some(row_id);
        self.ensure_row_visible(row_id);
        Ok(())
    }

    pub fn selected_rows(&self) -> Vec<usize> {
        self.selected_rows.iter().copied().collect()
    }

    pub fn move_selection(&mut self, delta: isize) -> Option<usize> {
        if self.visible_rows.is_empty() {
            return None;
        }
        let pos = match self
            .active_row
            .and_then(|r| self.visible_rows.iter().position(|&v| v == r))
        {
            Some(idx) => idx
                .saturating_add_signed(delta)
                .min(self.visible_rows.len() - 1),
            None => {
                if delta >= 0 {
                    0
                } else {
                    self.visible_rows.len() - 1
                }
            }
        };
        let row_id = self.visible_rows[pos];
        self.selected_rows.clear();
        self.selected_rows.insert(row_id);
        self.active_row = Some(row_id);
        self.ensure_row_visible(row_id);
        Some(row_id)
    }

    pub fn ensure_row_visible(&mut self, row_id: usize) {
        if let Some(pos) = self.visible_rows.iter().position(|&r| r == row_id) {
            let top = (pos as f64) * self.row_height;
            let bottom = top + self.row_height;
            if top < self.scroll_y {
                self.scroll_y = top;
            } else if bottom > self.scroll_y + self.viewport_height {
                self.scroll_y = (bottom - self.viewport_height).max(0.0);
            }
            self.clamp_scroll();
        }
    }

    pub fn set_scroll_y(&mut self, scroll_y: f64) -> Result<f64, ModelError> {
        if !scroll_y.is_finite() {
            return Err(ModelError::NonFiniteValue);
        }
        self.scroll_y = scroll_y;
        self.clamp_scroll();
        Ok(self.scroll_y)
    }

    pub fn scroll_y(&self) -> f64 {
        self.scroll_y
    }

    /// Returns the `[start_visible_index, end_visible_index)` slice of `visible_rows` that intersect the viewport.
    pub fn virtual_window(&self, overscan: usize) -> Range<usize> {
        if self.visible_rows.is_empty() || self.viewport_height <= 0.0 {
            return 0..0;
        }
        let first = (self.scroll_y / self.row_height).floor().max(0.0) as usize;
        let count = (self.viewport_height / self.row_height).ceil().max(1.0) as usize;
        let start = first.saturating_sub(overscan).min(self.visible_rows.len());
        let end = (first + count + overscan).min(self.visible_rows.len());
        start..end
    }
}
