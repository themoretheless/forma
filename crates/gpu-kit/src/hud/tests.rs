use super::*;
use crate::test_support::validate_wgsl;

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= f32::EPSILON,
        "{actual} != {expected}"
    );
}

#[test]
fn four_cards_fill_the_nominal_telemetry_dashboard() {
    let cards = hud_card_layout([780.0, 620.0], 4);
    assert_eq!(cards.len(), 4);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[0].y, 16.0);
    assert_close(cards[0].width, 748.0);
    assert_close(cards[0].height, 138.0);
    assert_close(cards[1].y, 166.0);
    assert_close(cards[2].y, 316.0);
    assert_close(cards[3].y, 466.0);
    assert_close(cards[3].y + cards[3].height, 604.0);
}

#[test]
fn card_layout_scales_for_a_retina_surface() {
    let cards = hud_card_layout([1560.0, 1240.0], 4);
    assert_close(cards[0].x, 32.0);
    assert_close(cards[0].width, 1496.0);
    assert_close(cards[0].height, 276.0);
    assert_close(cards[1].y, 332.0);
}

#[test]
fn ten_cards_fill_a_two_column_five_row_grid() {
    let cards = hud_card_layout([780.0, 620.0], 10);
    assert_eq!(cards.len(), 10);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[0].y, 16.0);
    assert_close(cards[0].width, 368.0);
    assert_close(cards[0].height, 108.0);
    assert_close(cards[1].x, 396.0);
    assert_close(cards[1].y, 16.0);
    assert_close(cards[2].x, 16.0);
    assert_close(cards[2].y, 136.0);
    assert_close(cards[8].x, 16.0);
    assert_close(cards[8].y, 496.0);
    assert_close(cards[9].x, 396.0);
    assert_close(cards[9].y, 496.0);
    assert_close(cards[9].x + cards[9].width, 764.0);
    assert_close(cards[9].y + cards[9].height, 604.0);
}

#[test]
fn eleven_cards_use_three_columns_for_the_total_block() {
    let cards = hud_card_layout([780.0, 620.0], 11);
    let expected_width = (748.0 - 24.0) / 3.0;
    assert_eq!(cards.len(), 11);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[0].width, expected_width);
    assert_close(cards[1].x, 28.0 + expected_width);
    assert_close(cards[2].x, 40.0 + expected_width * 2.0);
    assert_close(cards[3].x, 16.0);
    assert_close(cards[3].y, 166.0);
    assert_close(cards[9].x, 16.0);
    assert_close(cards[9].y, 466.0);
    assert_close(cards[10].x, 28.0 + expected_width);
    assert_close(cards[10].y + cards[10].height, 604.0);
}

#[test]
fn twenty_four_cards_fill_a_four_column_six_row_dashboard() {
    let cards = hud_card_layout([1600.0, 920.0], 24);
    assert_eq!(cards.len(), 24);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[0].y, 16.0);
    assert_close(cards[0].width, 383.0);
    assert_close(cards[0].height, 138.0);
    assert_close(cards[1].x, 411.0);
    assert_close(cards[2].x, 806.0);
    assert_close(cards[3].x, 1201.0);
    assert_close(cards[4].x, 16.0);
    assert_close(cards[4].y, 166.0);
    assert_close(cards[20].y, 766.0);
    assert_close(cards[23].x + cards[23].width, 1584.0);
    assert_close(cards[23].y + cards[23].height, 904.0);
}

#[test]
fn twenty_eight_cards_fill_a_five_column_six_row_dashboard() {
    let cards = hud_card_layout([1600.0, 920.0], 28);
    assert_eq!(cards.len(), 28);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[0].y, 16.0);
    assert_close(cards[0].width, 304.0);
    assert_close(cards[0].height, 138.0);
    assert_close(cards[1].x, 332.0);
    assert_close(cards[2].x, 648.0);
    assert_close(cards[3].x, 964.0);
    assert_close(cards[4].x, 1280.0);
    assert_close(cards[5].x, 16.0);
    assert_close(cards[5].y, 166.0);
    assert_close(cards[27].x, 648.0);
    assert_close(cards[27].y, 766.0);
    assert_close(cards[4].x + cards[4].width, 1584.0);
    assert_close(cards[27].y + cards[27].height, 904.0);
}

