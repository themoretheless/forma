//! Dependency-free bitmap HUD and structured telemetry card overlay renderer.

use crate::pipeline::{
    PipelineSpec, color_target, render_pipeline, viewport_uniform_resources, wgsl_module,
    write_growing_vertex_buffer, write_viewport_uniforms,
};
use crate::wgpu;
use bytemuck::{Pod, Zeroable};

/// A 2D screen-space HUD vertex with position in physical pixels and linear RGBA color.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct HudVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl HudVertex {
    pub(crate) const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: size_of::<[f32; 2]>() as u64,
                shader_location: 1,
            },
        ],
    };
}

const HUD_CARD_MARGIN: f32 = 16.0;
const HUD_CARD_GAP: f32 = 12.0;
const HUD_CARD_ACCENT: [f32; 4] = [0.16, 0.58, 0.92, 0.96];
const HUD_CARD_NOMINAL_VIEWPORT: [f32; 2] = [780.0, 620.0];
const HUD_CARD_DENSE_NOMINAL_VIEWPORT: [f32; 2] = [1600.0, 920.0];
const HUD_CARD_SINGLE_COLUMN_LIMIT: usize = 4;
const HUD_CARD_TWO_COLUMN_LIMIT: usize = 10;
const HUD_CARD_THREE_COLUMN_LIMIT: usize = 15;
const HUD_CARD_FOUR_COLUMN_LIMIT: usize = 24;
const HUD_CARD_COMPACT_MAX_WIDTH: f32 = 420.0;
const HUD_CARD_COMPACT_MAX_HEIGHT: f32 = 120.0;
/// Default card-view zoom: slightly reduced so long pipelines fit more of the
/// flow on screen; the user can zoom with Ctrl+wheel and scroll with the wheel.
const HUD_CARD_DEFAULT_ZOOM: f32 = 0.75;
const HUD_CARD_MIN_ZOOM: f32 = 0.3;
const HUD_CARD_MAX_ZOOM: f32 = 3.0;

/// One structured block in a telemetry HUD.
///
/// `time` is deliberately a string instead of a duration so callers can show
/// a measured value (for example `"14.2 MS"`) or a state such as `"WAITING"`.
/// The optional accent is an RGBA color in linear floating-point components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudCard<'a> {
    pub title: &'a str,
    pub time: &'a str,
    pub description: &'a str,
    pub status: Option<&'a str>,
    pub accent: Option<[f32; 4]>,
}

impl<'a> HudCard<'a> {
    pub const fn new(title: &'a str, time: &'a str, description: &'a str) -> Self {
        Self {
            title,
            time,
            description,
            status: None,
            accent: None,
        }
    }

    #[must_use]
    pub const fn with_status(mut self, status: &'a str) -> Self {
        self.status = Some(status);
        self
    }

    #[must_use]
    pub const fn with_accent(mut self, accent: [f32; 4]) -> Self {
        self.accent = Some(accent);
        self
    }
}

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

fn hud_card_scale(viewport: [f32; 2]) -> f32 {
    let width_scale = finite_dimension(viewport[0]) / HUD_CARD_NOMINAL_VIEWPORT[0];
    let height_scale = finite_dimension(viewport[1]) / HUD_CARD_NOMINAL_VIEWPORT[1];
    width_scale.min(height_scale).clamp(0.5, 4.0)
}

fn hud_card_scale_for_count(viewport: [f32; 2], card_count: usize) -> f32 {
    if card_count <= HUD_CARD_THREE_COLUMN_LIMIT {
        return hud_card_scale(viewport);
    }

    let width_scale = finite_dimension(viewport[0]) / HUD_CARD_DENSE_NOMINAL_VIEWPORT[0];
    let height_scale = finite_dimension(viewport[1]) / HUD_CARD_DENSE_NOMINAL_VIEWPORT[1];
    width_scale.min(height_scale).clamp(0.5, 4.0)
}

