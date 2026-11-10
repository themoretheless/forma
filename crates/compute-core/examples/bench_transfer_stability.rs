//! Controlled transfer experiments. Separate CPU batches from GPU call latency;
//! counterbalanced ABBA/BAAB blocks plus identical-code AA controls, no timed IO.
use compute_core::{
    ComputeRuntime,
    gpu_compute::{self, ByteReadback, GpuContext, wgpu},
};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
const WAIT: Duration = Duration::from_secs(10);
const ROUNDS: usize = 24;
#[derive(Clone, Copy, Default)]
struct Timing {
    cpu: f64,
    total: f64,
}
impl Timing {
    fn mean(a: Self, b: Self) -> Self {
        Self {
            cpu: (a.cpu + b.cpu) * 0.5,
            total: (a.total + b.total) * 0.5,
        }
    }
}
struct Row {
    phase: &'static str,
    case: &'static str,
    bytes: usize,
    round: usize,
    order: &'static str,
    n: usize,
    a: Timing,
    b: Timing,
}
fn ns(t: Instant) -> f64 {
    t.elapsed().as_nanos() as f64
}

fn measure(
    rows: &mut Vec<Row>,
    phase: &'static str,
    case: &'static str,
    bytes: usize,
    n: usize,
    mut sample: impl FnMut(bool, usize) -> Timing,
) {
    let warm = Instant::now();
    while warm.elapsed() < Duration::from_millis(150) {
        sample(false, n);
        sample(true, n);
    }
    for round in 0..ROUNDS {
        let paths = if round % 2 == 0 {
            [false, true, true, false]
        } else {
            [true, false, false, true]
        };
        let samples = paths.map(|v| sample(v, n));
        let (a, b) = if round % 2 == 0 {
            (
                Timing::mean(samples[0], samples[3]),
                Timing::mean(samples[1], samples[2]),
            )
        } else {
            (
                Timing::mean(samples[1], samples[2]),
                Timing::mean(samples[0], samples[3]),
            )
        };
        rows.push(Row {
            phase,
            case,
            bytes,
            round,
            order: if round % 2 == 0 { "ABBA" } else { "BAAB" },
            n,
            a,
            b,
        });
    }
}

