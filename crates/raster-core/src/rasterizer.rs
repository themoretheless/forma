//! Headless offscreen rasterizer. Renders a [`Frame`] (grid, deep-selection
//! underlay, meshes, edges, lines, selection overlay, instanced groups) into
//! a caller-provided texture view or a fresh texture via
//! [`Rasterizer::render_to_rgba`], mirroring the browser renderer's draw
//! semantics: opaque draws write depth, transparent draws blend without
//! depth writes, morph source bound at vertex slot 1, rest morph weight 1.

use crate::pipeline::RasterPipelines;
use crate::shaders::MORPH_VERTEX_STRIDE;
use crate::uniform::{SCENE_UNIFORM_BYTES, SCENE_UNIFORM_FLOATS, SceneUniform, shadow_light_vp};
use gpu_compute::{GpuContext, f32_le_bytes, wgpu};

pub use crate::resources::{
    DrawEdges, DrawMesh, InstancePool, InstancedGeometry, LineBatch, OverlayBatch,
};

/// Key-light shadow map resolution (depth32float, depth-only pass) — mirrors
/// the browser renderer's SHADOW_MAP_SIZE.
pub const SHADOW_MAP_SIZE: u32 = 1024;

/// One frame's worth of draws, composited in the browser renderer's order:
/// deep-selection underlay, grid, opaque meshes, instanced meshes,
/// transparent meshes, edges, lines, selection overlay.
#[derive(Default)]
pub struct Frame<'a> {
    pub clear: wgpu::Color,
    /// Fullscreen ground grid; requires `SceneUniform.inverse_vp` and
    /// `options.y` (grid step) to be set.
    pub grid: bool,
    /// Deep-selection x-ray overlay (selected mesh + its edges, drawn through
    /// everything after the meshes: depth compare `Always`, alpha blend).
    pub deep: Option<(&'a DrawMesh, &'a DrawEdges)>,
    pub meshes: &'a [&'a DrawMesh],
    /// (geometry, pool, first_instance, instance_count, transparent)
    pub instances: &'a [(&'a InstancedGeometry, &'a InstancePool, u32, u32, bool)],
    pub edges: &'a [&'a DrawEdges],
    pub lines: Option<&'a LineBatch>,
    pub overlay: Option<&'a OverlayBatch>,
}

