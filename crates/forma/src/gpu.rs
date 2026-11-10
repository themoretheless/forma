//! Native WebGPU/wgpu backend. Uploads vector commands/edges, never RGBA images.
use crate::display_list::{DisplayList, PARAMS_LEN, RenderScene, TILE, TileScratch};
use std::sync::Arc;
pub const SHADER: &str = include_str!("vector.wgsl");

/// Adapter memory topology classification:
/// - [`MemoryArchitecture::Uma`] (Unified Memory Architecture): CPU and GPU share
///   one physical memory pool (e.g., Apple Silicon / Metal on macOS/iOS, integrated GPUs).
///   Process RSS and GPU buffer/target allocations reside in the same system RAM and are
///   **not** additive.
/// - [`MemoryArchitecture::Duma`] (Discrete / Non-Unified Memory Architecture): GPU owns
///   dedicated device memory (VRAM) separate from host RAM, or runs as a distinct device
///   pool (e.g., discrete PCIe GPUs). Host RSS (`cpu_*` bytes) and device allocations
///   (`buffer_bytes` + render targets) live in separate pools and transfers cross a bus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryArchitecture {
    Uma,
    #[default]
    Duma,
}

impl MemoryArchitecture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uma => "UMA",
            Self::Duma => "DUMA",
        }
    }

    pub const fn is_unified(self) -> bool {
        matches!(self, Self::Uma)
    }

    /// Classify a `wgpu::AdapterInfo` into `UMA` or `DUMA`.
    pub fn from_adapter_info(info: &wgpu::AdapterInfo) -> Self {
        match info.device_type {
            wgpu::DeviceType::IntegratedGpu => Self::Uma,
            wgpu::DeviceType::DiscreteGpu => Self::Duma,
            wgpu::DeviceType::VirtualGpu | wgpu::DeviceType::Cpu => Self::Uma,
            wgpu::DeviceType::Other => {
                if matches!(info.backend, wgpu::Backend::Metal) {
                    Self::Uma
                } else {
                    Self::Duma
                }
            }
        }
    }
}

/// Live resources owned by this renderer, not process VRAM usage or GPU residency.
/// Buffer sizes include wgpu's creation alignment. Driver heaps, in-flight staging,
/// pipeline memory, and the caller-owned render target/swapchain are not included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuResourceStats {
    pub memory_architecture: MemoryArchitecture,
    pub buffer_count: usize,
    pub buffer_bytes: u64,
    pub uniform_bytes: u64,
    pub command_bytes: u64,
    pub edge_bytes: u64,
    pub tile_bytes: u64,
    pub paint_bytes: u64,
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
    /// Retained current/last-uploaded uniform and paint storage, not GPU memory.
    pub cpu_paint_scratch_bytes: usize,
    /// Unified memory pool bytes when `memory_architecture == Uma` (`buffer_bytes` +
    /// CPU-side renderer scratch/shared geometry in the same physical RAM), or `0` on `Duma`.
    pub uma_shared_pool_bytes: u64,
    /// Dedicated device-local GPU buffer bytes when `memory_architecture == Duma`, or `0` on `Uma`.
    pub duma_device_local_bytes: u64,
    /// Host-side RAM retained by the renderer + shared geometry when `memory_architecture == Duma`,
    /// or `0` on `Uma` (where it is accounted in `uma_shared_pool_bytes`).
    pub duma_host_ram_bytes: u64,
}

/// Optional backend suballocator accounting. Not total adapter memory or residency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendAllocatorStats {
    pub allocated_bytes: u64,
    pub reserved_bytes: u64,
    pub allocation_count: usize,
    pub block_count: usize,
}

// One contiguous write bounds the changed words. No temporary diff/range list,
// and byte offsets remain COPY_BUFFER_ALIGNMENT-aligned (f32 is four bytes).
fn changed_paint_range(previous: &[f32], next: &[f32]) -> Option<std::ops::Range<usize>> {
    if previous.len() != next.len() {
        return Some(0..next.len());
    }
    let first = previous
        .iter()
        .zip(next)
        .position(|(a, b)| a.to_bits() != b.to_bits())?;
    let last = previous
        .iter()
        .zip(next)
        .rposition(|(a, b)| a.to_bits() != b.to_bits())
        .unwrap();
    Some(first..last + 1)
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

// Preserve the first actionable failure; teardown/device-loss callbacks must
// not overwrite the original validation diagnostic.
fn record_gpu_fault(fault: &std::sync::Mutex<Option<String>>, error: String) {
    let mut stored = fault
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if stored.is_none() {
        *stored = Some(error);
    }
}
fn check_gpu_fault(fault: &std::sync::Mutex<Option<String>>) -> Result<(), String> {
    let stored = fault
        .lock()
        .map_err(|_| "GPU fault state poisoned".to_owned())?;
    match stored.as_ref() {
        Some(error) => Err(error.clone()),
        None => Ok(()),
    }
}

#[cfg(any(feature = "native", test))]
pub(crate) fn device_queue_idle(device: &wgpu::Device) -> bool {
    #[cfg(not(target_os = "linux"))]
    {
        device.is_queue_idle()
    }
    #[cfg(target_os = "linux")]
    {
        matches!(
            device.poll(wgpu::PollType::Poll),
            Ok(wgpu::PollStatus::QueueEmpty)
        )
    }
}

#[cfg(test)]
fn missing_compute_pipeline_error(message: &str) -> bool {
    message.contains("without an active ComputePipeline")
        || message.contains("Compute pipeline must be set")
}

#[cfg(test)]
fn gpu_test_adapter_supported(info: &wgpu::AdapterInfo) -> bool {
    if info.device_type != wgpu::DeviceType::Cpu {
        return true;
    }
    if info.backend != wgpu::Backend::Vulkan {
        return false;
    }
    // Mesa's lavapipe Vulkan driver commonly identifies the device as llvmpipe.
    [&info.name, &info.driver, &info.driver_info]
        .iter()
        .any(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("lavapipe") || value.contains("llvmpipe")
        })
}

