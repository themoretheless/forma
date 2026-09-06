//! Native WebGPU/wgpu backend. Uploads vector commands/edges, never RGBA images.
use crate::{
    display_list::{DisplayList, TileScratch, TILE},
    Button,
};
use std::sync::Arc;
pub const SHADER: &str = include_str!("vector.wgsl");

/// Live resources owned by this renderer, not process VRAM usage or GPU residency.
/// Buffer sizes include wgpu's creation alignment. Driver heaps, in-flight staging,
/// pipeline memory, and the caller-owned render target/swapchain are not included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuResourceStats {
    pub buffer_count: usize,
    pub buffer_bytes: u64,
    pub uniform_bytes: u64,
    pub command_bytes: u64,
    pub edge_bytes: u64,
    pub tile_bytes: u64,
    pub timing_buffer_bytes: u64,
    pub bind_group_count: usize,
    pub render_pipeline_count: usize,
    pub query_set_count: usize,
    /// Cumulative application-created buffers, including ones already replaced.
    pub buffer_allocations_total: u64,
    pub buffer_allocation_bytes_total: u64,
    /// Application payload sent via initialized buffers and queue writes.
    pub uploaded_bytes_total: u64,
    pub geometry_uploads_total: u64,
    pub tile_uploads_total: u64,
    pub bind_group_creations_total: u64,
    /// Exclusive renderer geometry copies. Zero with shared immutable snapshots.
    pub cpu_geometry_cache_bytes: usize,
    /// Shared snapshot commands/edges capacity. Count once with the model, not
    /// again as exclusive renderer memory. Excludes Arc/Vec header overhead.
    pub cpu_shared_geometry_bytes: usize,
    /// Retained CPU tile-binning scratch capacities, not GPU memory.
    pub cpu_tile_scratch_bytes: usize,
}

/// Optional backend suballocator accounting. Not total adapter memory or residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendAllocatorStats {
    pub allocated_bytes: u64,
    pub reserved_bytes: u64,
    pub allocation_count: usize,
    pub block_count: usize,
}

struct GpuTiming {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    period_ns: f64,
    pending: bool,
}

fn timestamp_duration_ns(begin: u64, end: u64, period_ns: f64) -> Option<f64> {
    // A reversed pair is an invalid measurement, not evidence of a counter wrap.
    // Metal can report unavailable/error samples as all bits set. Never turn those
    // values (or an unwritten/equal pair) into an apparently valid GPU duration.
    if begin == u64::MAX || end == u64::MAX || !period_ns.is_finite() || period_ns <= 0. {
        return None;
    }
    let ticks = end.checked_sub(begin).filter(|ticks| *ticks != 0)?;
    let ns = ticks as f64 * period_ns;
    ns.is_finite().then_some(ns)
}