pub struct Rasterizer {
    format: wgpu::TextureFormat,
    pub context: GpuContext,
    pub pipelines: RasterPipelines,
    scene_buffer: wgpu::Buffer,
    scene_bind_group: wgpu::BindGroup,
    morph_dummy: wgpu::Buffer,
    grid_quad: wgpu::Buffer,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    /// 1×1 dummy depth map bound where mesh-surface and grid shaders sample
    /// the key-light shadow map while shadows are off; combined with
    /// `shadow_params` disabled this keeps the default look pixel-identical.
    shadow_bind_group: wgpu::BindGroup,
    /// Comparison sampler shared by the dummy and the real shadow map.
    shadow_sampler: wgpu::Sampler,
    /// Real 1024² depth32float shadow map + its bind group, allocated lazily
    /// the first time shadows are enabled.
    shadow_map: Option<(wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
    /// Contact-shadow toggle (mirrors `setShadowsEnabled`). Default off.
    shadows_enabled: bool,
    /// Scene bounds (center, radius) feeding the light orthographic VP; the
    /// shadow pass stays inactive until both this and the toggle are set.
    shadow_bounds: Option<([f32; 3], f32)>,
    /// Resolved in `set_scene`: shadows requested AND bounds known AND the
    /// scene did not force `shadow_params` off.
    shadow_active: bool,
    /// Mirrors `SceneUniform.options.x`; gates the section-cap pass.
    section_enabled: bool,
    /// Cached key-light orthographic VP `(light, center, radius, vp)` to avoid recomputing when unchanged.
    cached_shadow_vp: Option<([f32; 3], [f32; 3], f32, [f32; 16])>,
    /// Last uploaded scene uniform floats to skip redundant GPU queue writes.
    last_scene_floats: Option<[f32; SCENE_UNIFORM_FLOATS]>,
}

impl Rasterizer {
    pub fn new(context: GpuContext, format: wgpu::TextureFormat) -> Self {
        let pipelines = RasterPipelines::new(&context.device, format);
        let scene_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster scene uniform"),
            size: SCENE_UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("raster scene bind group"),
                layout: &pipelines.scene_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene_buffer.as_entire_binding(),
                }],
            });
        let morph_dummy = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster morph dummy"),
            size: MORPH_VERTEX_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Fullscreen triangle-pair quad for the grid shader (vec2 corners).
        let grid_quad = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster grid quad"),
            size: 6 * 8,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        grid_quad
            .slice(..)
            .get_mapped_range_mut()
            .expect("mapped grid quad")
            .copy_from_slice(&f32_le_bytes(&[
                -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0,
            ]));
        grid_quad.unmap();
        // Shadow sampling shaders always have something bound: natively a 1×1
        // depth dummy + comparison sampler, with shadow_params disabled.
        let shadow_dummy = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("raster shadow dummy"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("raster shadow sampler"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("raster shadow bind group"),
                layout: &pipelines.shadow_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &shadow_dummy.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                    },
                ],
            });
        Self {
            format,
            context,
            pipelines,
            scene_buffer,
            scene_bind_group,
            morph_dummy,
            grid_quad,
            depth: None,
            shadow_bind_group,
            shadow_sampler,
            shadow_map: None,
            shadows_enabled: false,
            shadow_bounds: None,
            shadow_active: false,
            section_enabled: false,
            cached_shadow_vp: None,
            last_scene_floats: None,
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.context.device
    }

    /// Uploads the full scene uniform. When shadows are active (enabled via
    /// [`Rasterizer::set_shadows_enabled`] with bounds set, and the scene does
    /// not force `shadow_params` off), the key-light orthographic VP is
    /// computed from the bounds and the scene light direction and written over
    /// `light_vp`, with `shadow_params.x` forced to 1 — mirroring the browser
    /// renderer's scene-upload tail. Otherwise the shadow tail is zeroed and
    /// the frame stays pixel-identical to the unshadowed path.
    pub fn set_scene(&mut self, scene: &SceneUniform) {
        let mut floats = [0.0f32; SCENE_UNIFORM_FLOATS];
        scene.write_f32(&mut floats);
        // scene.shadow_params[0] < 0 forces off even when the toggle is on;
        // the default (0.0) simply follows the toggle.
        self.shadow_active = self.shadows_enabled
            && self.shadow_bounds.is_some()
            && self.shadow_map.is_some()
            && scene.shadow_params[0] >= 0.0;
        if self.shadow_active {
            let (center, radius) = self.shadow_bounds.unwrap();
            let light_dir = [scene.light[0], scene.light[1], scene.light[2]];
            let vp = match self.cached_shadow_vp {
                Some((cached_light, cached_center, cached_radius, cached_vp))
                    if cached_light == light_dir
                        && cached_center == center
                        && cached_radius == radius =>
                {
                    cached_vp
                }
                _ => {
                    let computed = shadow_light_vp(light_dir, center, radius);
                    self.cached_shadow_vp = Some((light_dir, center, radius, computed));
                    computed
                }
            };
            floats[76..92].copy_from_slice(&vp);
            floats[92] = 1.0;
            floats[93] = 1.0 / SHADOW_MAP_SIZE as f32;
            floats[94] = if scene.shadow_params[2] > 0.0 { scene.shadow_params[2] } else { 0.0015 };
            floats[95] = if scene.shadow_params[3] > 0.0 { scene.shadow_params[3] } else { 1.0 };
        } else {
            floats[76..92].fill(0.0);
            floats[92] = 0.0;
            floats[93] = 1.0 / SHADOW_MAP_SIZE as f32;
            floats[94] = 0.0015;
            floats[95] = 1.0;
        }
        let unchanged = self
            .last_scene_floats
            .as_ref()
            .is_some_and(|prev| prev == &floats);
        if !unchanged {
            self.context
                .queue
                .write_buffer(&self.scene_buffer, 0, &f32_le_bytes(&floats));
            self.last_scene_floats = Some(floats);
        }
        self.section_enabled = scene.options[0] > 0.5;
    }

    /// Whether contact shadows are currently toggled on (independent of
    /// whether bounds are known yet).
    pub fn shadows_enabled(&self) -> bool {
        self.shadows_enabled
    }

    /// Toggles the key-light contact-shadow pass. Enabling lazily allocates
    /// the real 1024² depth32float shadow map and rebinds it; disabling
    /// rebinds the 1×1 dummy and restores the pre-shadow look exactly.
    pub fn set_shadows_enabled(&mut self, enabled: bool) {
        if self.shadows_enabled == enabled {
            return;
        }
        self.shadows_enabled = enabled;
        if enabled && self.shadow_map.is_none() {
            let texture = self.context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("raster shadow map"),
                size: wgpu::Extent3d {
                    width: SHADOW_MAP_SIZE,
                    height: SHADOW_MAP_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self
                .context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("raster shadow map bind group"),
                    layout: &self.pipelines.shadow_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                        },
                    ],
                });
            self.shadow_map = Some((texture, view, bind_group));
        }
    }

    /// Sets the scene bounds (center, radius) used to build the key-light
    /// orthographic VP; `None` disables the shadow pass until bounds are set.
    pub fn set_shadow_bounds(&mut self, bounds: Option<([f32; 3], f32)>) {
        if self.shadow_bounds != bounds {
            self.shadow_bounds = bounds;
            self.cached_shadow_vp = None;
        }
    }

    /// The bind group the sampling shaders should read this frame: the real
    /// shadow map while active, the inert dummy otherwise.
    fn active_shadow_bg(&self) -> &wgpu::BindGroup {
        match (&self.shadow_map, self.shadow_active) {
            (Some((_, _, bind_group)), true) => bind_group,
            _ => &self.shadow_bind_group,
        }
    }

    fn ensure_depth(&mut self, width: u32, height: u32) {
        let recreate = self
            .depth
            .as_ref()
            .map(|(_, _, w, h)| *w != width || *h != height)
            .unwrap_or(true);
        if recreate {
            let texture = self
                .context
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("raster depth"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth24Plus,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.depth = Some((texture, view, width, height));
        }
    }

    /// Draws the frame into `target` (opaque first, then transparent).
    pub fn render(&mut self, target: &wgpu::TextureView, width: u32, height: u32, frame: &Frame) {
        let mut encoder =
            self.context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("raster"),
                });
        self.record(&mut encoder, target, width, height, frame);
        self.context.queue.submit([encoder.finish()]);
    }

    /// Records rendering into the caller's encoder without submitting. The
    /// encoder and target must belong to this rasterizer's device. This allows
    /// compute, copies and rendering to share an ordered command buffer.
    /// Uniform queue writes apply before submission, not between recorded frames.
    pub fn record(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        frame: &Frame,
    ) {
        self.ensure_depth(width, height);
        // Contact shadow: render opaque meshes' depth from the key light into
        // the shadow map before the main pass samples it (surfaces + grid
        // floor). No camera-frustum culling, mirroring the browser renderer.
        // Simplified vs the TS Tier 2 cache: re-rendered every frame while
        // active (no epoch tracking in the native API).
        if self.shadow_active {
            let (_, view, _) = self.shadow_map.as_ref().unwrap();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("raster shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.shadow);
            pass.set_bind_group(0, &self.scene_bind_group, &[]);
            for draw in frame.meshes.iter().filter(|d| !d.transparent) {
                pass.set_bind_group(1, &draw.bind_group, &[]);
                pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
                match &draw.morph_source {
                    Some(source) => pass.set_vertex_buffer(1, source.slice(..)),
                    None => pass.set_vertex_buffer(1, self.morph_dummy.slice(..)),
                }
                pass.set_index_buffer(draw.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..draw.index_count, 0, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("raster frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(frame.clear),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.as_ref().unwrap().1,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.scene_bind_group, &[]);

            if frame.grid {
                pass.set_pipeline(&self.pipelines.grid);
                pass.set_bind_group(1, self.active_shadow_bg(), &[]);
                pass.set_vertex_buffer(0, self.grid_quad.slice(..));
                pass.draw(0..6, 0..1);
            }

            for draw in frame.meshes.iter().filter(|d| !d.transparent) {
                self.draw_mesh_with(
                    &mut pass,
                    &self.pipelines.mesh_opaque,
                    &draw.vertex_buffer,
                    draw.index_count,
                    &draw.bind_group,
                    draw.morph_source.as_ref(),
                    &draw.index_buffer,
                );
            }

            if self.section_enabled {
                // Section cap: redraw the opaque meshes with the inverted-clip,
                // front-culled cap pipeline. The interior back faces of closed
                // solids survive and read as a filled, unlit cut surface.
                for draw in frame.meshes.iter().filter(|d| !d.transparent) {
                    self.draw_mesh_with(
                        &mut pass,
                        &self.pipelines.section_cap,
                        &draw.vertex_buffer,
                        draw.index_count,
                        &draw.bind_group,
                        draw.morph_source.as_ref(),
                        &draw.index_buffer,
                    );
                }
            }

            for (geometry, pool, start, count, transparent) in frame.instances {
                let pipeline = if *transparent {
                    &self.pipelines.instanced_mesh_transparent
                } else {
                    &self.pipelines.instanced_mesh_opaque
                };
                pass.set_pipeline(pipeline);
                pass.set_bind_group(1, &pool.bind_group, &[]);
                pass.set_bind_group(2, self.active_shadow_bg(), &[]);
                pass.set_vertex_buffer(0, geometry.vertex_buffer.slice(..));
                match &geometry.morph_source {
                    Some(source) => pass.set_vertex_buffer(1, source.slice(..)),
                    None => pass.set_vertex_buffer(1, self.morph_dummy.slice(..)),
                }
                pass.set_index_buffer(geometry.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..geometry.index_count, 0, *start..*start + *count);
            }

            for draw in frame.meshes.iter().filter(|d| d.transparent) {
                self.draw_mesh_with(
                    &mut pass,
                    &self.pipelines.mesh_transparent,
                    &draw.vertex_buffer,
                    draw.index_count,
                    &draw.bind_group,
                    draw.morph_source.as_ref(),
                    &draw.index_buffer,
                );
            }

            if let Some((deep_mesh, deep_edges)) = frame.deep {
                // Deep selection is an x-ray overlay: depth compare Always with
                // alpha blend, drawn after the meshes so it bleeds through.
                self.draw_mesh_with(
                    &mut pass,
                    &self.pipelines.deep_mesh,
                    &deep_mesh.vertex_buffer,
                    deep_mesh.index_count,
                    &deep_mesh.bind_group,
                    deep_mesh.morph_source.as_ref(),
                    &deep_mesh.index_buffer,
                );
                self.draw_edges_with(
                    &mut pass,
                    &self.pipelines.deep_edge,
                    deep_edges,
                    &deep_edges.index_buffer,
                );
            }

            for edges in frame.edges {
                self.draw_edges_with(&mut pass, &self.pipelines.edge, edges, &edges.index_buffer);
            }

            if let Some(lines) = frame.lines {
                pass.set_pipeline(&self.pipelines.line);
                pass.set_bind_group(1, self.active_shadow_bg(), &[]);
                pass.set_vertex_buffer(0, lines.vertex_buffer.slice(..));
                pass.draw(0..lines.vertex_count, 0..1);
            }

            if let Some(overlay) = frame.overlay {
                pass.set_pipeline(&self.pipelines.selection_overlay);
                pass.set_bind_group(1, self.active_shadow_bg(), &[]);
                pass.set_vertex_buffer(0, overlay.vertex_buffer.slice(..));
                pass.draw(0..overlay.vertex_count, 0..1);
            }
        }
    }

    fn draw_mesh_with<'p>(
        &self,
        pass: &mut wgpu::RenderPass<'p>,
        pipeline: &'p wgpu::RenderPipeline,
        vertex_buffer: &'p wgpu::Buffer,
        index_count: u32,
        bind_group: &'p wgpu::BindGroup,
        morph_source: Option<&'p wgpu::Buffer>,
        index_buffer: &'p wgpu::Buffer,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, bind_group, &[]);
        // Mesh-surface shaders sample the shadow map at group(2); the dummy
        // binding is inert while shadows are inactive, and pipelines whose
        // shader does not declare group(2) simply ignore it.
        pass.set_bind_group(2, self.active_shadow_bg(), &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        match morph_source {
            Some(source) => pass.set_vertex_buffer(1, source.slice(..)),
            None => pass.set_vertex_buffer(1, self.morph_dummy.slice(..)),
        }
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..index_count, 0, 0..1);
    }

    fn draw_edges_with<'p>(
        &self,
        pass: &mut wgpu::RenderPass<'p>,
        pipeline: &'p wgpu::RenderPipeline,
        edges: &'p DrawEdges,
        index_buffer: &'p wgpu::Buffer,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, &edges.bind_group, &[]);
        pass.set_bind_group(2, self.active_shadow_bg(), &[]);
        pass.set_vertex_buffer(0, edges.vertex_buffer.slice(..));
        match &edges.morph_source {
            Some(source) => pass.set_vertex_buffer(1, source.slice(..)),
            None => pass.set_vertex_buffer(1, self.morph_dummy.slice(..)),
        }
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..edges.index_count, 0, 0..1);
    }

    /// Compatibility convenience. Use `try_render_to_rgba` to recover errors.
    pub fn render_to_rgba(&mut self, width: u32, height: u32, frame: &Frame) -> Vec<u8> {
        self.try_render_to_rgba(width, height, frame)
            .expect("RGBA readback failed")
    }

    pub fn try_render_to_rgba(
        &mut self,
        width: u32,
        height: u32,
        frame: &Frame,
    ) -> Result<Vec<u8>, gpu_compute::ReadbackError> {
        self.render_rgba_async(width, height, frame)?
            .wait(std::time::Duration::from_secs(30))
    }

    /// Records the frame and its readback into one submission, without waiting.
    pub fn render_rgba_async(
        &mut self,
        width: u32,
        height: u32,
        frame: &Frame,
    ) -> Result<crate::readback::RgbaReadback, gpu_compute::ReadbackError> {
        if self.format != wgpu::TextureFormat::Rgba8Unorm {
            return Err(gpu_compute::ReadbackError::Device(
                "RGBA output requires an Rgba8Unorm rasterizer".into(),
            ));
        }
        let texture = crate::readback::RgbaReadback::target(&self.context.device, width, height)?;
        let view = texture.create_view(&Default::default());
        let mut encoder = self
            .context
            .device
            .create_command_encoder(&Default::default());
        self.record(&mut encoder, &view, width, height, frame);
        let mut ticket = crate::readback::RgbaReadback::record(
            &self.context.device,
            &mut encoder,
            &texture,
            width,
            height,
        )?;
        ticket.submitted(self.context.queue.submit([encoder.finish()]));
        Ok(ticket)
    }
}

/// Writes RGBA bytes as a binary PPM (P6) — dependency-free snapshot for
/// eyeballing headless frames.
pub fn write_ppm(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    write!(file, "P6\n{} {}\n255\n", width, height)?;
    // Pack RGB once and issue a single write instead of a 3-byte write per pixel.
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    file.write_all(&rgb)?;
    Ok(())
}
