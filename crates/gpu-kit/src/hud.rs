//! Dependency-free bitmap HUD and structured telemetry card overlay renderer.

use crate::pipeline::{
    PipelineSpec, color_target, render_pipeline, viewport_uniform_resources, wgsl_module,
    write_growing_vertex_buffer, write_viewport_uniforms,
};
use crate::wgpu;
use bytemuck::{Pod, Zeroable};

mod layout;
mod text;

pub use layout::*;
pub use text::*;

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
mod tests;
