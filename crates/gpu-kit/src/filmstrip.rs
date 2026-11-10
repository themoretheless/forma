//! Bottom folder filmstrip overlay for viewer windows.
//!
//! The strip reuses the timing HUD's approach: an alpha-blended pass encoded
//! after the main viewport pass with [`wgpu::LoadOp::Load`], colored quads and
//! the crate's 5x7 bitmap font for placeholders and labels, plus one sampled
//! texture per ready thumbnail. Folder counts are small (sibling directories
//! of one parent), so each thumbnail keeps an individual texture and bind
//! group instead of an atlas.

use crate::hud::{HUD_SHADER, HudVertex, floor_to_usize, glyph_rows, push_quad, take_chars};
use crate::{GpuResourceLease, QueuedUploadReservation, wgpu};
use bytemuck::{Pod, Zeroable};
use memory_budget::{BufferError, MemoryBudget, Reservation};
use wgpu::util::DeviceExt;

/// Strip band height in physical pixels.
pub const STRIP_HEIGHT: f32 = 112.0;
/// Tile extent including the label line below the thumbnail.
pub const TILE_WIDTH: f32 = 144.0;
pub const TILE_HEIGHT: f32 = 96.0;
pub const TILE_GAP: f32 = 8.0;
pub const STRIP_PADDING: f32 = 8.0;
pub const TILE_STRIDE: f32 = TILE_WIDTH + TILE_GAP;
const THUMB_INSET: f32 = 4.0;
const THUMB_HEIGHT: f32 = 74.0;
const LABEL_SCALE: f32 = 1.5;
const BAND_COLOR: [f32; 4] = [0.010, 0.014, 0.020, 0.84];
const SEPARATOR_COLOR: [f32; 4] = [0.30, 0.36, 0.44, 0.85];
const TILE_COLOR: [f32; 4] = [0.045, 0.060, 0.085, 0.95];
const TILE_HIGHLIGHT_COLOR: [f32; 4] = [0.16, 0.58, 0.92, 0.98];
const PLACEHOLDER_COLOR: [f32; 4] = [0.020, 0.028, 0.040, 0.95];
const LABEL_COLOR: [f32; 4] = [0.78, 0.84, 0.92, 1.0];

/// Top edge of the strip band in physical pixels.
#[must_use]
pub fn strip_band_y(viewport_height: f32) -> f32 {
    (viewport_height - STRIP_HEIGHT).max(0.0)
}

/// Whether a physical cursor y lies inside the strip band.
#[must_use]
pub fn point_in_strip(y: f32, viewport_height: f32) -> bool {
    y >= strip_band_y(viewport_height) && y <= viewport_height
}

/// Left edge of tile `index` for a given scroll offset.
#[must_use]
pub fn tile_x(index: usize, scroll: f32) -> f32 {
    STRIP_PADDING + index as f32 * TILE_STRIDE - scroll
}

/// Hit-test a physical x coordinate against the tile row. Gaps between tiles
/// and the outer padding are dead zones.
#[must_use]
pub fn tile_index_at(x: f32, scroll: f32, tile_count: usize) -> Option<usize> {
    let local = x + scroll - STRIP_PADDING;
    if local < 0.0 {
        return None;
    }
    let index = floor_to_usize(local / TILE_STRIDE);
    if index >= tile_count || local - index as f32 * TILE_STRIDE > TILE_WIDTH {
        return None;
    }
    Some(index)
}

/// Largest useful scroll offset; zero when every tile fits.
#[must_use]
pub fn max_scroll(viewport_width: f32, tile_count: usize) -> f32 {
    (STRIP_PADDING * 2.0 + tile_count as f32 * TILE_STRIDE - TILE_GAP - viewport_width).max(0.0)
}