fn finite_dimension(value: f32) -> f32 {
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
struct HudCardMetrics {
    content_left_inset: f32,
    content_right_inset: f32,
    heading_scale: f32,
    heading_y_offset: f32,
    divider_y_offset: f32,
    body_scale: f32,
    status_y_offset: f32,
    description_y_offset: f32,
    description_without_status_y_offset: f32,
    bottom_inset: f32,
}

fn hud_card_metrics(bounds: HudCardBounds, scale: f32) -> HudCardMetrics {
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

fn hud_card_metrics_for_count(
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

/// Small dependency-free bitmap HUD renderer. Keeping the text rasterizer here
/// avoids coupling a viewer to a font stack while making timings and overlays
/// visible directly in the viewport.
#[derive(Debug)]
pub struct HudRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u64,
    vertex_count: u32,
    viewport: [f32; 2],
    card_zoom: f32,
    card_scroll: f32,
    card_max_scroll: f32,
}

impl HudRenderer {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        viewport: [f32; 2],
    ) -> Self {
        let shader = wgsl_module(device, "Timing HUD shader", HUD_SHADER);
        let (uniform_layout, uniform_buffer, uniform_bind_group) =
            viewport_uniform_resources(device, "Timing HUD", viewport);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Timing HUD pipeline layout"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });
        let pipeline = render_pipeline(
            device,
            PipelineSpec {
                label: "Timing HUD pipeline",
                layout: Some(&pipeline_layout),
                module: &shader,
                buffers: &[Some(HudVertex::LAYOUT)],
                primitive: wgpu::PrimitiveState::default(),
                depth: None,
                target: color_target(target_format, Some(wgpu::BlendState::ALPHA_BLENDING)),
            },
        );
        let vertex_capacity = 4096_u64;
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Timing HUD vertices"),
            size: vertex_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            uniform_buffer,
            uniform_bind_group,
            vertex_buffer,
            vertex_capacity,
            vertex_count: 0,
            viewport,
            card_zoom: HUD_CARD_DEFAULT_ZOOM,
            card_scroll: 0.0,
            card_max_scroll: 0.0,
        }
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, viewport: [f32; 2]) {
        self.viewport = write_viewport_uniforms(queue, &self.uniform_buffer, viewport);
    }

    /// Uploads caller-generated screen-space HUD vertices.
    pub fn write_vertices(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &str,
        vertices: &[HudVertex],
    ) {
        self.vertex_count = write_growing_vertex_buffer(
            device,
            queue,
            &mut self.vertex_buffer,
            &mut self.vertex_capacity,
            label,
            vertices,
        );
    }

    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, text: &str) {
        let lines: Vec<Vec<char>> = text
            .lines()
            .map(|line| line.chars().map(|ch| ch.to_ascii_uppercase()).collect())
            .collect();
        if lines.is_empty() {
            self.vertex_count = 0;
            return;
        }
        let scale = 2.5_f32;
        let padding = 12.0_f32;
        let cell_width = 6.0 * scale;
        let cell_height = 8.0 * scale;
        let max_chars = lines.iter().map(Vec::len).max().unwrap_or(0) as f32;
        let panel_width = (padding * 2.0 + max_chars * cell_width).min(self.viewport[0] - 16.0);
        let panel_height =
            (padding * 2.0 + lines.len() as f32 * cell_height).min(self.viewport[1] - 16.0);
        let mut vertices = Vec::new();
        push_quad(
            &mut vertices,
            8.0,
            8.0,
            panel_width.max(32.0),
            panel_height.max(24.0),
            [0.16, 0.58, 0.92, 0.96],
        );
        push_quad(
            &mut vertices,
            10.0,
            10.0,
            (panel_width - 4.0).max(28.0),
            (panel_height - 4.0).max(20.0),
            [0.004, 0.008, 0.016, 0.97],
        );
        for (line_index, line) in lines.iter().enumerate() {
            let y = 8.0 + padding + line_index as f32 * cell_height;
            for (char_index, character) in line.iter().enumerate() {
                let glyph = glyph_rows(*character);
                let x = 8.0 + padding + char_index as f32 * cell_width;
                for (row, bits) in glyph.iter().enumerate() {
                    for column in 0..5 {
                        if bits & (1 << (4 - column)) != 0 {
                            push_quad(
                                &mut vertices,
                                x + column as f32 * scale,
                                y + row as f32 * scale,
                                scale,
                                scale,
                                [0.96, 0.99, 1.0, 1.0],
                            );
                        }
                    }
                }
            }
        }
        self.write_vertices(device, queue, "Timing HUD vertices", &vertices);
    }

    /// Current zoom and scroll, retained by the host across device recreation.
    pub fn cards_view(&self) -> [f32; 2] {
        [self.card_zoom, self.card_scroll]
    }

    /// Restores absolute view values; `update_cards` reapplies current scroll bounds.
    pub fn restore_cards_view(&mut self, view: [f32; 2]) {
        if view[0].is_finite() && view[0] > 0.0 && view[1].is_finite() {
            self.card_zoom = view[0].clamp(HUD_CARD_MIN_ZOOM, HUD_CARD_MAX_ZOOM);
            self.card_scroll = view[1].max(0.0);
        }
    }

    /// Multiplies the card-view zoom, clamped to a readable range. Callers
    /// should rebuild the card buffer afterwards via [`Self::update_cards`].
    pub fn zoom_cards_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.card_zoom = (self.card_zoom * factor).clamp(HUD_CARD_MIN_ZOOM, HUD_CARD_MAX_ZOOM);
        }
    }

    /// Scrolls the card view vertically by `delta` pixels (positive moves the
    /// content up, revealing lower cards). Clamped to the last known content
    /// height; the bound is refreshed on every [`Self::update_cards`].
    pub fn scroll_cards_by(&mut self, delta: f32) {
        if delta.is_finite() {
            self.card_scroll = (self.card_scroll + delta).clamp(0.0, self.card_max_scroll);
        }
    }

    /// Restores the default reduced zoom and returns to the top of the flow.
    pub fn reset_cards_view(&mut self) {
        self.card_zoom = HUD_CARD_DEFAULT_ZOOM;
        self.card_scroll = 0.0;
    }

    /// Replaces the HUD contents with structured telemetry cards.
    ///
    /// Cards are laid out in input order and descriptions are word-wrapped to
    /// stay inside each card. Longer pipelines use up to five columns.
    /// This is independent of [`Self::update`], which retains the compact
    /// free-form HUD used by the main image viewport.
    pub fn update_cards(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cards: &[HudCard<'_>],
    ) {
        if cards.is_empty() {
            self.vertex_count = 0;
            return;
        }

        // Lay the flow out in a virtual viewport enlarged by 1/zoom so cards
        // shrink as the user zooms out, then apply the scroll offset. Quads
        // that land outside the surface are clipped by the rasterizer, but
        // fully off-screen cards are skipped to keep the vertex buffer small.
        let zoom = self.card_zoom;
        let virtual_viewport = [self.viewport[0] / zoom, self.viewport[1] / zoom];
        let mut bounds = hud_card_layout(virtual_viewport, cards.len());
        let base_scale = hud_card_scale_for_count(virtual_viewport, cards.len());
        let content_bottom = bounds
            .iter()
            .map(|b| b.y + b.height)
            .fold(0.0_f32, f32::max)
            + HUD_CARD_MARGIN * base_scale;
        self.card_max_scroll = (content_bottom * zoom - self.viewport[1]).max(0.0);
        self.card_scroll = self.card_scroll.clamp(0.0, self.card_max_scroll);
        let scroll = self.card_scroll;
        let scale = base_scale * zoom;
        for b in &mut bounds {
            b.x *= zoom;
            b.y = b.y * zoom - scroll;
            b.width *= zoom;
            b.height *= zoom;
        }

        let viewport_height = self.viewport[1];
        let mut vertices = Vec::new();
        for (card, bounds) in cards.iter().zip(bounds) {
            if bounds.y + bounds.height < 0.0 || bounds.y > viewport_height {
                continue;
            }
            push_hud_card(&mut vertices, card, bounds, scale, cards.len());
        }

        // Scroll position indicator along the right edge, shown only when the
        // flow overflows the viewport. The thumb size mirrors the visible
        // fraction and its position mirrors scroll progress.
        if self.card_max_scroll > 0.0 {
            let viewport_width = self.viewport[0];
            let content_height = content_bottom * zoom;
            let track_x = viewport_width - 7.0;
            let track_height = (viewport_height - 16.0).max(0.0);
            if track_height > 0.0 {
                push_quad(
                    &mut vertices,
                    track_x,
                    8.0,
                    3.0,
                    track_height,
                    [0.32, 0.38, 0.46, 0.45],
                );
                let thumb_height =
                    (track_height * viewport_height / content_height).clamp(20.0, track_height);
                let thumb_travel = track_height - thumb_height;
                let thumb_y = 8.0 + thumb_travel * (scroll / self.card_max_scroll);
                push_quad(
                    &mut vertices,
                    track_x,
                    thumb_y,
                    3.0,
                    thumb_height,
                    [0.62, 0.72, 0.84, 0.9],
                );
            }
        }

        self.write_vertices(device, queue, "Timing HUD vertices", &vertices);
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        self.encode_pass(encoder, target, wgpu::LoadOp::Load, None);
    }

    /// Draw a standalone HUD while clearing its background in the same pass.
    /// Existing `encode` keeps overlay/load semantics and skips empty geometry.
    pub fn encode_with_clear(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        color: wgpu::Color,
    ) {
        self.encode_pass(encoder, target, wgpu::LoadOp::Clear(color), None);
    }

    pub fn encode_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        if self.vertex_count == 0 && matches!(load, wgpu::LoadOp::Load) {
            return;
        }
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Timing HUD"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.draw(0..self.vertex_count, 0..1);
    }
}