#[test]
fn thirty_eight_cards_fill_a_five_column_eight_row_dashboard() {
    let cards = hud_card_layout([1600.0, 920.0], 38);
    assert_eq!(cards.len(), 38);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[0].y, 16.0);
    assert_close(cards[0].width, 304.0);
    assert_close(cards[0].height, 100.5);
    assert_close(cards[5].y, 128.5);
    assert_close(cards[35].y, 803.5);
    assert_close(cards[37].x, 648.0);
    assert_close(cards[37].y, 803.5);
    assert_close(cards[4].x + cards[4].width, 1584.0);
    assert_close(cards[37].y + cards[37].height, 904.0);

    let scale = hud_card_scale_for_count([1600.0, 920.0], 38);
    let metrics = hud_card_metrics_for_count(cards[0], scale, 38);
    let description_height = cards[0].height - metrics.bottom_inset - metrics.description_y_offset;
    let description_lines = floor_to_usize(description_height / (8.0 * metrics.body_scale));
    assert!(description_lines >= 3);
}

#[test]
fn forty_one_cards_keep_algorithm_descriptions_readable() {
    let viewport = [1600.0, 1080.0];
    let cards = hud_card_layout(viewport, 41);
    assert_eq!(cards.len(), 41);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[40].x, 16.0);
    assert_close(cards[40].y + cards[40].height, 1064.0);
    let scale = hud_card_scale_for_count(viewport, 41);
    let metrics = hud_card_metrics_for_count(cards[0], scale, 41);
    let description_height = cards[0].height - metrics.bottom_inset - metrics.description_y_offset;
    let description_lines = floor_to_usize(description_height / (8.0 * metrics.body_scale));
    assert!(description_lines >= 3);
}

#[test]
fn five_column_dashboard_scales_exactly_for_a_retina_surface() {
    let cards = hud_card_layout([3200.0, 1840.0], 28);
    assert_close(hud_card_scale_for_count([3200.0, 1840.0], 28), 2.0);
    assert_close(cards[0].x, 32.0);
    assert_close(cards[0].y, 32.0);
    assert_close(cards[0].width, 608.0);
    assert_close(cards[0].height, 276.0);
    assert_close(cards[1].x, 664.0);
    assert_close(cards[4].x, 2560.0);
    assert_close(cards[5].y, 332.0);
    assert_close(cards[27].x, 1296.0);
    assert_close(cards[27].y, 1532.0);
    assert_close(cards[4].x + cards[4].width, 3168.0);
    assert_close(cards[27].y + cards[27].height, 1808.0);
}

#[test]
fn odd_dense_card_count_keeps_row_major_order() {
    let cards = hud_card_layout([780.0, 620.0], 9);
    assert_eq!(cards.len(), 9);
    assert_close(cards[0].x, 16.0);
    assert_close(cards[1].x, 396.0);
    assert_close(cards[0].y, cards[1].y);
    assert_close(cards[8].x, 16.0);
    assert_close(cards[8].y, 496.0);
}

#[test]
fn dense_card_grid_scales_for_a_retina_surface() {
    let cards = hud_card_layout([1560.0, 1240.0], 10);
    assert_close(cards[0].x, 32.0);
    assert_close(cards[0].y, 32.0);
    assert_close(cards[0].width, 736.0);
    assert_close(cards[0].height, 216.0);
    assert_close(cards[1].x, 792.0);
    assert_close(cards[8].y, 992.0);
    assert_close(cards[9].x + cards[9].width, 1528.0);
    assert_close(cards[9].y + cards[9].height, 1208.0);
}