/// Adjust `scroll` (already clamped) so tile `index` is fully visible.
#[must_use]
pub fn scroll_to_reveal(index: usize, viewport_width: f32, tile_count: usize, scroll: f32) -> f32 {
    let max = max_scroll(viewport_width, tile_count);
    let scroll = scroll.clamp(0.0, max);
    if tile_x(index, scroll) < STRIP_PADDING {
        // Scroll left until the tile's left edge reaches the padding.
        return (index as f32 * TILE_STRIDE).clamp(0.0, max);
    }
    if tile_x(index, scroll) + TILE_WIDTH > viewport_width - STRIP_PADDING {
        // Scroll right until the tile's right edge reaches the padding.
        let right_scroll =
            STRIP_PADDING * 2.0 + index as f32 * TILE_STRIDE + TILE_WIDTH - viewport_width;
        return right_scroll.clamp(0.0, max);
    }
    scroll
}

/// Opaque handle to one uploaded thumbnail texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilmstripTileId(usize);

impl FilmstripTileId {
    /// Fabricate a handle without a GPU, for UI-state unit tests in other
    /// crates. Never valid for rendering.
    #[doc(hidden)]
    pub const fn from_raw_for_test(value: usize) -> Self {
        Self(value)
    }
}

/// One tile as laid out by the caller for the current frame.
#[derive(Debug, Clone)]
pub struct FilmstripTile {
    /// Scroll-adjusted left edge in physical pixels.
    pub x: f32,
    pub texture: Option<FilmstripTileId>,
    pub label: String,
    pub highlighted: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FilmstripUploadError {
    #[error("invalid thumbnail dimensions or RGBA8 payload")]
    Invalid,
    #[error(transparent)]
    Memory(#[from] BufferError),
    #[error("thumbnail texture admission: {0}")]
    TextureMemory(BufferError),
}

#[derive(Debug)]
struct TextureSlot {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    _reservation: Option<std::sync::Arc<Reservation>>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct TexVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

const TEXTURE_SHADER: &str = r"
struct StripUniforms {
    viewport: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: StripUniforms;
@group(1) @binding(0) var tile_texture: texture_2d<f32>;
@group(1) @binding(1) var tile_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let x = input.position.x / uniforms.viewport.x * 2.0 - 1.0;
    let y = 1.0 - input.position.y / uniforms.viewport.y * 2.0;
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    output.uv = input.uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(tile_texture, tile_sampler, input.uv);
}
";

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct StripUniforms {
    viewport: [f32; 2],
    _padding: [f32; 2],
}

#[derive(Debug)]
struct TexDraw {
    slot: usize,
    first_vertex: u32,
    vertex_count: u32,
}

/// Overlay renderer for the folder filmstrip. `update` rebuilds vertex data;
/// `encode` draws band, tiles and labels clipped to the strip band.
#[derive(Debug)]
pub struct FilmstripRenderer {
    color_pipeline: wgpu::RenderPipeline,
    texture_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    color_vertex_buffer: wgpu::Buffer,
    color_vertex_capacity: u64,
    color_vertex_count: u32,
    tex_vertex_buffer: wgpu::Buffer,
    tex_vertex_capacity: u64,
    tex_draws: Vec<TexDraw>,
    slots: Vec<Option<TextureSlot>>,
    free_slots: Vec<usize>,
    viewport: [f32; 2],
    upload_queue_budget: Option<MemoryBudget>,
    texture_budget: Option<MemoryBudget>,
}

impl FilmstripRenderer {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        viewport: [f32; 2],
    ) -> Self {
        let color_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Filmstrip color shader"),
            source: wgpu::ShaderSource::Wgsl(HUD_SHADER.into()),
        });
        let texture_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Filmstrip texture shader"),
            source: wgpu::ShaderSource::Wgsl(TEXTURE_SHADER.into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Filmstrip uniform layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Filmstrip tile texture layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let color_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Filmstrip color pipeline layout"),
                bind_group_layouts: &[Some(&uniform_layout)],
                immediate_size: 0,
            });
        let texture_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Filmstrip texture pipeline layout"),
                bind_group_layouts: &[Some(&uniform_layout), Some(&texture_bind_group_layout)],
                immediate_size: 0,
            });
        let color_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<HudVertex>() as u64,
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
        let tex_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<TexVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: size_of::<[f32; 2]>() as u64,
                    shader_location: 1,
                },
            ],
        };
        let color_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Filmstrip color pipeline"),
            layout: Some(&color_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &color_shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(color_vertex_layout)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &color_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let texture_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Filmstrip texture pipeline"),
            layout: Some(&texture_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &texture_shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(tex_vertex_layout)],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &texture_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = StripUniforms {
            viewport,
            _padding: [0.0; 2],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Filmstrip uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Filmstrip uniform bind group"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Filmstrip tile sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let color_vertex_capacity = 65536_u64;
        let tex_vertex_capacity = 65536_u64;
        Self {
            color_pipeline,
            texture_pipeline,
            uniform_buffer,
            uniform_bind_group,
            texture_bind_group_layout,
            sampler,
            color_vertex_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Filmstrip color vertices"),
                size: color_vertex_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            color_vertex_capacity,
            color_vertex_count: 0,
            tex_vertex_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Filmstrip texture vertices"),
                size: tex_vertex_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            tex_vertex_capacity,
            tex_draws: Vec::new(),
            slots: Vec::new(),
            free_slots: Vec::new(),
            upload_queue_budget: None,
            texture_budget: None,
            viewport,
        }
    }

    /// Conservatively snapshot all resident tile credits for the submitted strip.
    pub fn resource_lease(&self) -> GpuResourceLease {
        GpuResourceLease::new(
            self.slots
                .iter()
                .flatten()
                .filter_map(|slot| slot._reservation.clone()),
        )
    }

    pub fn resident_bytes(&self) -> u64 {
        self.slots
            .iter()
            .flatten()
            .map(|slot| u64::from(slot._texture.width()) * u64::from(slot._texture.height()) * 4)
            .sum()
    }

    /// Cap live RGBA8 texture payload; removing a slot releases its reservation.
    /// Driver overhead and in-flight retention are excluded.
    pub fn with_texture_budget(mut self, budget: MemoryBudget) -> Self {
        self.texture_budget = Some(budget);
        self
    }

    /// Admit aligned thumbnail queue payload until submitted GPU work completes.
    pub fn with_upload_queue_budget(mut self, budget: MemoryBudget) -> Self {
        self.upload_queue_budget = Some(budget);
        self
    }

    pub fn resize(&mut self, queue: &wgpu::Queue, viewport: [f32; 2]) {
        self.viewport = [viewport[0].max(1.0), viewport[1].max(1.0)];
        let uniforms = StripUniforms {
            viewport: self.viewport,
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Upload one RGBA8 thumbnail, returning its handle. Slots are recycled
    /// after [`Self::remove_tile`].
    pub fn upload_tile(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba8: &[u8],
    ) -> FilmstripTileId {
        self.try_upload_tile(device, queue, width, height, rgba8)
            .expect("thumbnail upload refused; use try_upload_tile for budgeted uploads")
    }

    /// Validates and admits before allocation; refusal leaves existing slots intact.
    pub fn try_upload_tile(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba8: &[u8],
    ) -> Result<FilmstripTileId, FilmstripUploadError> {
        let bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|n| n.checked_mul(4))
            .ok_or(FilmstripUploadError::Invalid)?;
        if width == 0
            || height == 0
            || width > device.limits().max_texture_dimension_2d
            || height > device.limits().max_texture_dimension_2d
            || bytes != rgba8.len() as u64
        {
            return Err(FilmstripUploadError::Invalid);
        }
        let reservation = self
            .texture_budget
            .as_ref()
            .map(|budget| budget.try_reserve(bytes))
            .transpose()
            .map_err(FilmstripUploadError::TextureMemory)?
            .map(std::sync::Arc::new);
        let _queued_upload = QueuedUploadReservation {
            queue: queue.clone(),
            resources: GpuResourceLease::new(reservation.iter().cloned()),
            reservation: self
                .upload_queue_budget
                .as_ref()
                .map(|budget| {
                    let row = (u64::from(width) * 4).div_ceil(256) * 256;
                    budget.try_reserve(row * u64::from(height))
                })
                .transpose()?,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Filmstrip tile texture"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba8,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width.max(1).saturating_mul(4)),
                rows_per_image: Some(height.max(1)),
            },
            wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Filmstrip tile bind group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let slot = TextureSlot {
            _texture: texture,
            bind_group,
            _reservation: reservation,
        };
        if let Some(index) = self.free_slots.pop() {
            self.slots[index] = Some(slot);
            Ok(FilmstripTileId(index))
        } else {
            self.slots.push(Some(slot));
            Ok(FilmstripTileId(self.slots.len() - 1))
        }
    }

    pub fn remove_tile(&mut self, id: FilmstripTileId) {
        if let Some(slot) = self.slots.get_mut(id.0)
            && slot.is_some()
        {
            *slot = None;
            self.free_slots.push(id.0);
        }
    }

    /// Rebuild strip vertex data for the current tile list. Tiles fully
    /// outside the band are skipped; partial tiles are clipped by the
    /// rasterizer/scissor in `encode`.
    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tiles: &[FilmstripTile]) {
        let viewport_width = self.viewport[0];
        let viewport_height = self.viewport[1];
        let band_y = strip_band_y(viewport_height);
        let mut colors: Vec<HudVertex> = Vec::new();
        let mut texs: Vec<TexVertex> = Vec::new();
        self.tex_draws.clear();
        if tiles.is_empty() {
            self.color_vertex_count = 0;
            return;
        }

        push_quad(
            &mut colors,
            0.0,
            band_y,
            viewport_width,
            STRIP_HEIGHT,
            BAND_COLOR,
        );
        push_quad(
            &mut colors,
            0.0,
            band_y,
            viewport_width,
            1.0,
            SEPARATOR_COLOR,
        );

        for tile in tiles {
            if tile.x + TILE_WIDTH < 0.0 || tile.x > viewport_width {
                continue;
            }
            let tile_y = band_y + STRIP_PADDING;
            let frame = if tile.highlighted {
                TILE_HIGHLIGHT_COLOR
            } else {
                TILE_COLOR
            };
            push_quad(&mut colors, tile.x, tile_y, TILE_WIDTH, TILE_HEIGHT, frame);
            let inner_x = tile.x + THUMB_INSET;
            let inner_y = tile_y + THUMB_INSET;
            let inner_width = TILE_WIDTH - THUMB_INSET * 2.0;
            if tile.highlighted {
                push_quad(
                    &mut colors,
                    inner_x,
                    inner_y,
                    inner_width,
                    TILE_HEIGHT - THUMB_INSET * 2.0,
                    TILE_COLOR,
                );
            }
            if let Some(texture) = tile.texture
                && self.slots.get(texture.0).is_some_and(Option::is_some)
            {
                let first_vertex = u32::try_from(texs.len()).unwrap_or(u32::MAX);
                push_tex_quad(&mut texs, inner_x, inner_y, inner_width, THUMB_HEIGHT);
                self.tex_draws.push(TexDraw {
                    slot: texture.0,
                    first_vertex,
                    vertex_count: 6,
                });
            } else {
                push_quad(
                    &mut colors,
                    inner_x,
                    inner_y,
                    inner_width,
                    THUMB_HEIGHT,
                    PLACEHOLDER_COLOR,
                );
            }
            let label_limit = take_chars(
                &tile.label,
                floor_to_usize(inner_width / (6.0 * LABEL_SCALE)),
            );
            push_strip_text(
                &mut colors,
                &label_limit,
                inner_x,
                tile_y + THUMB_INSET + THUMB_HEIGHT + 3.0,
                LABEL_SCALE,
                LABEL_COLOR,
            );
        }

        let color_bytes = bytemuck::cast_slice::<HudVertex, u8>(&colors);
        if color_bytes.len() as u64 > self.color_vertex_capacity {
            self.color_vertex_capacity = (color_bytes.len() as u64).next_power_of_two();
            self.color_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Filmstrip color vertices"),
                size: self.color_vertex_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.color_vertex_buffer, 0, color_bytes);
        self.color_vertex_count = u32::try_from(colors.len()).unwrap_or(u32::MAX);

        let tex_bytes = bytemuck::cast_slice::<TexVertex, u8>(&texs);
        if tex_bytes.len() as u64 > self.tex_vertex_capacity {
            self.tex_vertex_capacity = (tex_bytes.len() as u64).next_power_of_two();
            self.tex_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Filmstrip texture vertices"),
                size: self.tex_vertex_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !tex_bytes.is_empty() {
            queue.write_buffer(&self.tex_vertex_buffer, 0, tex_bytes);
        }
    }

    /// Draw the strip over the already-rendered viewport. A scissor rect
    /// confines both passes to the band so off-screen tiles never paint over
    /// the image above the strip.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if self.color_vertex_count == 0 {
            return;
        }
        let band_y = strip_band_y(self.viewport[1]);
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Filmstrip"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render_pass.set_scissor_rect(
            0,
            f32_to_px(band_y),
            f32_to_px(self.viewport[0].max(1.0)),
            f32_to_px(STRIP_HEIGHT.max(1.0)),
        );
        render_pass.set_pipeline(&self.color_pipeline);
        render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.color_vertex_buffer.slice(..));
        render_pass.draw(0..self.color_vertex_count, 0..1);
        if !self.tex_draws.is_empty() {
            render_pass.set_pipeline(&self.texture_pipeline);
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.tex_vertex_buffer.slice(..));
            for draw in &self.tex_draws {
                let Some(Some(slot)) = self.slots.get(draw.slot) else {
                    continue;
                };
                render_pass.set_bind_group(1, &slot.bind_group, &[]);
                render_pass.draw(
                    draw.first_vertex..draw.first_vertex + draw.vertex_count,
                    0..1,
                );
            }
        }
    }
}

