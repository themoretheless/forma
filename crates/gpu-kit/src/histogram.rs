//! Bounded, scene-linear RGB/luminance histogram on CPU and GPU.
//!
//! Log bins retain HDR excursions from -16 through +16 EV relative to 18% grey.
//! On GPU, one active job plus one replaceable pending job reads back only 1100 bytes.

use crate::{GpuResourceLease, wgpu};
use memory_budget::{MemoryBudget, Reservation};
use std::sync::{Arc, mpsc};
use wgpu::util::DeviceExt;

pub const BINS: usize = 64;
pub const MAX_SAMPLES: usize = 65_536;
const WORDS: usize = 275;
pub const HISTOGRAM_JOB_BYTES: u64 = 32 + 2 * WORDS as u64 * 4;

const HISTOGRAM_SHADER: &str = r"
struct Parameters { sensor_width:u32, x:u32, y:u32, width:u32, height:u32, stride:u32, samples:u32, padding:u32 };
@group(0) @binding(0) var<storage,read> source_pixels:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> counters:array<atomic<u32>>;
@group(0) @binding(2) var<uniform> p:Parameters;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=p.samples {return;}
    let local=id.x*p.stride;
    let pixel=source_pixels[(p.y+local/p.width)*p.sensor_width+p.x+local%p.width];
    atomicAdd(&counters[272],1u);
    if !all(abs(pixel)<=vec4<f32>(3.402823466e38)) || pixel.a<0.0 || pixel.a>1.0 {atomicAdd(&counters[273],1u);return;}
    if pixel.a==0.0 {atomicAdd(&counters[274],1u);return;}
    let values=vec4<f32>(pixel.rgb,dot(pixel.rgb,vec3<f32>(0.2126,0.7152,0.0722)));
    for(var c=0u;c<4u;c+=1u) {
        let value=values[c];
        if value<0.0 {atomicAdd(&counters[256u+c],1u);}
        else if value==0.0 {atomicAdd(&counters[260u+c],1u);}
        else {
            // Difference of logarithms avoids overflow/underflow in value/0.18.
            let position=(log2(value)-log2(0.18)+16.0)*2.0;
            if position<0.0 {atomicAdd(&counters[264u+c],1u);}
            else if position>=64.0 {atomicAdd(&counters[268u+c],1u);}
            else {atomicAdd(&counters[c*64u+u32(floor(position))],1u);}
        }
    }
}
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    /// Half-stop bins from -16 through +16 EV relative to 18% grey, R/G/B/Y.
    pub channels: [[u64; BINS]; 4],
    pub negative: [u64; 4],
    pub zero: [u64; 4],
    pub underflow: [u64; 4],
    pub overflow: [u64; 4],
    pub sampled: u64,
    pub invalid: u64,
    pub transparent: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            channels: [[0; BINS]; 4],
            negative: [0; 4],
            zero: [0; 4],
            underflow: [0; 4],
            overflow: [0; 4],
            sampled: 0,
            invalid: 0,
            transparent: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HistogramError {
    #[error("histogram cancelled")]
    Cancelled,
    #[error("invalid histogram layout, sample limit or exposure")]
    Invalid,
}

impl Histogram {
    /// Uniformly subsamples at most `max_samples` pixels. Nonfinite pixels or
    /// alpha outside [0,1] are counted as invalid; fully transparent RGB is ignored.
    /// Signed samples are recorded separately; exposure is applied in log space.
    // BINS is 64 and the float index is checked in [0,64) before conversion.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn linear_rgba(
        pixels: &[f32],
        max_samples: usize,
        exposure: f32,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Self, HistogramError> {
        if cancelled() {
            return Err(HistogramError::Cancelled);
        }
        if !pixels.len().is_multiple_of(4)
            || !(1..=MAX_SAMPLES).contains(&max_samples)
            || !exposure.is_finite()
            || !(-32. ..=32.).contains(&exposure)
        {
            return Err(HistogramError::Invalid);
        }
        let count = pixels.len() / 4;
        let stride = count.div_ceil(max_samples).max(1);
        let mut result = Self::default();
        for (i, p) in pixels.as_chunks::<4>().0.iter().step_by(stride).enumerate() {
            if i % 256 == 0 && cancelled() {
                return Err(HistogramError::Cancelled);
            }
            result.sampled += 1;
            if p.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&p[3]) {
                result.invalid += 1;
                continue;
            }
            if p[3] == 0. {
                result.transparent += 1;
                continue;
            }
            // f64 avoids overflow while forming luminance from finite f32 HDR.
            let values = [
                f64::from(p[0]),
                f64::from(p[1]),
                f64::from(p[2]),
                0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]),
            ];
            for (channel, value) in values.into_iter().enumerate() {
                if value < 0. {
                    result.negative[channel] += 1;
                } else if value == 0. {
                    result.zero[channel] += 1;
                } else {
                    let position = ((value / 0.18).log2() + f64::from(exposure) + 16.) * 2.;
                    if position < 0. {
                        result.underflow[channel] += 1;
                    } else if position >= BINS as f64 {
                        result.overflow[channel] += 1;
                    } else {
                        result.channels[channel][position.floor() as usize] += 1;
                    }
                }
            }
        }
        if cancelled() {
            return Err(HistogramError::Cancelled);
        }
        Ok(result)
    }
}

