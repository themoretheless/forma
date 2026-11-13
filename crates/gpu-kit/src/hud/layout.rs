//! Where the HUD cards go: the grid, its scale and the metrics of one card.

pub(super) const HUD_CARD_MARGIN: f32 = 16.0;

pub(super) const HUD_CARD_GAP: f32 = 12.0;

pub(super) const HUD_CARD_ACCENT: [f32; 4] = [0.16, 0.58, 0.92, 0.96];

pub(super) const HUD_CARD_NOMINAL_VIEWPORT: [f32; 2] = [780.0, 620.0];

pub(super) const HUD_CARD_DENSE_NOMINAL_VIEWPORT: [f32; 2] = [1600.0, 920.0];

pub(super) const HUD_CARD_SINGLE_COLUMN_LIMIT: usize = 4;

pub(super) const HUD_CARD_TWO_COLUMN_LIMIT: usize = 10;

pub(super) const HUD_CARD_THREE_COLUMN_LIMIT: usize = 15;

pub(super) const HUD_CARD_FOUR_COLUMN_LIMIT: usize = 24;

pub(super) const HUD_CARD_COMPACT_MAX_WIDTH: f32 = 420.0;

pub(super) const HUD_CARD_COMPACT_MAX_HEIGHT: f32 = 120.0;

/// Default card-view zoom: slightly reduced so long pipelines fit more of the
/// flow on screen; the user can zoom with Ctrl+wheel and scroll with the wheel.
pub(super) const HUD_CARD_DEFAULT_ZOOM: f32 = 0.75;

pub(super) const HUD_CARD_MIN_ZOOM: f32 = 0.3;

pub(super) const HUD_CARD_MAX_ZOOM: f32 = 3.0;

/// Pixel bounds assigned to a structured HUD card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudCardBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Computes a responsive card layout without requiring a GPU device. Up to
/// four cards are stacked vertically, then longer pipelines progressively use
/// two, three, four or five columns. At the compact telemetry window's nominal
/// 780x620 size, four cards are 748x138 pixels with 12 pixel gaps and 16 pixel
/// outer margins. Pipelines with at least 16 stages use the dense dashboard's
/// 1600x920 nominal viewport so its geometry and text scale remain readable.
#[must_use]
pub fn hud_card_layout(viewport: [f32; 2], card_count: usize) -> Vec<HudCardBounds> {
    if card_count == 0 {
        return Vec::new();
    }

    let viewport_width = finite_dimension(viewport[0]);
    let viewport_height = finite_dimension(viewport[1]);
    let scale = hud_card_scale_for_count([viewport_width, viewport_height], card_count);
    let horizontal_margin = (HUD_CARD_MARGIN * scale).min(viewport_width * 0.5);
    let vertical_margin = (HUD_CARD_MARGIN * scale).min(viewport_height * 0.5);
    let content_width = (viewport_width - horizontal_margin * 2.0).max(0.0);
    let content_height = (viewport_height - vertical_margin * 2.0).max(0.0);
    let columns = match card_count {
        0..=HUD_CARD_SINGLE_COLUMN_LIMIT => 1,
        5..=HUD_CARD_TWO_COLUMN_LIMIT => 2,
        11..=HUD_CARD_THREE_COLUMN_LIMIT => 3,
        16..=HUD_CARD_FOUR_COLUMN_LIMIT => 4,
        _ => 5,
    };
    let rows = card_count.div_ceil(columns);
    let horizontal_gap_count = columns.saturating_sub(1) as f32;
    let vertical_gap_count = rows.saturating_sub(1) as f32;
    let horizontal_gap = if horizontal_gap_count == 0.0 {
        0.0
    } else {
        (HUD_CARD_GAP * scale).min(content_width / horizontal_gap_count)
    };
    let vertical_gap = if vertical_gap_count == 0.0 {
        0.0
    } else {
        (HUD_CARD_GAP * scale).min(content_height / vertical_gap_count)
    };
    let card_width =
        ((content_width - horizontal_gap * horizontal_gap_count) / columns as f32).max(0.0);
    let card_height = ((content_height - vertical_gap * vertical_gap_count) / rows as f32).max(0.0);

    (0..card_count)
        .map(|index| {
            let column = index % columns;
            let row = index / columns;
            HudCardBounds {
                x: horizontal_margin + column as f32 * (card_width + horizontal_gap),
                y: vertical_margin + row as f32 * (card_height + vertical_gap),
                width: card_width,
                height: card_height,
            }
        })
        .collect()
}