#[cfg(test)]
pub(crate) fn assert_gpu_test_adapter(info: &wgpu::AdapterInfo) {
    assert!(
        gpu_test_adapter_supported(info),
        "hardware adapter or Mesa lavapipe required, got {info:?}"
    );
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub memory_architecture: MemoryArchitecture,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: Option<wgpu::BindGroup>,
    geometry: Option<Arc<DisplayList>>,
    tile_key: Option<(u32, u32, f32, [f32; 2])>,
    tile_scratch: TileScratch,
    buffers: Vec<wgpu::Buffer>,
    params: Vec<f32>,
    uploaded_params: Vec<f32>,
    paints: Vec<f32>,
    uploaded_paints: Vec<f32>,
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
/// Physical render target dimensions and DPI scale for a single [`Renderer::draw`] pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl Viewport {
    #[inline]
    pub const fn new(width: u32, height: u32, scale: f32) -> Self {
        Self {
            width,
            height,
            scale,
        }
    }
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
        let memory_architecture = MemoryArchitecture::from_adapter_info(&adapter.get_info());
        let timestamps = profile && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: if timestamps {
                    wgpu::Features::TIMESTAMP_QUERY
                } else {
                    wgpu::Features::empty()
                },
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let fault = std::sync::Arc::new(std::sync::Mutex::new(None));
        let errors = fault.clone();
        device.on_uncaptured_error(std::sync::Arc::new(move |error| {
            record_gpu_fault(&errors, format!("GPU validation/device error: {error}"));
        }));
        let lost = fault.clone();
        device.set_device_lost_callback(move |reason, message| {
            record_gpu_fault(&lost, format!("GPU device lost: {reason:?}: {message}"));
        });
        // Late registration can immediately deliver an already-lost backend.
        // Refuse setup before creating more device resources in that case.
        check_gpu_fault(&fault)?;
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
            label: Some("Forma colors, viewport and scroll"),
            size: (PARAMS_LEN * std::mem::size_of::<f32>()) as u64,
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
            memory_architecture,
            pipeline,
            uniform,
            bindings: None,
            geometry: None,
            tile_key: None,
            tile_scratch: TileScratch::default(),
            buffers: Vec::with_capacity(4),
            params: Vec::with_capacity(PARAMS_LEN),
            uploaded_params: Vec::with_capacity(PARAMS_LEN),
            paints: Vec::new(),
            uploaded_paints: Vec::new(),
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
        let paint_bytes = bytes(3);
        let timing_buffer_bytes = self
            .timing
            .as_ref()
            .map_or(0, |t| t.resolve.size() + t.readback.size());
        let buffer_bytes = uniform_bytes
            + command_bytes
            + edge_bytes
            + tile_bytes
            + paint_bytes
            + timing_buffer_bytes;
        let cpu_shared_geometry_bytes = self.geometry.as_ref().map_or(0, |list| {
            (list.commands.capacity() + list.edges.capacity()) * std::mem::size_of::<f32>()
        });
        let cpu_tile_scratch_bytes = self.tile_scratch.capacity_bytes();
        let cpu_paint_scratch_bytes = (self.params.capacity()
            + self.uploaded_params.capacity()
            + self.paints.capacity()
            + self.uploaded_paints.capacity())
            * std::mem::size_of::<f32>();
        let host_bytes =
            (cpu_shared_geometry_bytes + cpu_tile_scratch_bytes + cpu_paint_scratch_bytes) as u64;
        let (uma_shared_pool_bytes, duma_device_local_bytes, duma_host_ram_bytes) =
            match self.memory_architecture {
                MemoryArchitecture::Uma => (buffer_bytes + host_bytes, 0, 0),
                MemoryArchitecture::Duma => (0, buffer_bytes, host_bytes),
            };
        GpuResourceStats {
            memory_architecture: self.memory_architecture,
            buffer_count: 1 + self.buffers.len() + if self.timing.is_some() { 2 } else { 0 },
            buffer_bytes,
            uniform_bytes,
            command_bytes,
            edge_bytes,
            tile_bytes,
            paint_bytes,
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
            cpu_shared_geometry_bytes,
            cpu_tile_scratch_bytes,
            cpu_paint_scratch_bytes,
            uma_shared_pool_bytes,
            duma_device_local_bytes,
            duma_host_ram_bytes,
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
        #[cfg(not(target_os = "linux"))]
        let (begin, end) = {
            let values = timing.readback.uma_slice();
            (
                u64::from_ne_bytes(values[..8].try_into().unwrap()),
                u64::from_ne_bytes(values[8..16].try_into().unwrap()),
            )
        };
        #[cfg(target_os = "linux")]
        let (begin, end) = {
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
            let pair = (
                u64::from_ne_bytes(values[..8].try_into().unwrap()),
                u64::from_ne_bytes(values[8..16].try_into().unwrap()),
            );
            drop(values);
            timing.readback.unmap();
            pair
        };
        timing.pending = false;
        Ok(timestamp_duration_ns(begin, end, timing.period_ns))
    }
    /// Report a latched renderer fault before configuring/acquiring a surface.
    /// Hosts should leave the GPU path when this fails, even if acquisition
    /// would otherwise only produce transient timeouts.
    pub fn check_health(&self) -> Result<(), String> {
        self.device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| format!("GPU health poll: {e:?}"))?;
        check_gpu_fault(&self.fault)
    }

    #[cfg(feature = "native-test-hooks")]
    pub(crate) fn inject_device_loss_for_testing(&self, message: String) {
        #[cfg(not(target_os = "linux"))]
        self.device.inject_backend_device_loss_for_testing(message);
        #[cfg(target_os = "linux")]
        record_gpu_fault(&self.fault, message);
    }

    pub fn draw(
        &mut self,
        model: &impl RenderScene,
        target: &wgpu::TextureView,
        viewport: Viewport,
        background: bool,
        opaque: bool,
    ) -> Result<(), String> {
        let Viewport {
            width,
            height,
            scale,
        } = viewport;
        self.check_health()?;
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
        // The binner depends on the number of tiles and the scroll offset, not
        // exact viewport pixels. Sub-tile resize changes only uniforms; scrolling
        // re-bins the retained geometry without any command/edge upload.
        let scroll = model.gpu_scroll();
        let tile_key = (width.div_ceil(TILE), height.div_ceil(TILE), scale, scroll);
        let tiles_changed = changed || self.tile_key != Some(tile_key);
        model.gpu_paints_into(&mut self.paints);
        let paint_range = changed_paint_range(&self.uploaded_paints, &self.paints);
        let paints_changed = self.buffers.len() < 4 || paint_range.is_some();
        {
            let tiles = if tiles_changed {
                list.tiles_into(width, height, scale, scroll, &mut self.tile_scratch)
            } else {
                &[]
            };
            let edges = if list.edges.is_empty() {
                &[0f32; 4][..]
            } else {
                &list.edges
            };
            let data: [&[u8]; 4] = [
                bytemuck::cast_slice(&list.commands),
                bytemuck::cast_slice(edges),
                bytemuck::cast_slice(tiles),
                bytemuck::cast_slice(&self.paints),
            ];
            let limits = self.device.limits();
            let storage_limit = limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size);
            if data.iter().any(|d| d.len() as u64 > storage_limit) {
                return Err("Vector scene exceeds GPU storage budget".into());
            }
            let mut rebind = self.bindings.is_none();
            for (index, payload) in data.iter().enumerate() {
                if (index < 2 && !changed)
                    || (index == 2 && !tiles_changed)
                    || (index == 3 && !paints_changed)
                {
                    continue;
                }
                let current = self.buffers.get(index).map_or(0, wgpu::Buffer::size);
                let capacity = storage_capacity(payload.len() as u64, current, storage_limit)?;
                if capacity != current {
                    let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(
                            [
                                "Forma commands",
                                "Forma edges",
                                "Forma tiles",
                                "Forma paints",
                            ][index],
                        ),
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
                let (offset, bytes) = if index == 3 && capacity == current {
                    let range = paint_range
                        .as_ref()
                        .expect("changed paints have a write range");
                    (range.start * 4, &payload[range.start * 4..range.end * 4])
                } else {
                    (0, *payload)
                };
                self.queue
                    .write_buffer(&self.buffers[index], offset as u64, bytes);
                self.uploaded_bytes += bytes.len() as u64;
            }
            if changed {
                self.uploads += 1;
            }
            if tiles_changed {
                self.tile_uploads += 1;
            }
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
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.buffers[3].as_entire_binding(),
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
        if paints_changed {
            self.uploaded_paints.clone_from(&self.paints);
        }
        model.gpu_params_into(width, height, scale, opaque, &mut self.params);
        if opaque && self.format.is_srgb() {
            self.params[3] = 2.;
        }
        if self.params != self.uploaded_params {
            self.queue
                .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&self.params));
            self.uploaded_bytes += std::mem::size_of_val(self.params.as_slice()) as u64;
            self.uploaded_params.clone_from(&self.params);
        }
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
        model.release_cpu_cache();
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod paint_range_tests;