fn push_hud_card(
    vertices: &mut Vec<HudVertex>,
    card: &HudCard<'_>,
    bounds: HudCardBounds,
    scale: f32,
    card_count: usize,
) {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }

    let accent = card.accent.unwrap_or(HUD_CARD_ACCENT);
    push_quad(
        vertices,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        accent,
    );
    let border = scale;
    if bounds.width > border * 2.0 && bounds.height > border * 2.0 {
        push_quad(
            vertices,
            bounds.x + border,
            bounds.y + border,
            bounds.width - border * 2.0,
            bounds.height - border * 2.0,
            [0.012, 0.022, 0.038, 0.98],
        );
        push_quad(
            vertices,
            bounds.x + border,
            bounds.y + border,
            (5.0 * scale).min(bounds.width - border * 2.0),
            bounds.height - border * 2.0,
            accent,
        );
    }

    let metrics = hud_card_metrics_for_count(bounds, scale, card_count);
    let content_left = bounds.x + metrics.content_left_inset;
    let content_right = bounds.x + bounds.width - metrics.content_right_inset;
    let content_width = (content_right - content_left).max(0.0);
    if content_width == 0.0 {
        return;
    }

    let heading_scale = metrics.heading_scale;
    let heading_cell_width = 6.0 * heading_scale;
    let time_limit = floor_to_usize((content_width * 0.4) / heading_cell_width);
    let time = take_chars(card.time, time_limit);
    let time_width = time.chars().count() as f32 * heading_cell_width;
    let title_width = (content_width - time_width - heading_cell_width).max(0.0);
    let title_limit = floor_to_usize(title_width / heading_cell_width);
    let heading_y = bounds.y + metrics.heading_y_offset;
    if heading_y + 7.0 * heading_scale <= bounds.y + bounds.height {
        push_hud_text(
            vertices,
            &take_chars(card.title, title_limit),
            content_left,
            heading_y,
            heading_scale,
            [0.96, 0.99, 1.0, 1.0],
        );
        push_hud_text(
            vertices,
            &time,
            content_right - time_width,
            heading_y,
            heading_scale,
            accent,
        );
    }

    let divider_y = bounds.y + metrics.divider_y_offset;
    if divider_y < bounds.y + bounds.height - scale {
        push_quad(
            vertices,
            content_left,
            divider_y,
            content_width,
            scale,
            [0.14, 0.18, 0.24, 0.9],
        );
    }

    let body_scale = metrics.body_scale;
    let body_cell_width = 6.0 * body_scale;
    let body_cell_height = 8.0 * body_scale;
    let body_limit = floor_to_usize(content_width / body_cell_width);
    let status = card.status.filter(|status| !status.is_empty());
    let status_y = bounds.y + metrics.status_y_offset;
    let description_y = if let Some(status) = status {
        if status_y + 7.0 * body_scale <= bounds.y + bounds.height {
            push_hud_text(
                vertices,
                &take_chars(status, body_limit),
                content_left,
                status_y,
                body_scale,
                accent,
            );
        }
        bounds.y + metrics.description_y_offset
    } else {
        bounds.y + metrics.description_without_status_y_offset
    };
    let description_height =
        (bounds.y + bounds.height - metrics.bottom_inset - description_y).max(0.0);
    let description_lines = floor_to_usize(description_height / body_cell_height);
    for (line_index, line) in wrap_hud_text(card.description, body_limit, description_lines)
        .iter()
        .enumerate()
    {
        push_hud_text(
            vertices,
            line,
            content_left,
            description_y + line_index as f32 * body_cell_height,
            body_scale,
            [0.68, 0.75, 0.84, 1.0],
        );
    }
}

