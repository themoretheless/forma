//! Linear HDR exposure without tone mapping; independent of display transfer.

use crate::{GpuResourceLease, wgpu};
use memory_budget::{BufferError, MemoryBudget, PixelBuffer, Reservation};
use wgpu::util::DeviceExt;

const LINEAR_EXPOSURE_SHADER: &str = r"
struct Parameters { gain: f32, count: u32, padding: vec2<u32> }
@group(0) @binding(0) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output: array<vec4<f32>>;
@group(0) @binding(2) var<uniform> parameters: Parameters;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= parameters.count { return; }
    let pixel = source[id.x];
    output[id.x] = vec4<f32>(pixel.rgb * parameters.gain, pixel.a);
}
";

#[derive(Debug, thiserror::Error)]
pub enum ExposureError {
    #[error("GPU exposure cancelled")]
    Cancelled,
    #[error("{0}")]
    Invalid(&'static str),
    #[error(transparent)]
    Memory(#[from] BufferError),
    #[error("GPU exposure readback failed: {0}")]
    Readback(String),
}

/// Queue-ordered linear RGBA32F result; storage remains admitted while any
/// owner or in-flight submission retains it. No CPU readback is performed.
#[derive(Debug, Clone)]
pub struct ResidentExposure {
    buffer: wgpu::Buffer,
    pixels: u32,
    raster_alpha_valid: bool,
    alpha_associated: bool,
    _reservation: std::sync::Arc<Reservation>,
}

impl ResidentExposure {
    /// Whether the alpha channel was validated to lie in `0.0..=1.0`.
    pub fn raster_alpha_valid(&self) -> bool {
        self.raster_alpha_valid
    }

    /// Whether RGB channels are associated (premultiplied) with alpha.
    pub fn alpha_associated(&self) -> bool {
        self.alpha_associated
    }

    /// Exposure preserves RGB/alpha association; callers pass the input tag.
    pub fn with_alpha_associated(mut self, associated: bool) -> Self {
        self.alpha_associated = associated;
        self
    }

    /// Retain this owner or resource_lease through completion of downstream work.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn resource_lease(&self) -> GpuResourceLease {
        GpuResourceLease::new(std::iter::once(self._reservation.clone()))
    }

    pub fn pixels(&self) -> u32 {
        self.pixels
    }
}

#[derive(Debug)]
pub struct LinearExposureCompute {
    device: wgpu::Device,
    pipeline: wgpu::ComputePipeline,
}

impl LinearExposureCompute {
    /// Submit exposure and retain its output on GPU. Input is CPU-owned;
    /// GPU admission covers source, output and per-dispatch uniforms. Subsequent
    /// commands on the same queue can consume the result without a CPU wait.
    pub fn execute_resident<F: FnMut() -> bool>(
        &self,
        queue: &wgpu::Queue,
        samples: &[[f32; 4]],
        stops: f32,
        budget: &MemoryBudget,
        mut cancelled: F,
    ) -> Result<ResidentExposure, ExposureError> {
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        if !stops.is_finite() || !(-32.0..=32.0).contains(&stops) {
            return Err(ExposureError::Invalid(
                "exposure outside finite -32..32 range",
            ));
        }
        let count =
            u32::try_from(samples.len()).map_err(|_| ExposureError::Invalid("too many pixels"))?;
        if count == 0 {
            return Err(ExposureError::Invalid("resident exposure requires pixels"));
        }
        let gain = stops.exp2();
        let mut raster_alpha_valid = true;
        for (index, pixel) in samples.iter().enumerate() {
            raster_alpha_valid &= (0.0..=1.0).contains(&pixel[3]);
            if index % 4096 == 0 && cancelled() {
                return Err(ExposureError::Cancelled);
            }
            if pixel.iter().any(|v| !v.is_finite())
                || pixel[..3].iter().any(|v| !(v * gain).is_finite())
            {
                return Err(ExposureError::Invalid(
                    "exposure requires finite input and output",
                ));
            }
        }
        let bytes = u64::from(count) * 16;
        let limits = self.device.limits();
        if bytes > limits.max_buffer_size {
            return Err(ExposureError::Invalid("buffer exceeds device limit"));
        }
        let alignment = u64::from(limits.min_storage_buffer_offset_alignment).max(16) / 16;
        let maximum = (limits.max_storage_buffer_binding_size / 16)
            .min(u64::from(limits.max_compute_workgroups_per_dimension) * 64);
        let chunk = maximum / alignment * alignment;
        if chunk == 0 {
            return Err(ExposureError::Invalid("device cannot bind aligned chunk"));
        }
        let transient =
            std::sync::Arc::new(budget.try_reserve(bytes + u64::from(count).div_ceil(chunk) * 16)?);
        let resident = std::sync::Arc::new(budget.try_reserve(bytes)?);
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("resident exposure source"),
                contents: bytemuck::cast_slice(samples),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident exposure output"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.encode(&mut encoder, &source, &output, count, stops)
            .map_err(ExposureError::Invalid)?;
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        queue.submit([encoder.finish()]);
        let pending = resident.clone();
        queue.on_submitted_work_done(move || drop((transient, pending)));
        Ok(ResidentExposure {
            buffer: output,
            pixels: count,
            raster_alpha_valid,
            alpha_associated: false,
            _reservation: resident,
        })
    }