/// Active sampling crop within a 2D pixel buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistogramRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

type Cancel = Arc<dyn Fn() -> bool + Send + Sync>;

struct Input {
    buffer: wgpu::Buffer,
    dimensions: [u32; 2],
    crop: HistogramRect,
    resources: GpuResourceLease,
    cancelled: Cancel,
}

struct Done {
    result: Result<(), String>,
    cancelled: Cancel,
}

struct Readback {
    buffer: wgpu::Buffer,
    _reservation: Arc<Reservation>,
}

pub struct GpuHistogram {
    revision: Arc<std::sync::atomic::AtomicU64>,
    readback: Option<Readback>,
    device: wgpu::Device,
    pipeline: wgpu::ComputePipeline,
    budget: MemoryBudget,
    sender: mpsc::SyncSender<Done>,
    receiver: mpsc::Receiver<Done>,
    active: bool,
    pending: Option<Input>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for GpuHistogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuHistogram")
            .field("active", &self.active)
            .field("pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

impl GpuHistogram {
    pub fn new(
        device: &wgpu::Device,
        budget: MemoryBudget,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let pipeline = crate::pipeline::compute_pipeline(
            device,
            "Bounded histogram",
            HISTOGRAM_SHADER,
            "main",
        );
        let (sender, receiver) = mpsc::sync_channel(1);
        Self {
            revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            readback: None,
            device: device.clone(),
            pipeline,
            budget,
            sender,
            receiver,
            active: false,
            pending: None,
            wake,
        }
    }

    /// Submit a linear RGBA32F GPU buffer for histogram sampling inside `crop`.
    pub fn submit_buffer(
        &mut self,
        queue: &wgpu::Queue,
        buffer: wgpu::Buffer,
        dimensions: [u32; 2],
        crop: HistogramRect,
        resources: GpuResourceLease,
        cancelled: Cancel,
    ) -> Result<(), String> {
        let job = Input {
            buffer,
            dimensions,
            crop,
            resources,
            cancelled,
        };
        self.submit(queue, job)
    }

    fn submit(&mut self, queue: &wgpu::Queue, mut input: Input) -> Result<(), String> {
        if (input.cancelled)() {
            return Ok(());
        }
        let id = self
            .revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;
        let revision = self.revision.clone();
        let cancelled = input.cancelled;
        input.cancelled = Arc::new(move || {
            cancelled() || revision.load(std::sync::atomic::Ordering::Acquire) != id
        });
        if self.active {
            self.pending = Some(input);
            Ok(())
        } else {
            self.start(queue, input)
        }
    }

    fn start(&mut self, queue: &wgpu::Queue, input: Input) -> Result<(), String> {
        if input.buffer.size() > self.device.limits().max_storage_buffer_binding_size {
            return Err("histogram source exceeds this adapter's storage binding limit".into());
        }
        let crop = input.crop;
        let [width, height] = input.dimensions;
        let valid = crop
            .x
            .checked_add(crop.width)
            .is_some_and(|end| end <= width)
            && crop
                .y
                .checked_add(crop.height)
                .is_some_and(|end| end <= height)
            && u64::from(width)
                .checked_mul(u64::from(height))
                .and_then(|pixels| pixels.checked_mul(16))
                .is_some_and(|bytes| bytes <= input.buffer.size());
        if !valid {
            return Err("histogram crop/source extent is invalid".into());
        }
        let count = crop
            .width
            .checked_mul(crop.height)
            .ok_or("histogram dimensions overflow")?;
        if count == 0 {
            return Err("empty histogram crop".into());
        }
        let stride = count.div_ceil(MAX_SAMPLES as u32);
        let samples = count.div_ceil(stride);
        let reservation = self
            .budget
            .try_reserve(HISTOGRAM_JOB_BYTES)
            .map_err(|e| e.to_string())?;
        let output = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("histogram counters"),
                contents: &[0; WORDS * 4],
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("histogram readback"),
            size: WORDS as u64 * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params = [
            input.dimensions[0],
            crop.x,
            crop.y,
            crop.width,
            crop.height,
            stride,
            samples,
            0,
        ];
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("histogram dimensions"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(samples.div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &read, 0, WORDS as u64 * 4);
        queue.submit([encoder.finish()]);
        let reservation = Arc::new(reservation);
        self.readback = Some(Readback {
            buffer: read.clone(),
            _reservation: reservation.clone(),
        });
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        self.active = true;
        // Capture credits, never a Buffer/Device: a callback owning the mapped
        // buffer would keep its own device/context alive after service teardown.
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |status| {
                drop(input.resources);
                drop(reservation);
                let _ = sender.try_send(Done {
                    result: status.map_err(|e| e.to_string()),
                    cancelled: input.cancelled,
                });
                wake();
            });
        Ok(())
    }

    /// Call after device polling. Obsolete generations never reach the UI.
    ///
    /// # Panics
    /// Panics if an internal completion arrives without its owned readback.
    pub fn poll(&mut self, queue: &wgpu::Queue) -> Option<Result<Histogram, String>> {
        let done = self.receiver.try_recv().ok()?;
        self.active = false;
        let readback = self
            .readback
            .take()
            .expect("active histogram owns readback");
        let result = if (done.cancelled)() {
            None
        } else {
            Some(done.result.and_then(|()| read_histogram(&readback.buffer)))
        };
        readback.buffer.unmap();
        drop(readback);

        if let Some(pending) = self.pending.take()
            && !(pending.cancelled)()
            && let Err(error) = self.start(queue, pending)
        {
            return Some(Err(error));
        }
        result
    }

    pub fn is_pending(&self) -> bool {
        self.active || self.pending.is_some()
    }

    pub fn cancel_pending(&mut self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.pending = None;
    }
}

fn read_histogram(buffer: &wgpu::Buffer) -> Result<Histogram, String> {
    let view = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|e| e.to_string())?;
    let words = bytemuck::cast_slice::<u8, u32>(&view);
    let mut histogram = Histogram::default();
    for c in 0..4 {
        for i in 0..64 {
            histogram.channels[c][i] = u64::from(words[c * 64 + i]);
        }
        histogram.negative[c] = u64::from(words[256 + c]);
        histogram.zero[c] = u64::from(words[260 + c]);
        histogram.underflow[c] = u64::from(words[264 + c]);
        histogram.overflow[c] = u64::from(words[268 + c]);
    }
    histogram.sampled = u64::from(words[272]);
    histogram.invalid = u64::from(words[273]);
    histogram.transparent = u64::from(words[274]);
    Ok(histogram)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::validate_wgsl;

    #[test]
    fn analytic_sdr_hdr_signed_alpha_and_nonfinite_policies() {
        let values = [
            0.18,
            0.36,
            0.72,
            1.,
            -1.,
            0.,
            f32::MAX,
            1.,
            f32::NAN,
            0.,
            0.,
            1.,
            4.,
            4.,
            4.,
            0.,
        ];
        let histogram = Histogram::linear_rgba(&values, 16, 0., || false).unwrap();
        assert_eq!(histogram.sampled, 4);
        assert_eq!(histogram.invalid, 1);
        assert_eq!(histogram.transparent, 1);
        assert_eq!(histogram.channels[0][32], 1);
        assert_eq!(histogram.channels[1][34], 1);
        assert_eq!(histogram.channels[2][36], 1);
        assert_eq!(histogram.negative[0], 1);
        assert_eq!(histogram.zero[1], 1);
        assert_eq!(histogram.overflow[2], 1);
        let shifted = Histogram::linear_rgba(&values[..4], 16, 1., || false).unwrap();
        assert_eq!(shifted.channels[0][34], 1);
    }

    #[test]
    fn sample_count_and_cancellation_are_bounded() {
        let input = vec![1.; 400_000];
        let histogram = Histogram::linear_rgba(&input, 100, 0., || false).unwrap();
        assert_eq!(histogram.sampled, 100);
        let mut checks = 0;
        assert!(matches!(
            Histogram::linear_rgba(&input, MAX_SAMPLES, 0., || {
                checks += 1;
                checks == 3
            }),
            Err(HistogramError::Cancelled)
        ));
        assert!(Histogram::linear_rgba(&[1.; 3], 1, 0., || false).is_err());
    }

    #[test]
    fn histogram_shader_validates_and_storage_is_bounded() {
        validate_wgsl(HISTOGRAM_SHADER, wgpu::naga::valid::Capabilities::all());
        assert_eq!(HISTOGRAM_JOB_BYTES, 2232);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn odd_crops_extreme_values_and_repeated_navigation_metal() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let width = 513u32;
        let height = 259u32;
        let patches = [
            [0.23, 0.51, 1.13, 1.],
            [-1., 0., 4., 1.],
            [f32::MAX, f32::MAX, f32::MAX, 1.],
            [1e-20, 1e-20, 1e-20, 1.],
            [f32::NAN, 1., 1., 1.],
            [1., 1., 1., 0.],
            [1., 1., 1., -0.1],
            [1., 1., 1., 1.1],
            [f32::INFINITY, 0., 0., 1.],
        ];
        let values: Vec<f32> = (0..width * height)
            .flat_map(|i| patches[i as usize % patches.len()])
            .collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&values),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let make = |crop| Input {
            buffer: buffer.clone(),
            dimensions: [width, height],
            crop,
            resources: Default::default(),
            cancelled: Arc::new(|| false),
        };
        let budget = MemoryBudget::new(HISTOGRAM_JOB_BYTES);
        let mut worker = GpuHistogram::new(&device, budget.clone(), Arc::new(|| {}));
        for crop in [
            HistogramRect {
                x: 3,
                y: 5,
                width: 507,
                height: 251,
            },
            HistogramRect {
                x: 512,
                y: 258,
                width: 1,
                height: 1,
            },
            HistogramRect {
                x: 0,
                y: 0,
                width,
                height,
            },
        ] {
            let mut cropped = Vec::new();
            for y in crop.y..crop.y + crop.height {
                let start = ((y * width + crop.x) * 4) as usize;
                cropped.extend_from_slice(&values[start..start + crop.width as usize * 4]);
            }
            for _ in 0..4 {
                worker.submit(&queue, make(crop)).unwrap();
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let actual = worker.poll(&queue).unwrap().unwrap();
                assert_eq!(
                    actual,
                    Histogram::linear_rgba(&cropped, MAX_SAMPLES, 0., || false).unwrap()
                );
                assert_eq!(budget.used(), 0);
            }
        }
        for crop in [
            HistogramRect {
                x: width,
                y: 0,
                width: 1,
                height: 1,
            },
            HistogramRect {
                x: u32::MAX,
                y: 0,
                width: 2,
                height: 1,
            },
            HistogramRect {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
        ] {
            assert!(worker.submit(&queue, make(crop)).is_err());
            assert_eq!(budget.used(), 0);
        }
        let mut short = make(HistogramRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        });
        short.dimensions = [width, height + 1];
        assert!(worker.submit(&queue, short).is_err());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn newer_frame_supersedes_result_even_within_one_decode_generation_metal() {
        let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[0.23f32, 0.51, 1.13, 1.]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let make = || Input {
            buffer: buffer.clone(),
            dimensions: [1, 1],
            crop: HistogramRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            resources: Default::default(),
            cancelled: Arc::new(|| false),
        };
        let budget = MemoryBudget::new(HISTOGRAM_JOB_BYTES);
        let mut h = GpuHistogram::new(&device, budget.clone(), Arc::new(|| {}));
        h.submit(&queue, make()).unwrap();
        h.submit(&queue, make()).unwrap();
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(
            h.poll(&queue).is_none(),
            "older animation/page histogram must not be published"
        );
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert_eq!(h.poll(&queue).unwrap().unwrap().sampled, 1);
        assert_eq!(budget.used(), 0);
    }
}