fn f32_to_px(value: f32) -> u32 {
    u32::try_from(floor_to_usize(value)).unwrap_or(u32::MAX)
}

fn push_tex_quad(vertices: &mut Vec<TexVertex>, x: f32, y: f32, width: f32, height: f32) {
    let corners = [
        ([x, y], [0.0, 0.0]),
        ([x + width, y], [1.0, 0.0]),
        ([x + width, y + height], [1.0, 1.0]),
        ([x, y], [0.0, 0.0]),
        ([x + width, y + height], [1.0, 1.0]),
        ([x, y + height], [0.0, 1.0]),
    ];
    vertices.extend(
        corners
            .into_iter()
            .map(|(position, uv)| TexVertex { position, uv }),
    );
}

/// Bitmap text for strip labels; mirrors the HUD glyph rasterizer.
fn push_strip_text(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::validate_wgsl;

    #[test]
    fn texture_shader_parses_and_validates_with_naga() {
        validate_wgsl(TEXTURE_SHADER, wgpu::naga::valid::Capabilities::empty());
    }

    #[test]
    fn hit_testing_finds_tiles_and_rejects_gaps_and_padding() {
        // Tile 0 spans x in [8, 152]; the gap is (152, 160); tile 1 starts at 160.
        assert_eq!(tile_index_at(8.0, 0.0, 3), Some(0));
        assert_eq!(tile_index_at(151.9, 0.0, 3), Some(0));
        assert_eq!(tile_index_at(155.0, 0.0, 3), None, "gap is a dead zone");
        assert_eq!(tile_index_at(160.0, 0.0, 3), Some(1));
        assert_eq!(tile_index_at(7.9, 0.0, 3), None, "left padding");
        assert_eq!(tile_index_at(8.0, 0.0, 0), None, "no tiles");
        assert_eq!(tile_index_at(160.0, 0.0, 1), None, "past last tile");
        // Scrolled one full stride: tile 1 now starts at x=8.
        assert_eq!(tile_index_at(8.0, TILE_STRIDE, 3), Some(1));
    }

    #[test]
    fn max_scroll_is_zero_when_everything_fits() {
        assert!(max_scroll(2000.0, 3).abs() < 0.001);
        assert!(max_scroll(100.0, 3) > 0.0);
        // Content for 3 tiles: 8 + 3*152 - 8 + 8 = 464; max scroll = 464 - 100.
        assert!((max_scroll(100.0, 3) - 364.0).abs() < 0.001);
    }

    #[test]
    fn scroll_to_reveal_moves_only_when_needed() {
        // Tile 0 already visible: no movement.
        assert!(scroll_to_reveal(0, 500.0, 5, 0.0).abs() < 0.001);
        // Tile 4 at [616, 760] is past the 500 viewport: scroll right.
        let scroll = scroll_to_reveal(4, 500.0, 5, 0.0);
        assert!(tile_x(4, scroll) + TILE_WIDTH <= 500.0 - STRIP_PADDING + 0.001);
        // Scrolled far right, tile 0 is off-screen left: scroll back.
        let scroll = scroll_to_reveal(0, 500.0, 5, 300.0);
        assert!(tile_x(0, scroll) >= STRIP_PADDING - 0.001);
        // Reveal clamps to max_scroll instead of overshooting.
        let scroll = scroll_to_reveal(4, 500.0, 5, 0.0);
        assert!(scroll <= max_scroll(500.0, 5));
    }

    #[test]
    fn strip_band_hit_region() {
        assert!(point_in_strip(913.0, 1024.0));
        assert!(!point_in_strip(911.0, 1024.0));
        assert!(point_in_strip(1024.0, 1024.0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn thumbnail_queue_refusal_validation_slot_reuse_and_completion() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let budget = MemoryBudget::new(256);
        let mut strip =
            FilmstripRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb, [640.0, 480.0])
                .with_upload_queue_budget(budget.clone());
        for (w, h, data) in [
            (0, 1, vec![]),
            (1, 1, vec![0; 3]),
            (u32::MAX, u32::MAX, vec![]),
        ] {
            assert!(matches!(
                strip.try_upload_tile(&device, &queue, w, h, &data),
                Err(FilmstripUploadError::Invalid)
            ));
        }
        assert_eq!(budget.peak(), 0);
        let pressure = budget.try_reserve(256).unwrap();
        assert!(matches!(
            strip.try_upload_tile(&device, &queue, 1, 1, &[255; 4]),
            Err(FilmstripUploadError::Memory(_))
        ));
        drop(pressure);
        let first = strip
            .try_upload_tile(&device, &queue, 1, 1, &[255; 4])
            .unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(budget.used(), 0);
        assert!(matches!(
            strip.try_upload_tile(&device, &queue, 65, 1, &vec![255; 260]),
            Err(FilmstripUploadError::Memory(_))
        ));
        strip.remove_tile(first);
        let second = strip
            .try_upload_tile(&device, &queue, 1, 1, &[128; 4])
            .unwrap();
        assert_eq!(first, second);
        drop(strip);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 256);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hdr_filmstrip_thumbnail_is_linear_and_releases_resources() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let textures = MemoryBudget::new(4);
        let uploads = MemoryBudget::new(256);
        let mut strip =
            FilmstripRenderer::new(&device, wgpu::TextureFormat::Rgba16Float, [128., 128.])
                .with_texture_budget(textures.clone())
                .with_upload_queue_budget(uploads.clone());
        let id = strip
            .try_upload_tile(&device, &queue, 1, 1, &[128, 128, 128, 255])
            .unwrap();
        strip.update(
            &device,
            &queue,
            &[FilmstripTile {
                x: 0.,
                texture: Some(id),
                label: String::new(),
                highlighted: false,
            }],
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HDR filmstrip target"),
            size: wgpu::Extent3d {
                width: 128,
                height: 128,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HDR filmstrip readback"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        strip.encode(&mut encoder, &view);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 20, y: 40, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap()
        });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        {
            let mapped = slice.get_mapped_range().unwrap();
            for channel in 0..3 {
                let bits = u16::from_le_bytes([mapped[channel * 2], mapped[channel * 2 + 1]]);
                let exponent = i32::from((bits >> 10) & 31) - 15;
                let actual = (1. + f32::from(bits & 1023) / 1024.) * 2f32.powi(exponent);
                let expected = ((128f32 / 255. + 0.055) / 1.055).powf(2.4);
                assert!(
                    (actual - expected).abs() < 0.0005,
                    "HDR thumbnail must be linear: {actual} vs {expected}"
                );
            }
            assert_eq!(u16::from_le_bytes([mapped[6], mapped[7]]), 0x3c00);
        }
        buffer.unmap();
        strip.remove_tile(id);
        drop(strip);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!((textures.used(), uploads.used()), (0, 0));
        assert!(pollster::block_on(error_scope.pop()).is_none());
    }
}