    /// Blocking, explicitly budgeted exposure/readback. The caller owns the
    /// input's CPU accounting. GPU accounting covers buffer sizes (including
    /// readback and dispatch uniforms), excluding driver/pipeline overhead.
    pub fn execute_managed(
        &self,
        queue: &wgpu::Queue,
        source_pixels: &[[f32; 4]],
        stops: f32,
        gpu_budget: &MemoryBudget,
        cpu_budget: &MemoryBudget,
    ) -> Result<PixelBuffer<[f32; 4]>, ExposureError> {
        self.execute_managed_with_cancel(
            queue,
            source_pixels,
            stops,
            gpu_budget,
            cpu_budget,
            || false,
        )
    }

    /// Cooperatively cancels CPU validation/admission and discards obsolete
    /// results. Submitted GPU work still completes before this blocking method
    /// returns; its reservations remain live through queue completion.
    pub fn execute_managed_with_cancel(
        &self,
        queue: &wgpu::Queue,
        source_pixels: &[[f32; 4]],
        stops: f32,
        gpu_budget: &MemoryBudget,
        cpu_budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<PixelBuffer<[f32; 4]>, ExposureError> {
        self.execute_into(
            queue,
            source_pixels,
            stops,
            gpu_budget,
            cpu_budget,
            [0.0; 4],
            1,
            &mut cancelled,
        )
    }

    /// Direct flat RGBA32F input/output for raster storage. No temporary
    /// array-of-pixels allocation or flattened result copy is required.
    pub fn execute_interleaved_with_cancel(
        &self,
        queue: &wgpu::Queue,
        samples: &[f32],
        stops: f32,
        gpu_budget: &MemoryBudget,
        cpu_budget: &MemoryBudget,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<PixelBuffer<f32>, ExposureError> {
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        let (pixels, remainder) = samples.as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(ExposureError::Invalid(
                "incomplete interleaved RGBA32F pixel",
            ));
        }
        self.execute_into(
            queue, pixels, stops, gpu_budget, cpu_budget, 0.0, 4, cancelled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_into<T: bytemuck::Pod>(
        &self,
        queue: &wgpu::Queue,
        source_pixels: &[[f32; 4]],
        stops: f32,
        gpu_budget: &MemoryBudget,
        cpu_budget: &MemoryBudget,
        initial: T,
        components: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<PixelBuffer<T>, ExposureError> {
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        if !stops.is_finite() || !(-32.0..=32.0).contains(&stops) {
            return Err(ExposureError::Invalid(
                "exposure outside finite -32..32 range",
            ));
        }
        let count = u32::try_from(source_pixels.len())
            .map_err(|_| ExposureError::Invalid("too many RGBA32F pixels"))?;
        let gain = stops.exp2();
        for (index, pixel) in source_pixels.iter().enumerate() {
            if index % 4096 == 0 && cancelled() {
                return Err(ExposureError::Cancelled);
            }
            if pixel.iter().any(|value| !value.is_finite()) {
                return Err(ExposureError::Invalid("RGBA32F samples must be finite"));
            }
            if pixel[..3].iter().any(|value| !(value * gain).is_finite()) {
                return Err(ExposureError::Invalid(
                    "exposed RGB exceeds finite f32 range",
                ));
            }
        }
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        if count == 0 {
            return Ok(cpu_budget.try_buffer(0, initial)?.freeze().into());
        }
        let bytes = u64::from(count) * 16;
        let limits = self.device.limits();
        if bytes > limits.max_buffer_size {
            return Err(ExposureError::Invalid(
                "RGBA32F buffer exceeds device limit",
            ));
        }
        let alignment = u64::from(limits.min_storage_buffer_offset_alignment).max(16) / 16;
        let maximum = (limits.max_storage_buffer_binding_size / 16)
            .min(u64::from(limits.max_compute_workgroups_per_dimension) * 64);
        let chunk = maximum / alignment * alignment;
        if chunk == 0 {
            return Err(ExposureError::Invalid(
                "device cannot bind an aligned RGBA32F chunk",
            ));
        }
        let reserved_bytes = bytes * 3 + u64::from(count).div_ceil(chunk) * 16;
        let cpu_reservation = cpu_budget.try_reserve(bytes)?;
        let reservation = std::sync::Arc::new(gpu_budget.try_reserve(reserved_bytes)?);
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        let output_len = source_pixels
            .len()
            .checked_mul(components)
            .ok_or(ExposureError::Invalid("RGBA32F output length overflow"))?;
        let mut result = cpu_reservation.try_buffer(output_len, initial)?;
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("managed exposure source"),
                contents: bytemuck::cast_slice(source_pixels),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("managed exposure output"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("managed exposure readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.encode(&mut encoder, &source, &output, count, stops)
            .map_err(ExposureError::Invalid)?;
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        let submission = queue.submit([encoder.finish()]);
        // Even a failed poll must not release accounting for in-flight work.
        let in_flight = reservation.clone();
        queue.on_submitted_work_done(move || drop(in_flight));
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |status| {
                let _ = tx.send(status);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| ExposureError::Readback(error.to_string()))?;
        rx.recv()
            .map_err(|error| ExposureError::Readback(error.to_string()))?
            .map_err(|error| ExposureError::Readback(error.to_string()))?;
        // Bound obsolete CPU work to 4096 pixels even for full-resolution
        // readback. Always release the mapping before returning cancellation.
        let copied = {
            let mapped = readback
                .slice(..)
                .get_mapped_range()
                .map_err(|error| ExposureError::Readback(error.to_string()))?;
            (|| {
                for (destination, source) in result
                    .chunks_mut(4096 * components)
                    .zip(bytemuck::cast_slice::<u8, T>(&mapped).chunks(4096 * components))
                {
                    if cancelled() {
                        return Err(ExposureError::Cancelled);
                    }
                    destination.copy_from_slice(source);
                }
                Ok(())
            })()
        };
        readback.unmap();
        copied?;
        if cancelled() {
            return Err(ExposureError::Cancelled);
        }
        Ok(result.freeze().into())
    }

    pub fn new(device: &wgpu::Device) -> Self {
        let pipeline = crate::pipeline::compute_pipeline(
            device,
            "linear HDR exposure",
            LINEAR_EXPOSURE_SHADER,
            "main",
        );
        Self {
            device: device.clone(),
            pipeline,
        }
    }

    /// Encodes exposure over separate RGBA32F STORAGE buffers. Alpha is unchanged.
    /// Uses aligned subrange bindings and bounded dispatches for large buffers.
    /// This does not submit work or perform HDR display/tone mapping.
    /// The caller is responsible for finite samples and RGB overflow admission;
    /// `execute_managed` validates CPU input before allocating or submitting.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        output: &wgpu::Buffer,
        count: u32,
        stops: f32,
    ) -> Result<(), &'static str> {
        if !stops.is_finite() || !(-32.0..=32.0).contains(&stops) {
            return Err("exposure outside finite -32..32 range");
        }
        if count == 0 {
            return Ok(());
        }
        let bytes = u64::from(count) * 16;
        let limits = self.device.limits();
        if source.size() < bytes || output.size() < bytes {
            return Err("RGBA32F buffer is too short");
        }
        // Binding offsets must satisfy both device alignment and RGBA32F stride.
        let alignment_pixels = u64::from(limits.min_storage_buffer_offset_alignment).max(16) / 16;
        let max_pixels = (limits.max_storage_buffer_binding_size / 16)
            .min(u64::from(limits.max_compute_workgroups_per_dimension) * 64);
        let chunk_pixels = max_pixels / alignment_pixels * alignment_pixels;
        if chunk_pixels == 0 {
            return Err("device cannot bind an aligned RGBA32F chunk");
        }
        if source == output {
            return Err("exposure requires separate buffers");
        }
        if !source.usage().contains(wgpu::BufferUsages::STORAGE)
            || !output.usage().contains(wgpu::BufferUsages::STORAGE)
        {
            return Err("buffers require STORAGE usage");
        }
        let mut offset_pixels = 0_u64;
        while offset_pixels < u64::from(count) {
            let current = (u64::from(count) - offset_pixels).min(chunk_pixels) as u32;
            let words = [stops.exp2().to_bits(), current, 0, 0];
            let parameters = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("linear exposure parameters"),
                    contents: bytemuck::cast_slice(&words),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("linear exposure bindings"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: source,
                            offset: offset_pixels * 16,
                            size: std::num::NonZeroU64::new(u64::from(current) * 16),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: output,
                            offset: offset_pixels * 16,
                            size: std::num::NonZeroU64::new(u64::from(current) * 16),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: parameters.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(current.div_ceil(64), 1, 1);
            drop(pass);
            offset_pixels += u64::from(current);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::validate_wgsl;

    #[test]
    fn shader_parses_and_validates_with_naga() {
        validate_wgsl(
            LINEAR_EXPOSURE_SHADER,
            wgpu::naga::valid::Capabilities::empty(),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn exposure_readback_cancellation_releases_mapping_and_allows_retry() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let compute = LinearExposureCompute::new(&device);
        let input = vec![[4.0, -2.0, 0.125, 0.375]; 8200];
        let bytes = input.len() as u64 * 16;
        let cpu = MemoryBudget::new(bytes);
        let gpu = MemoryBudget::new(bytes * 3 + 16);
        for flat in [false, true] {
            for checkpoint in 1..=6 {
                let mut admitted_polls = 0;
                let mut cancel = || {
                    if gpu.used() > 0 {
                        admitted_polls += 1;
                    }
                    admitted_polls == checkpoint
                };
                let cancelled = if flat {
                    matches!(
                        compute.execute_interleaved_with_cancel(
                            &queue,
                            bytemuck::cast_slice(&input),
                            1.0,
                            &gpu,
                            &cpu,
                            &mut cancel,
                        ),
                        Err(ExposureError::Cancelled)
                    )
                } else {
                    matches!(
                        compute.execute_managed_with_cancel(
                            &queue,
                            &input,
                            1.0,
                            &gpu,
                            &cpu,
                            &mut cancel,
                        ),
                        Err(ExposureError::Cancelled)
                    )
                };
                assert!(cancelled);
                assert_eq!(admitted_polls, checkpoint);
                assert_eq!(cpu.used(), 0);
                assert_eq!(gpu.used(), 0);
                let retry = compute
                    .execute_managed(&queue, &input, 1.0, &gpu, &cpu)
                    .unwrap();
                assert!(retry.iter().all(|pixel| *pixel == [8.0, -4.0, 0.25, 0.375]));
                drop(retry);
                assert_eq!(cpu.used(), 0);
                assert_eq!(gpu.used(), 0);
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn linear_exposure_compute_preserves_hdr_alpha_and_dispatch_tail() {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits {
                max_storage_buffer_binding_size: 1024,
                ..wgpu::Limits::default()
            },
            ..Default::default()
        }))
        .unwrap();
        let source_pixels: Vec<[f32; 4]> = (0..129)
            .map(|i| [128.0 + i as f32, -2.0, 0.125, 0.25])
            .collect();
        let initial = vec![[9.0_f32; 4]; 130];
        let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&source_pixels),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&initial),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: output.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let compute = LinearExposureCompute::new(&device);
        let finite_cpu = MemoryBudget::new(16);
        let finite_gpu = MemoryBudget::new(64);
        for pixel in [
            [f32::MAX, 0.0, 0.0, 1.0],
            [-f32::MAX, 0.0, 0.0, 1.0],
            [f32::INFINITY, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, f32::NAN],
        ] {
            assert!(matches!(
                compute.execute_managed(&queue, &[pixel], 1.0, &finite_gpu, &finite_cpu),
                Err(ExposureError::Invalid(_))
            ));
            assert_eq!(finite_cpu.peak(), 0);
            assert_eq!(finite_gpu.peak(), 0);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        assert!(
            compute
                .encode(&mut encoder, &source, &output, 129, f32::NAN)
                .is_err()
        );
        assert!(
            compute
                .encode(&mut encoder, &source, &source.clone(), 129, 1.0)
                .is_err()
        );
        assert!(
            compute
                .encode(&mut encoder, &source, &output, 130, 1.0)
                .is_err()
        );
        compute
            .encode(&mut encoder, &source, &output, 129, 1.0)
            .unwrap();
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        {
            let mapped = readback.slice(..).get_mapped_range().unwrap();
            let pixels: &[[f32; 4]] = bytemuck::cast_slice(&mapped);
            for (i, pixel) in pixels[..129].iter().enumerate() {
                assert_eq!(pixel, &[256.0 + 2.0 * i as f32, -4.0, 0.25, 0.25]);
            }
            assert_eq!(pixels[129], [9.0; 4]);
        }
        readback.unmap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn resident_exposure_preserves_hdr_tail_and_budgeted_gpu_ownership() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let compute = LinearExposureCompute::new(&device);
        let input = vec![[4., -2., 0.125, 0.375]; 8200];
        let bytes = input.len() as u64 * 16;
        let root = MemoryBudget::new(bytes * 2 + 16);
        for checkpoint in 1..=6 {
            let mut polls = 0;
            assert!(matches!(
                compute.execute_resident(&queue, &input, 1., &root, || {
                    polls += 1;
                    polls == checkpoint
                }),
                Err(ExposureError::Cancelled)
            ));
            assert_eq!(root.used(), 0);
        }
        let refused = MemoryBudget::new(bytes * 2 + 15);
        assert!(
            compute
                .execute_resident(&queue, &input, 1., &refused, || false)
                .is_err()
        );
        assert_eq!(refused.used(), 0);
        let output = compute
            .execute_resident(&queue, &input, 1., &root, || false)
            .unwrap();
        assert_eq!(output.pixels(), 8200);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(root.used(), bytes);
        let held = output.clone();
        drop(output);
        assert_eq!(root.used(), bytes);
        let downstream = held.resource_lease();
        drop(held);
        assert_eq!(root.used(), bytes);
        drop(downstream);
        assert_eq!(root.used(), 0);
        let abandoned = compute
            .execute_resident(&queue, &input, 1., &root, || false)
            .unwrap();
        drop(abandoned);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(root.used(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn exposure_signed_f32_boundary_values_remain_exact_and_finite() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let compute = LinearExposureCompute::new(&device);
        let cpu = MemoryBudget::new(16);
        let gpu = MemoryBudget::new(64);
        for (stops, magnitude) in [
            (0f32, f32::MAX),
            (1., f32::MAX / 2.),
            (32., f32::MAX / 4294967296.),
            (-32., f32::MAX),
        ] {
            let pixel = [magnitude, -magnitude, -0., -f32::MAX];
            let gain = stops.exp2();
            let expected = [pixel[0] * gain, pixel[1] * gain, pixel[2] * gain, pixel[3]];
            let result = compute
                .execute_managed(&queue, &[pixel], stops, &gpu, &cpu)
                .unwrap();
            assert!(result[0].iter().all(|v| v.is_finite()));
            assert_eq!(result[0].map(f32::to_bits), expected.map(f32::to_bits));
            drop(result);
            assert_eq!((cpu.used(), gpu.used()), (0, 0));
        }
    }
}
