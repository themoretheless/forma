//! Same-process alternating pairs for updates and readback CPU copies.
//! Exploratory single-call timings; use bench_transfer_stability and its AA
//! controls for performance decisions. Full verification between these samples
//! perturbs allocation/cache state and single-call waits are scheduler-sensitive.
//! cargo run --release -p compute-core --example bench_transfer_copies
use compute_core::{
    ComputeRuntime,
    gpu_compute::{self, ByteReadback, GpuContext, wgpu},
};
use std::time::{Duration, Instant};
const WAIT: Duration = Duration::from_secs(10);
#[derive(Clone, Copy)]
struct Timing {
    cpu_us: f64,
    total_us: f64,
}
fn micros(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}

fn pairs(kind: &str, bytes: usize, mut sample: impl FnMut(bool) -> Timing) {
    for _ in 0..3 {
        sample(false);
        sample(true);
    }
    for pair in 0..12 {
        let (old, new) = if pair % 2 == 0 {
            (sample(false), sample(true))
        } else {
            let new = sample(true);
            (sample(false), new)
        };
        println!(
            "{kind},{bytes},{pair},{},{:.3},{:.3},{:.3},{:.3}",
            if pair % 2 == 0 {
                "old-first"
            } else {
                "new-first"
            },
            old.cpu_us,
            new.cpu_us,
            old.total_us,
            new.total_us
        );
    }
}
fn generated(i: usize) -> f32 {
    ((i % 4093) as f64 * 0.03125 - 17.0) as f32
}
fn decode_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect()
}
fn decode_f64(bytes: &[u8]) -> Vec<f64> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|v| f64::from_le_bytes(*v))
        .collect()
}
fn unpack_rgba(bytes: &[u8]) -> Vec<u8> {
    // 511 RGBA pixels: 2044 useful bytes in a 2048-byte padded row.
    let mut result = Vec::with_capacity(bytes.len() / 2048 * 2044);
    for row in bytes.chunks_exact(2048) {
        result.extend_from_slice(&row[..2044]);
    }
    result
}

fn main() {
    let context = GpuContext::new().expect("GPU required");
    let runtime = ComputeRuntime::new(&context).unwrap();
    let info = context.device.adapter_info();
    println!(
        "# adapter={} backend={:?} shared_uploads={}",
        info.name,
        info.backend,
        gpu_compute::shared_memory_uploads(&context.device)
    );
    println!("kind,bytes,pair,order,old_cpu_us,new_cpu_us,old_total_us,new_total_us");
    for bytes in [4096, 1048576, 16777216] {
        let len = bytes / 4;
        let input = runtime.zeros::<f32>(len).unwrap();
        let output = runtime.zeros::<f32>(len).unwrap();
        let mut program = runtime.program();
        program.affine_into(&input, 2.0, -1.0, &output).unwrap();
        let data: Vec<f32> = (0..len).map(generated).collect();
        for kind in ["update_generated", "update_borrowed"] {
            pairs(kind, bytes, |optimized| {
                let started = Instant::now();
                match (kind, optimized) {
                    ("update_generated", false) => {
                        let values: Vec<f32> = (0..len).map(generated).collect();
                        // Previous point conversion path: float Vec, packed Vec, queue staging.
                        let packed = gpu_compute::pack_f32(&values);
                        context.queue.write_buffer(input.view().raw(), 0, &packed);
                    }
                    ("update_generated", true) => {
                        runtime.write_generated(&input, 0, len, generated).unwrap()
                    }
                    (_, false) => context.queue.write_buffer(
                        input.view().raw(),
                        0,
                        &gpu_compute::pack_f32(&data),
                    ),
                    (_, true) => context.queue.write_buffer(
                        input.view().raw(),
                        0,
                        &gpu_compute::f32_le_bytes(&data),
                    ),
                }
                let cpu_us = micros(started);
                let submission = program.submit();
                context
                    .device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(WAIT),
                    })
                    .unwrap();
                let total_us = micros(started);
                // Verify all values outside timing, draining the queue before the next sample.
                let actual = runtime.read(&output).unwrap().wait(WAIT).unwrap();
                assert!(actual.iter().zip(&data).all(|(v, x)| *v == *x * 2.0 - 1.0));
                Timing { cpu_us, total_us }
            });
        }

        // Same immutable GPU source and same snapshot copy/map for both variants.
        let source = input.view();
        for kind in ["read_f32", "read_f64", "read_rgba"] {
            pairs(kind, bytes, |optimized| {
                let started = Instant::now();
                let mut encoder = context.device.create_command_encoder(&Default::default());
                let mut ticket = ByteReadback::copy_buffer(
                    &context.device,
                    &mut encoder,
                    source.raw(),
                    0,
                    bytes as u64,
                )
                .unwrap();
                ticket.submitted(context.queue.submit([encoder.finish()]));
                let cpu_us;
                // Return a fingerprint of output outside the decoder timing so correctness
                // checking cannot dominate or be optimized away from just one variant.
                enum Output {
                    F32(Vec<f32>),
                    F64(Vec<f64>),
                    Rgba(Vec<u8>),
                }
                let (result, copy_us) = if optimized {
                    ticket
                        .wait_with(WAIT, |mapped| {
                            let decode = Instant::now();
                            let result = match kind {
                                "read_f32" => Output::F32(decode_f32(mapped)),
                                "read_f64" => Output::F64(decode_f64(mapped)),
                                _ => Output::Rgba(unpack_rgba(mapped)),
                            };
                            (result, micros(decode))
                        })
                        .unwrap()
                } else {
                    // The previous byte transport copied a complete Vec before decoding.
                    let (raw, copy_us) = ticket
                        .wait_with(WAIT, |mapped| {
                            let copy = Instant::now();
                            (mapped.to_vec(), micros(copy))
                        })
                        .unwrap();
                    let decode = Instant::now();
                    let result = match kind {
                        "read_f32" => Output::F32(decode_f32(&raw)),
                        "read_f64" => {
                            // Previous binary64 reader additionally materialized u32 words.
                            let words: Vec<u32> = raw
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|v| u32::from_le_bytes(*v))
                                .collect();
                            Output::F64(
                                words
                                    .as_chunks::<2>()
                                    .0
                                    .iter()
                                    .map(|w| {
                                        f64::from_bits(u64::from(w[0]) | (u64::from(w[1]) << 32))
                                    })
                                    .collect(),
                            )
                        }
                        _ => Output::Rgba(unpack_rgba(&raw)),
                    };
                    (result, copy_us + micros(decode))
                };
                cpu_us = copy_us;
                let total_us = micros(started);
                match result {
                    Output::F32(values) => assert_eq!(values, data),
                    Output::F64(values) => {
                        for (v, words) in values.iter().zip(data.as_chunks::<2>().0) {
                            assert_eq!(
                                v.to_bits(),
                                u64::from(words[0].to_bits())
                                    | (u64::from(words[1].to_bits()) << 32)
                            );
                        }
                    }
                    Output::Rgba(values) => {
                        let expected = unpack_rgba(&gpu_compute::f32_le_bytes(&data));
                        assert_eq!(values, expected);
                    }
                }
                Timing { cpu_us, total_us }
            });
        }
    }
}