#[derive(Debug, PartialEq)]
enum Output {
    F32(Vec<f32>),
    Bytes(Vec<u8>),
}
// Keep the typed decoder monomorphic, as in the production callback. A generic
// recursive enum-returning decoder inhibited LLVM's bulk-copy optimization and
// created a false win for an explicit memcpy candidate in the diagnostic runs.
#[inline(never)]
fn decode_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v))
        .collect()
}
fn decode(mapped: &[u8], kind: &str, variant: usize) -> Output {
    if variant == 0 {
        let raw = mapped.to_vec();
        return decode(&raw, kind, 1);
    }
    if kind == "f32" {
        Output::F32(decode_f32(mapped))
    } else {
        let row = 4096;
        let useful = if kind == "rgba_aligned" { row } else { row - 4 };
        if variant == 2 && useful == row {
            return Output::Bytes(mapped.to_vec());
        }
        let mut out = Vec::with_capacity(mapped.len() / row * useful);
        for line in mapped.chunks_exact(row) {
            out.extend_from_slice(&line[..useful]);
        }
        Output::Bytes(out)
    }
}
fn update(
    c: &GpuContext,
    dst: &wgpu::Buffer,
    values: &[f64],
    floats: &[f32],
    case: &str,
    optimized: bool,
) {
    if case == "borrowed" || case == "borrowed_aa" {
        if optimized || case.ends_with("_aa") {
            c.queue
                .write_buffer(dst, 0, &gpu_compute::f32_le_bytes(floats));
        } else {
            c.queue.write_buffer(dst, 0, &gpu_compute::pack_f32(floats));
        }
    } else if !optimized && !case.ends_with("_aa") {
        let flat: Vec<f32> = values.iter().map(|&v| v as f32).collect();
        c.queue.write_buffer(dst, 0, &gpu_compute::pack_f32(&flat));
    } else if case == "generated_buffered" {
        let flat: Vec<f32> = values.iter().map(|&v| v as f32).collect();
        c.queue
            .write_buffer(dst, 0, &gpu_compute::f32_le_bytes(&flat));
    } else {
        gpu_compute::write_buffer_words(
            &c.queue,
            dst,
            0,
            values.iter().map(|&v| (v as f32).to_le_bytes()),
        )
        .unwrap();
    }
}
fn drain(c: &GpuContext, index: wgpu::SubmissionIndex) {
    c.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: Some(WAIT),
        })
        .unwrap();
}
fn main() {
    // Fixed QoS for this benchmark thread only. This neither pins cores nor
    // controls system clocks, and applies equally to candidates and controls.
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32;
        }
        assert_eq!(pthread_set_qos_class_self_np(0x19, 0), 0);
    }
    let phase = std::env::args().nth(1).unwrap_or_else(|| "cpu".into());
    assert!(phase == "cpu" || phase == "full" || phase == "production");
    let c = GpuContext::new().expect("GPU required");
    let rt = ComputeRuntime::new(&c).unwrap();
    let info = c.device.adapter_info();
    let mut rows = Vec::new();
    for bytes in [4096, 1048576, 16777216] {
        let values: Vec<f64> = (0..bytes / 4)
            .map(|i| (i % 4093) as f64 * 0.03125 - 17.0)
            .collect();
        let floats: Vec<f32> = values.iter().map(|&v| v as f32).collect();
        let input = rt.zeros::<f32>(floats.len()).unwrap();
        let output = rt.zeros::<f32>(floats.len()).unwrap();
        let mut program = rt.program();
        program.affine_into(&input, 2.0, -1.0, &output).unwrap();
        for case in [
            "borrowed",
            "generated_direct",
            "generated_buffered",
            "borrowed_aa",
            "generated_aa",
        ]
        .into_iter()
        .filter(|_| phase != "production")
        {
            // Preflight checks are outside warm-up/measurement, not between samples.
            for candidate in [false, true] {
                update(&c, input.view().raw(), &values, &floats, case, candidate);
                drain(&c, program.submit());
                let result = rt.read(&output).unwrap().wait(WAIT).unwrap();
                assert!(
                    result
                        .iter()
                        .zip(&floats)
                        .all(|(v, x)| *v == *x * 2.0 - 1.0)
                );
            }
            let n = if phase == "cpu" {
                (8 * 1024 * 1024 / bytes).clamp(1, 64)
            } else {
                if bytes < 16777216 { 8 } else { 2 }
            };
            measure(
                &mut rows,
                if phase == "cpu" {
                    "cpu_update"
                } else {
                    "full_update"
                },
                case,
                bytes,
                n,
                |candidate, n| {
                    let started = Instant::now();
                    let mut cpu = 0.0;
                    for _ in 0..n {
                        let t = Instant::now();
                        update(&c, input.view().raw(), &values, &floats, case, candidate);
                        cpu += ns(t);
                        if phase == "full" {
                            drain(&c, program.submit());
                        }
                    }
                    let total = ns(started) / n as f64;
                    // CPU batches enqueue all uploads, then flush outside CPU timing.
                    if phase == "cpu" {
                        drain(&c, c.queue.submit([]));
                    }
                    Timing {
                        cpu: cpu / n as f64,
                        total: if phase == "cpu" {
                            cpu / n as f64
                        } else {
                            total
                        },
                    }
                },
            );
        }

        // Force an actual GPU-written private source, identical for both paths.
        rt.write(&input, 0, &floats).unwrap();
        drain(&c, program.submit());
        let source = output.view();
        let staging = c.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stability reusable snapshot"),
            size: bytes as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if phase == "production" {
            for case in ["original_vs_current", "scalar_vs_current", "current_aa"] {
                let read = |path: bool| -> Vec<f32> {
                    if path || case == "current_aa" {
                        compute_core::try_read_f32(&c.device, &c.queue, source.raw(), bytes / 4)
                            .unwrap()
                    } else {
                        let mut encoder = c.device.create_command_encoder(&Default::default());
                        let mut ticket = ByteReadback::copy_buffer(
                            &c.device,
                            &mut encoder,
                            source.raw(),
                            0,
                            bytes as u64,
                        )
                        .unwrap();
                        ticket.submitted(c.queue.submit([encoder.finish()]));
                        if case == "original_vs_current" {
                            let raw = ticket.wait(WAIT).unwrap();
                            decode_f32(&raw)
                        } else {
                            ticket.wait_with(WAIT, decode_f32).unwrap()
                        }
                    }
                };
                let expected: Vec<f32> = floats.iter().map(|x| *x * 2.0 - 1.0).collect();
                assert_eq!(read(false), expected);
                assert_eq!(read(true), expected);
                measure(
                    &mut rows,
                    "production_f32",
                    case,
                    bytes,
                    if bytes < 16777216 { 8 } else { 2 },
                    |path, n| {
                        let t = Instant::now();
                        for _ in 0..n {
                            drop(black_box(read(path)));
                        }
                        // Internal CPU time is intentionally unavailable through the public API.
                        Timing {
                            cpu: 0.0,
                            total: ns(t) / n as f64,
                        }
                    },
                );
            }
            continue;
        }
        if phase == "cpu" {
            let mut encoder = c.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(source.raw(), 0, &staging, 0, bytes as u64);
            drain(&c, c.queue.submit([encoder.finish()]));
            gpu_compute::try_read_buffer_with(&c.device, &staging, bytes, |mapped| {
                for (case, kind, candidate, control) in [
                    ("f32_direct", "f32", 1, false),
                    ("rgba_padded", "rgba_padded", 1, false),
                    ("rgba_aligned", "rgba_aligned", 1, false),
                    ("f32_aa", "f32", 1, true),
                    ("rgba_aa", "rgba_padded", 1, true),
                ] {
                    assert_eq!(decode(mapped, kind, 0), decode(mapped, kind, candidate));
                    let n = (32 * 1024 * 1024 / bytes).clamp(2, 2048);
                    measure(&mut rows, "cpu_read", case, bytes, n, |path, n| {
                        let variant = if path || control { candidate } else { 0 };
                        let start = Instant::now();
                        for _ in 0..n {
                            drop(black_box(decode(black_box(mapped), kind, variant)));
                        }
                        let cpu = ns(start) / n as f64;
                        Timing { cpu, total: cpu }
                    });
                }
            })
            .unwrap();
        } else {
            for (case, kind, candidate, control) in [
                ("f32_direct", "f32", 1, false),
                ("rgba_padded", "rgba_padded", 1, false),
                ("rgba_aligned", "rgba_aligned", 1, false),
                ("f32_aa", "f32", 1, true),
                ("rgba_aa", "rgba_padded", 1, true),
            ] {
                for reuse in [false, true] {
                    let n = if bytes < 16777216 { 8 } else { 2 };
                    measure(
                        &mut rows,
                        if reuse {
                            "full_read_reuse"
                        } else {
                            "full_read_fresh"
                        },
                        case,
                        bytes,
                        n,
                        |path, n| {
                            let started = Instant::now();
                            let mut cpu = 0.0;
                            for _ in 0..n {
                                let mut encoder =
                                    c.device.create_command_encoder(&Default::default());
                                let mut ticket = if reuse {
                                    encoder.copy_buffer_to_buffer(
                                        source.raw(),
                                        0,
                                        &staging,
                                        0,
                                        bytes as u64,
                                    );
                                    ByteReadback::map_on_submit(
                                        &c.device,
                                        &encoder,
                                        &staging,
                                        bytes as u64,
                                    )
                                    .unwrap()
                                } else {
                                    ByteReadback::copy_buffer(
                                        &c.device,
                                        &mut encoder,
                                        source.raw(),
                                        0,
                                        bytes as u64,
                                    )
                                    .unwrap()
                                };
                                ticket.submitted(c.queue.submit([encoder.finish()]));
                                let result = if !path && !control {
                                    // Match the former transport exactly: byte copy while mapped,
                                    // unmap, then decode the owned bytes outside the mapping.
                                    let raw = ticket
                                        .wait_with(WAIT, |mapped| {
                                            let t = Instant::now();
                                            let raw = mapped.to_vec();
                                            cpu += ns(t);
                                            raw
                                        })
                                        .unwrap();
                                    let t = Instant::now();
                                    let result = decode(&raw, kind, 1);
                                    cpu += ns(t);
                                    result
                                } else {
                                    ticket
                                        .wait_with(WAIT, |mapped| {
                                            let t = Instant::now();
                                            let result = decode(mapped, kind, candidate);
                                            cpu += ns(t);
                                            result
                                        })
                                        .unwrap()
                                };
                                drop(black_box(result));
                            }
                            Timing {
                                cpu: cpu / n as f64,
                                total: ns(started) / n as f64,
                            }
                        },
                    );
                }
            }
        }
    }
    // No stdout or validation reads occur between measured samples.
    println!(
        "# adapter={} backend={:?} phase={} qos=user_initiated rounds={} order=ABBA/BAAB warmup_ms=150",
        info.name, info.backend, phase, ROUNDS
    );
    println!("phase,case,bytes,round,order,iterations,a_cpu_ns,b_cpu_ns,a_total_ns,b_total_ns");
    for r in rows {
        println!(
            "{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3}",
            r.phase, r.case, r.bytes, r.round, r.order, r.n, r.a.cpu, r.b.cpu, r.a.total, r.b.total
        );
    }
}