pub(super) fn hud_card_scale(viewport: [f32; 2]) -> f32 {
    let width_scale = finite_dimension(viewport[0]) / HUD_CARD_NOMINAL_VIEWPORT[0];
    let height_scale = finite_dimension(viewport[1]) / HUD_CARD_NOMINAL_VIEWPORT[1];
    width_scale.min(height_scale).clamp(0.5, 4.0)
}

pub(super) fn hud_card_scale_for_count(viewport: [f32; 2], card_count: usize) -> f32 {
    if card_count <= HUD_CARD_THREE_COLUMN_LIMIT {
        return hud_card_scale(viewport);
    }

    let width_scale = finite_dimension(viewport[0]) / HUD_CARD_DENSE_NOMINAL_VIEWPORT[0];
    let height_scale = finite_dimension(viewport[1]) / HUD_CARD_DENSE_NOMINAL_VIEWPORT[1];
    width_scale.min(height_scale).clamp(0.5, 4.0)
}

pub(super) fn finite_dimension(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[allow(clippy::cast_sign_loss)]
pub fn floor_to_usize(value: f32) -> usize {
    finite_dimension(value).floor() as usize
}

#[derive(Debug, Clone, Copy)]
pub(super) struct HudCardMetrics {
    pub(super) content_left_inset: f32,
    pub(super) content_right_inset: f32,
    pub(super) heading_scale: f32,
    pub(super) heading_y_offset: f32,
    pub(super) divider_y_offset: f32,
    pub(super) body_scale: f32,
    pub(super) status_y_offset: f32,
    pub(super) description_y_offset: f32,
    pub(super) description_without_status_y_offset: f32,
    pub(super) bottom_inset: f32,
}

pub(super) fn hud_card_metrics(bounds: HudCardBounds, scale: f32) -> HudCardMetrics {
    let compact = bounds.width <= HUD_CARD_COMPACT_MAX_WIDTH * scale
        || bounds.height <= HUD_CARD_COMPACT_MAX_HEIGHT * scale;
    let values = if compact {
        (14.0, 12.0, 2.0, 10.0, 30.0, 1.75, 36.0, 54.0, 37.0, 8.0)
    } else {
        (22.0, 18.0, 2.5, 14.0, 43.0, 2.0, 51.0, 75.0, 54.0, 12.0)
    };
    HudCardMetrics {
        content_left_inset: values.0 * scale,
        content_right_inset: values.1 * scale,
        heading_scale: values.2 * scale,
        heading_y_offset: values.3 * scale,
        divider_y_offset: values.4 * scale,
        body_scale: values.5 * scale,
        status_y_offset: values.6 * scale,
        description_y_offset: values.7 * scale,
        description_without_status_y_offset: values.8 * scale,
        bottom_inset: values.9 * scale,
    }
}

pub(super) fn hud_card_metrics_for_count(
    bounds: HudCardBounds,
    scale: f32,
    card_count: usize,
) -> HudCardMetrics {
    if card_count <= HUD_CARD_FOUR_COLUMN_LIMIT {
        return hud_card_metrics(bounds, scale);
    }

    HudCardMetrics {
        content_left_inset: 12.0 * scale,
        content_right_inset: 10.0 * scale,
        heading_scale: 1.75 * scale,
        heading_y_offset: 9.0 * scale,
        divider_y_offset: 29.0 * scale,
        body_scale: 1.5 * scale,
        status_y_offset: 35.0 * scale,
        description_y_offset: 52.0 * scale,
        description_without_status_y_offset: 36.0 * scale,
        bottom_inset: 8.0 * scale,
    }
}