fn storage_capacity(required: u64, current: u64, limit: u64) -> Result<u64, String> {
    let limit = limit / wgpu::COPY_BUFFER_ALIGNMENT * wgpu::COPY_BUFFER_ALIGNMENT;
    let required = required.max(wgpu::COPY_BUFFER_ALIGNMENT);
    if required > limit || current > limit {
        return Err("Vector scene exceeds GPU storage budget".into());
    }
    if required <= current {
        return Ok(current);
    }
    // Amortize growth while never binding more bytes than the device allows.
    Ok(required
        .checked_next_power_of_two()
        .unwrap_or(limit)
        .min(limit))
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: Option<wgpu::BindGroup>,
    geometry: Option<Arc<DisplayList>>,
    tile_key: Option<(u32, u32, f32)>,
    tile_scratch: TileScratch,
    buffers: Vec<wgpu::Buffer>,
    pub uploads: u64,
    pub frames: u64,
    format: wgpu::TextureFormat,
    fault: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    buffer_allocations: u64,
    buffer_allocation_bytes: u64,
    uploaded_bytes: u64,
    tile_uploads: u64,
    bind_group_creations: u64,
    timing: Option<GpuTiming>,
}
impl Renderer {
    pub async fn new(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> Result<Self, String> {
        Self::new_with_profiling(adapter, format, false).await
    }

    /// Opt-in timestamp queries for benchmarks. Unsupported adapters render normally
    /// and report `gpu_timestamps_enabled() == false`; app rendering pays no overhead.
    pub async fn new_profiled(
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        Self::new_with_profiling(adapter, format, true).await
    }

    async fn new_with_profiling(
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
        profile: bool,
    ) -> Result<Self, String> {
        let timestamps = profile && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: if timestamps {
                    wgpu::Features::TIMESTAMP_QUERY
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            })
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
        let timing = timestamps.then(|| GpuTiming {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("Forma render-pass timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Forma timestamp resolve"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Forma timestamp readback"),
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            period_ns: queue.get_timestamp_period() as f64,
            pending: false,
        });
        let buffer_allocations = 1 + if timing.is_some() { 2 } else { 0 };
        let buffer_allocation_bytes = uniform.size()
            + timing
                .as_ref()
                .map_or(0, |t| t.resolve.size() + t.readback.size());
        Ok(Self {
            device,
            queue,
            pipeline,
            uniform,
            bindings: None,
            geometry: None,
            tile_key: None,
            tile_scratch: TileScratch::default(),
            buffers: Vec::with_capacity(3),
            uploads: 0,
            frames: 0,
            format,
            fault,
            buffer_allocations,
            buffer_allocation_bytes,
            uploaded_bytes: 0,
            tile_uploads: 0,
            bind_group_creations: 0,
            timing,
        })
    }

    /// Non-allocating snapshot of live owned capacities and cumulative operations.
    pub fn resource_stats(&self) -> GpuResourceStats {
        let bytes = |index| self.buffers.get(index).map_or(0, wgpu::Buffer::size);
        let uniform_bytes = self.uniform.size();
        let command_bytes = bytes(0);
        let edge_bytes = bytes(1);
        let tile_bytes = bytes(2);
        let timing_buffer_bytes = self
            .timing
            .as_ref()
            .map_or(0, |t| t.resolve.size() + t.readback.size());
        GpuResourceStats {
            buffer_count: 1 + self.buffers.len() + if self.timing.is_some() { 2 } else { 0 },
            buffer_bytes: uniform_bytes
                + command_bytes
                + edge_bytes
                + tile_bytes
                + timing_buffer_bytes,
            uniform_bytes,
            command_bytes,
            edge_bytes,
            tile_bytes,
            timing_buffer_bytes,
            bind_group_count: usize::from(self.bindings.is_some()),
            render_pipeline_count: 1,
            query_set_count: usize::from(self.timing.is_some()),
            buffer_allocations_total: self.buffer_allocations,
            buffer_allocation_bytes_total: self.buffer_allocation_bytes,
            uploaded_bytes_total: self.uploaded_bytes,
            geometry_uploads_total: self.uploads,
            tile_uploads_total: self.tile_uploads,
            bind_group_creations_total: self.bind_group_creations,
            cpu_geometry_cache_bytes: 0,
            cpu_shared_geometry_bytes: self.geometry.as_ref().map_or(0, |list| {
                (list.commands.capacity() + list.edges.capacity()) * std::mem::size_of::<f32>()
            }),
            cpu_tile_scratch_bytes: self.tile_scratch.capacity_bytes(),
        }
    }

    /// May allocate: call outside timed/allocation-counted benchmark intervals.
    /// `None` means unavailable, never zero usage. Metal commonly has no suballocator report.
    pub fn backend_allocator_stats(&self) -> Option<BackendAllocatorStats> {
        self.device
            .generate_allocator_report()
            .map(|report| BackendAllocatorStats {
                allocated_bytes: report.total_allocated_bytes,
                reserved_bytes: report.total_reserved_bytes,
                allocation_count: report.allocations.len(),
                block_count: report.blocks.len(),
            })
    }

    pub fn gpu_timestamps_enabled(&self) -> bool {
        self.timing.is_some()
    }

    /// Blocking read of the most recently submitted render pass duration in nanoseconds.
    /// Collect once per draw for per-frame samples. Excludes CPU submission, timestamp
    /// readback and OS presentation; may include backend render-pass scheduling effects.
    /// Unsupported queries or invalid timestamp pairs return `None`, never zero/wrapped time.
    /// Mapping allocates internally, so call outside CPU/allocation measurement intervals.
    pub fn read_gpu_duration_ns(&mut self) -> Result<Option<f64>, String> {
        let Some(timing) = self.timing.as_mut().filter(|t| t.pending) else {
            return Ok(None);
        };
        // Resolve only after the render submission completed. In particular, Metal
        // can otherwise resolve the previous end-of-fragment sample while the current
        // render pass is still in flight, yielding an impossible end < begin interval.
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Forma completed timestamp resolve"),
            });
        encoder.resolve_query_set(&timing.queries, 0..2, &timing.resolve, 0);
        encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.readback, 0, 16);
        self.queue.submit(Some(encoder.finish()));
        let (send, recv) = std::sync::mpsc::channel();
        timing
            .readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        recv.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let values = timing.readback.slice(..).get_mapped_range();
        let begin = u64::from_ne_bytes(values[..8].try_into().unwrap());
        let end = u64::from_ne_bytes(values[8..16].try_into().unwrap());
        drop(values);
        timing.readback.unmap();
        timing.pending = false;
        Ok(timestamp_duration_ns(begin, end, timing.period_ns))
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
        let list = model.vector_snapshot(scale, background);
        let changed = self
            .geometry
            .as_ref()
            .is_none_or(|cached| !Arc::ptr_eq(cached, &list));
        // The binner depends on the number of tiles, not exact viewport pixels.
        // Sub-tile resize changes only uniforms, retaining the same valid tile data.
        let tile_key = (width.div_ceil(TILE), height.div_ceil(TILE), scale);
        if changed || self.tile_key != Some(tile_key) {
            let tiles = list.tiles_into(width, height, scale, &mut self.tile_scratch);
            let edges = if list.edges.is_empty() {
                &[0f32; 4][..]
            } else {
                &list.edges
            };
            let data: [&[u8]; 3] = [
                bytemuck::cast_slice(&list.commands),
                bytemuck::cast_slice(edges),
                bytemuck::cast_slice(tiles),
            ];
            let limits = self.device.limits();
            let storage_limit =
                (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
            if data.iter().any(|d| d.len() as u64 > storage_limit) {
                return Err("Vector scene exceeds GPU storage budget".into());
            }
            let mut rebind = self.bindings.is_none();
            for (index, payload) in data.iter().enumerate() {
                if !changed && index != 2 {
                    continue;
                }
                let current = self.buffers.get(index).map_or(0, wgpu::Buffer::size);
                let capacity = storage_capacity(payload.len() as u64, current, storage_limit)?;
                if capacity != current {
                    let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(["Forma commands", "Forma edges", "Forma tiles"][index]),
                        size: capacity,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.buffer_allocations += 1;
                    self.buffer_allocation_bytes += buffer.size();
                    if index == self.buffers.len() {
                        self.buffers.push(buffer);
                    } else {
                        self.buffers[index] = buffer;
                    }
                    rebind = true;
                }
                self.queue.write_buffer(&self.buffers[index], 0, payload);
                self.uploaded_bytes += payload.len() as u64;
            }
            if changed {
                self.uploads += 1;
            }
            self.tile_uploads += 1;
            if rebind {
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
                self.bind_group_creations += 1;
            }
            if changed {
                self.geometry = Some(Arc::clone(&list));
            }
            self.tile_key = Some(tile_key);
        }
        let mut params = model.gpu_params(width, height, scale, opaque);
        if opaque && self.format.is_srgb() {
            params[3] = 2.;
        }
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&params));
        self.uploaded_bytes += std::mem::size_of_val(params.as_slice()) as u64;
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
                timestamp_writes: self
                    .timing
                    .as_ref()
                    .map(|t| wgpu::RenderPassTimestampWrites {
                        query_set: &t.queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, self.bindings.as_ref().unwrap(), &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(timing) = &mut self.timing {
            timing.pending = true;
        }
        self.queue.submit(Some(encoder.finish()));
        self.frames += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_timestamp_pairs_are_unavailable_not_wrapped_durations() {
        assert_eq!(timestamp_duration_ns(100, 150, 2.), Some(100.));
        assert_eq!(timestamp_duration_ns(150, 100, 1.), None);
        assert_eq!(timestamp_duration_ns(0, 0, 1.), None);
        assert_eq!(timestamp_duration_ns(100, u64::MAX, 1.), None);
        assert_eq!(timestamp_duration_ns(u64::MAX, 100, 1.), None);
        assert_eq!(timestamp_duration_ns(100, 150, f64::NAN), None);
        assert_eq!(timestamp_duration_ns(100, 150, 0.), None);
    }

    #[test]
    fn storage_capacity_grows_amortized_without_exceeding_binding_budget() {
        assert_eq!(storage_capacity(80, 0, 1024).unwrap(), 128);
        assert_eq!(storage_capacity(100, 128, 1024).unwrap(), 128);
        assert_eq!(storage_capacity(32, 128, 1024).unwrap(), 128);
        assert_eq!(storage_capacity(129, 128, 1024).unwrap(), 256);
        assert_eq!(storage_capacity(513, 512, 768).unwrap(), 768);
        assert_eq!(storage_capacity(0, 0, 16).unwrap(), 4);
        assert!(storage_capacity(769, 512, 768).is_err());
        assert!(storage_capacity(767, 512, 767).is_err());
        assert!(storage_capacity(16, 1024, 512).is_err());
    }

    #[test]
    #[ignore = "requires a real GPU; run explicitly with --features gpu -- --ignored"]
    fn profiled_renderer_reports_owned_buffers_and_reuses_them() {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance.request_adapter(&Default::default()).await.unwrap();
            let mut renderer = Renderer::new_profiled(&adapter, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
            let mut model = Button::from_sources(crate::EXAMPLE, crate::BUTTON_COMPONENT).unwrap();
            let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Forma profiling smoke test target"),
                size: wgpu::Extent3d {
                    width: 96,
                    height: 64,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let target = texture.create_view(&Default::default());
            assert_eq!(renderer.read_gpu_duration_ns().unwrap(), None);
            let initial = renderer.resource_stats();
            assert_eq!(initial.buffer_bytes, 48 + initial.timing_buffer_bytes);
            assert_eq!(
                initial.buffer_allocations_total,
                initial.buffer_count as u64
            );
            assert_eq!(initial.bind_group_count, 0);
            for _ in 0..2 {
                renderer
                    .draw(&model, &target, 96, 64, 1., true, true)
                    .unwrap();
                let duration = renderer.read_gpu_duration_ns().unwrap();
                if !renderer.gpu_timestamps_enabled() {
                    assert_eq!(duration, None);
                }
                if let Some(ns) = duration {
                    assert!(ns.is_finite() && ns >= 0., "invalid GPU duration: {ns}");
                }
                assert_eq!(renderer.read_gpu_duration_ns().unwrap(), None);
            }
            let stats = renderer.resource_stats();
            assert_eq!(stats.buffer_count, initial.buffer_count + 3);
            assert_eq!(
                stats.buffer_allocations_total,
                initial.buffer_allocations_total + 3
            );
            assert_eq!(stats.geometry_uploads_total, 1);
            assert_eq!(stats.tile_uploads_total, 1);
            assert_eq!(stats.bind_group_count, 1);
            assert_eq!(
                stats.buffer_bytes,
                stats.uniform_bytes
                    + stats.command_bytes
                    + stats.edge_bytes
                    + stats.tile_bytes
                    + stats.timing_buffer_bytes
            );
            assert_eq!(stats.buffer_allocation_bytes_total, stats.buffer_bytes);
            assert!(stats.uploaded_bytes_total >= 2 * stats.uniform_bytes);
            assert_eq!(stats.cpu_geometry_cache_bytes, 0);
            assert!(stats.cpu_shared_geometry_bytes > 0);
            assert!(stats.cpu_tile_scratch_bytes > 0);
            assert_eq!(stats.bind_group_creations_total, 1);

            // Same tile grid: viewport pixels change, no tile upload or allocation.
            renderer
                .draw(&model, &target, 95, 63, 1., true, true)
                .unwrap();
            let sub_tile = renderer.resource_stats();
            assert_eq!(sub_tile.tile_uploads_total, stats.tile_uploads_total);
            assert_eq!(
                sub_tile.buffer_allocations_total,
                stats.buffer_allocations_total
            );
            assert_eq!(
                sub_tile.bind_group_creations_total,
                stats.bind_group_creations_total
            );

            // Replacing geometry with a smaller scene must keep all three capacities.
            let smaller = Button::from_sources(
                "component Demo { Frame { width:64; height:64; padding:0; Button { width:64; height:64; } } }",
                "component Button { Rectangle { radius:8; background:#ffffff; } }",
            ).unwrap();
            renderer
                .draw(&smaller, &target, 96, 64, 1., true, true)
                .unwrap();
            let shrunk = renderer.resource_stats();
            assert_eq!(shrunk.buffer_bytes, stats.buffer_bytes);
            assert_eq!(
                shrunk.buffer_allocations_total,
                stats.buffer_allocations_total
            );
            assert_eq!(
                shrunk.bind_group_creations_total,
                stats.bind_group_creations_total
            );
            renderer
                .draw(&model, &target, 96, 64, 1., true, true)
                .unwrap();
            let restored = renderer.resource_stats();
            assert_eq!(restored.buffer_bytes, stats.buffer_bytes);
            assert_eq!(
                restored.buffer_allocations_total,
                stats.buffer_allocations_total
            );
            assert_eq!(
                restored.bind_group_creations_total,
                stats.bind_group_creations_total
            );

            // Growing the viewport enough needs a larger tile buffer exactly once;
            // returning to a smaller viewport retains that allocation and bind group.
            let large_texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Forma profiling resize test target"),
                size: wgpu::Extent3d {
                    width: 1024,
                    height: 1024,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            renderer
                .draw(
                    &model,
                    &large_texture.create_view(&Default::default()),
                    1024,
                    1024,
                    1.,
                    true,
                    true,
                )
                .unwrap();
            let grown = renderer.resource_stats();
            assert_eq!(
                grown.buffer_allocations_total,
                stats.buffer_allocations_total + 1
            );
            assert_eq!(
                grown.bind_group_creations_total,
                stats.bind_group_creations_total + 1
            );
            assert!(grown.tile_bytes > stats.tile_bytes);
            renderer
                .draw(&model, &target, 96, 64, 1., true, true)
                .unwrap();
            let back = renderer.resource_stats();
            assert_eq!(back.buffer_bytes, grown.buffer_bytes);
            assert_eq!(
                back.buffer_allocations_total,
                grown.buffer_allocations_total
            );
            assert_eq!(
                back.bind_group_creations_total,
                grown.bind_group_creations_total
            );
            let stable_uploads = renderer.uploads;
            model.pointer(100., 90., 0);
            model.focus(true);
            model.tick(16.);
            renderer
                .draw(&model, &target, 96, 64, 1., true, true)
                .unwrap();
            assert_eq!(renderer.uploads, stable_uploads);
            assert!(Arc::ptr_eq(
                renderer.geometry.as_ref().unwrap(),
                &model.vector_snapshot(1., true)
            ));

            let mut expected_uploads = stable_uploads;
            for (scale, background) in [(2., true), (2., false), (1., true)] {
                renderer
                    .draw(&model, &target, 96, 64, scale, background, true)
                    .unwrap();
                expected_uploads += 1;
                assert_eq!(renderer.uploads, expected_uploads);
            }
            model.load_source(crate::EXAMPLE).unwrap();
            renderer
                .draw(&model, &target, 96, 64, 1., true, true)
                .unwrap();
            expected_uploads += 1;
            assert_eq!(renderer.uploads, expected_uploads);
            model
                .load_component(crate::EXAMPLE, crate::BUTTON_COMPONENT)
                .unwrap();
            renderer
                .draw(&model, &target, 96, 64, 1., true, true)
                .unwrap();
            expected_uploads += 1;
            assert_eq!(renderer.uploads, expected_uploads);
            let held = Arc::clone(renderer.geometry.as_ref().unwrap());
            model = Button::new();
            renderer
                .draw(&model, &target, 96, 64, 1., true, true)
                .unwrap();
            expected_uploads += 1;
            assert_eq!(renderer.uploads, expected_uploads);
            assert!(!Arc::ptr_eq(&held, renderer.geometry.as_ref().unwrap()));

            let mut scrolling = Button::from_source(
                "component Demo { Frame { width:40; height:30; Scroll { Button { width:80; height:60; text:'О'; } } } }",
            ).unwrap();
            renderer
                .draw(&scrolling, &target, 96, 64, 1., true, true)
                .unwrap();
            let before_scroll = renderer.uploads;
            scrolling.scroll(5., 5.);
            renderer
                .draw(&scrolling, &target, 96, 64, 1., true, true)
                .unwrap();
            assert_eq!(renderer.uploads, before_scroll + 1);
            scrolling.scroll(0., 0.);
            renderer
                .draw(&scrolling, &target, 96, 64, 1., true, true)
                .unwrap();
            assert_eq!(renderer.uploads, before_scroll + 1);
            renderer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            println!(
                "GPU profiling smoke: {stats:?}; allocator={:?}",
                renderer.backend_allocator_stats()
            );
        });
    }
}