pub fn take_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

pub fn wrap_hud_text(text: &str, max_chars: usize, max_lines: usize) -> Vec<String> {
    if max_chars == 0 || max_lines == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word: Vec<char> = word.chars().collect();
        if word.len() <= max_chars {
            let required = word.len() + usize::from(!current.is_empty());
            if current.chars().count() + required > max_chars {
                lines.push(std::mem::take(&mut current));
                if lines.len() == max_lines {
                    return lines;
                }
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.extend(word);
            continue;
        }

        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines {
                return lines;
            }
        }
        for chunk in word.chunks(max_chars) {
            if chunk.len() == max_chars {
                lines.push(chunk.iter().collect());
                if lines.len() == max_lines {
                    return lines;
                }
            } else {
                current.extend(chunk);
            }
        }
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    lines
}

pub fn push_hud_text(
    vertices: &mut Vec<HudVertex>,
    text: &str,
    x: f32,
    y: f32,
    scale: f32,
    color: [f32; 4],
) {
    for (character_index, character) in text.chars().enumerate() {
        let glyph = glyph_rows(character.to_ascii_uppercase());
        let glyph_x = x + character_index as f32 * 6.0 * scale;
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    push_quad(
                        vertices,
                        glyph_x + column as f32 * scale,
                        y + row as f32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
    }
}

pub fn push_quad(
    vertices: &mut Vec<HudVertex>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let x2 = x + width;
    let y2 = y + height;
    vertices.extend_from_slice(&[
        HudVertex {
            position: [x, y],
            color,
        },
        HudVertex {
            position: [x2, y],
            color,
        },
        HudVertex {
            position: [x2, y2],
            color,
        },
        HudVertex {
            position: [x, y],
            color,
        },
        HudVertex {
            position: [x2, y2],
            color,
        },
        HudVertex {
            position: [x, y2],
            color,
        },
    ]);
}

pub fn glyph_rows(character: char) -> [u8; 7] {
    match character {
        ' ' => [0; 7],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b11100,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00100, 0b01000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '=' => [
            0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '%' => [
            0b11001, 0b11010, 0b00100, 0b01000, 0b10110, 0b01011, 0b10011,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '×' => [
            0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b00000,
        ],
        _ => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    }
}

pub const HUD_SHADER: &str = r"
struct HudUniforms {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: HudUniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = input.position.x / uniforms.viewport.x * 2.0 - 1.0;
    let y = 1.0 - input.position.y / uniforms.viewport.y * 2.0;
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
";

#[cfg(test)]
mod tests {
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
        let description_height =
            cards[0].height - metrics.bottom_inset - metrics.description_y_offset;
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
        let description_height =
            cards[0].height - metrics.bottom_inset - metrics.description_y_offset;
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
        let description_height =
            bounds.height - metrics.bottom_inset - metrics.description_y_offset;
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
        let description_height =
            bounds.height - metrics.bottom_inset - metrics.description_y_offset;
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
}