#[test]
fn compact_card_metrics_keep_dense_descriptions_readable() {
    let viewport = [780.0, 620.0];
    let bounds = hud_card_layout(viewport, 10)[0];
    let scale = hud_card_scale(viewport);
    let metrics = hud_card_metrics(bounds, scale);
    let content_width = bounds.width - metrics.content_left_inset - metrics.content_right_inset;
    let time_limit = floor_to_usize((content_width * 0.4) / (6.0 * metrics.heading_scale));
    let body_limit = floor_to_usize(content_width / (6.0 * metrics.body_scale));
    let description_height = bounds.height - metrics.bottom_inset - metrics.description_y_offset;
    let description_lines = floor_to_usize(description_height / (8.0 * metrics.body_scale));

    assert_close(metrics.heading_scale, 2.0);
    assert!(time_limit >= 10);
    assert!(body_limit >= 32);
    assert!(description_lines >= 3);
}

#[test]
fn five_column_metrics_preserve_timing_title_and_description_capacity() {
    let viewport = [1600.0, 920.0];
    let bounds = hud_card_layout(viewport, 28)[0];
    let scale = hud_card_scale_for_count(viewport, 28);
    let metrics = hud_card_metrics_for_count(bounds, scale, 28);
    let content_width = bounds.width - metrics.content_left_inset - metrics.content_right_inset;
    let heading_cell_width = 6.0 * metrics.heading_scale;
    let time_limit = floor_to_usize((content_width * 0.4) / heading_cell_width);
    let timing_width = "23.50 MS".chars().count() as f32 * heading_cell_width;
    let title_width = (content_width - timing_width - heading_cell_width).max(0.0);
    let title_limit = floor_to_usize(title_width / heading_cell_width);
    let body_limit = floor_to_usize(content_width / (6.0 * metrics.body_scale));
    let description_height = bounds.height - metrics.bottom_inset - metrics.description_y_offset;
    let description_lines = floor_to_usize(description_height / (8.0 * metrics.body_scale));

    assert_close(scale, 1.0);
    assert!(time_limit >= 10);
    assert!(title_limit >= 17);
    assert!(body_limit >= 31);
    assert!(description_lines >= 6);
}

#[test]
fn card_layout_is_safe_for_empty_and_tiny_viewports() {
    assert!(hud_card_layout([780.0, 620.0], 0).is_empty());
    let cards = hud_card_layout([8.0, 8.0], 4);
    assert_eq!(cards.len(), 4);
    assert!(cards.iter().all(|card| card.width >= 0.0));
    assert!(cards.iter().all(|card| card.height >= 0.0));
    assert!(
        cards
            .iter()
            .all(|card| card.x.is_finite() && card.y.is_finite())
    );

    let dense_cards = hud_card_layout([8.0, 8.0], 10);
    assert_eq!(dense_cards.len(), 10);
    assert!(dense_cards.iter().all(|card| card.width >= 0.0));
    assert!(dense_cards.iter().all(|card| card.height >= 0.0));
    assert!(
        dense_cards
            .iter()
            .all(|card| card.x.is_finite() && card.y.is_finite())
    );

    let maximal_cards = hud_card_layout([8.0, 8.0], 38);
    assert_eq!(maximal_cards.len(), 38);
    assert!(maximal_cards.iter().all(|card| card.width >= 0.0));
    assert!(maximal_cards.iter().all(|card| card.height >= 0.0));
    assert!(
        maximal_cards
            .iter()
            .all(|card| card.x.is_finite() && card.y.is_finite())
    );
}

#[test]
fn card_description_wraps_and_clips_by_line_count() {
    assert_eq!(
        wrap_hud_text("load source bytes and parse metadata", 12, 3),
        ["load source", "bytes and", "parse"]
    );
    assert_eq!(wrap_hud_text("abcdefgh", 3, 2), ["abc", "def"]);
    assert!(wrap_hud_text("description", 0, 4).is_empty());
}

#[test]
fn hud_space_is_blank_instead_of_unknown_glyph() {
    assert_eq!(glyph_rows(' '), [0; 7]);
    assert_eq!(glyph_rows(','), [0, 0, 0, 0, 0b00110, 0b00100, 0b01000]);
    assert_ne!(glyph_rows('?'), [0; 7]);
}

#[test]
fn hud_shader_parses_and_validates_with_naga() {
    validate_wgsl(HUD_SHADER, wgpu::naga::valid::Capabilities::empty());
}
