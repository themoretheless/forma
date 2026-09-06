//! Native WebGPU/wgpu backend. Uploads vector commands/edges, never RGBA images.
use crate::Button;
use wgpu::util::DeviceExt;
pub const SHADER: &str = include_str!("vector.wgsl");

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: Option<wgpu::BindGroup>,
    geometry: Option<(f32, Vec<f32>, Vec<f32>)>,
    tile_key: Option<(u32, u32, f32)>,
    buffers: Vec<wgpu::Buffer>,
    pub uploads: u64,
    pub frames: u64,
    format: wgpu::TextureFormat,
    fault: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}
impl Renderer {
    pub async fn new(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> Result<Self, String> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| e.to_string())?;
        let fault = std::sync::Arc::new(std::sync::Mutex::new(None));
        let errors = fault.clone();
        device.on_uncaptured_error(std::sync::Arc::new(move |error| {
            *errors.lock().unwrap() = Some(format!("GPU validation/device error: {error}"));
        }));
        let lost = fault.clone();
        device.set_device_lost_callback(move |reason, message| {
            *lost.lock().unwrap() = Some(format!("GPU device lost: {reason:?}: {message}"));
        });
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Forma vector"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Forma vector coverage"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        if let Some(error) = validation.pop().await {
            return Err(error.to_string());
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Forma colors and viewport"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            device,
            queue,
            pipeline,
            uniform,
            bindings: None,
            geometry: None,
            tile_key: None,
            buffers: Vec::new(),
            uploads: 0,
            frames: 0,
            format,
            fault,
        })
    }
    pub fn draw(
        &mut self,
        model: &Button,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        scale: f32,
        background: bool,
        opaque: bool,
    ) -> Result<(), String> {
        if let Some(error) = self.fault.lock().unwrap().clone() {
            return Err(error);
        }
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || !scale.is_finite()
            || scale <= 0.
        {
            return Err("Invalid GPU viewport/DPI".into());
        }
        let list = model.vector_list(scale, background);
        let changed = self
            .geometry
            .as_ref()
            .is_none_or(|(s, c, e)| *s != scale || *c != list.commands || *e != list.edges);
        if changed || self.tile_key != Some((width, height, scale)) {
            let tiles = list.tiles(width, height, scale);
            let edges = if list.edges.is_empty() {
                &[0f32; 4][..]
            } else {
                &list.edges
            };
            let data: [&[u8]; 3] = [
                bytemuck::cast_slice(&list.commands),
                bytemuck::cast_slice(edges),
                bytemuck::cast_slice(&tiles),
            ];
            if data
                .iter()
                .any(|d| d.len() > self.device.limits().max_storage_buffer_binding_size as usize)
            {
                return Err("Vector scene exceeds GPU storage budget".into());
            }
            let make = |d: &[u8]| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Forma vector data"),
                        contents: d,
                        usage: wgpu::BufferUsages::STORAGE,
                    })
            };
            if changed {
                self.buffers = data.iter().map(|d| make(d)).collect();
                self.uploads += 1;
            } else {
                self.buffers[2] = make(data[2]);
            }
            self.bindings = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Forma vectors"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.buffers[0].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.buffers[1].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.buffers[2].as_entire_binding(),
                    },
                ],
            }));
            if changed {
                self.geometry = Some((scale, list.commands.clone(), list.edges.clone()));
            }
            self.tile_key = Some((width, height, scale));
        }
        let mut params = model.gpu_params(width, height, scale, opaque);
        if opaque && self.format.is_srgb() {
            params[3] = 2.;
        }
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&params));
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Forma vector frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, self.bindings.as_ref().unwrap(), &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.frames += 1;
        Ok(())
    }
}
