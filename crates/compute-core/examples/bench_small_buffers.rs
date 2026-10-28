//! Counterbalanced small-update experiments; no output inside timed sections.
use compute_core::{
    ComputeRuntime, GpuArray,
    gpu_compute::{self, GpuContext, wgpu},
};
use std::time::{Duration, Instant};
const WAIT: Duration = Duration::from_secs(10);
fn drain(c: &GpuContext) {
    let index = c.queue.submit([]);
    c.device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(index),
            timeout: Some(WAIT),
        })
        .unwrap();
}
fn update(
    c: &GpuContext,
    rt: &ComputeRuntime,
    array: &GpuArray<u32>,
    bytes: usize,
    kind: &str,
    candidate: bool,
) {
    let b = array.view().raw();
    if kind == "generated" {
        if candidate {
            let mut scratch = [0u8; 4096];
            for (i, word) in scratch[..bytes].chunks_exact_mut(4).enumerate() {
                word.copy_from_slice(&((i as f64 * 0.25) as f32).to_le_bytes());
            }
            c.queue.write_buffer(b, 0, &scratch[..bytes]);
        } else {
            gpu_compute::write_buffer_words(
                &c.queue,
                b,
                0,
                (0..bytes / 4).map(|i| ((i as f64 * 0.25) as f32).to_le_bytes()),
            )
            .unwrap();
        }
    } else {
        let values = [0x0d0d0d0du32; 1024];

        if candidate && kind != "aa" {
            let n = bytes / 16;
            rt.write_small_parts(
                array,
                0,
                &[
                    &values[..n],
                    &values[n..2 * n],
                    &values[2 * n..3 * n],
                    &values[3 * n..4 * n],
                ],
            )
            .unwrap();
        } else {
            for (i, part) in values[..bytes / 4].chunks(bytes / 16).enumerate() {
                rt.write(array, i * bytes / 16, part).unwrap();
            }
        }
    }
}
fn main() {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(q: u32, p: i32) -> i32;
        }
        assert_eq!(pthread_set_qos_class_self_np(0x19, 0), 0);
    }
    let c = GpuContext::new().expect("GPU required");
    assert_eq!(c.backend, wgpu::Backend::Metal);
    eprintln!("{:?}", c.device.adapter_info());
    let rt = ComputeRuntime::new(&c).unwrap();
    let array = rt.zeros::<u32>(1024).unwrap();
    let b = array.view().raw();
    let mut rows = Vec::new();
    for bytes in [16, 64, 256, 1024, 4096] {
        for kind in ["generated", "four_parts", "aa"] {
            for candidate in [false, true] {
                update(&c, &rt, &array, bytes, kind, candidate);
                drain(&c);
                let mut encoder = c.device.create_command_encoder(&Default::default());
                let mut ticket = gpu_compute::ByteReadback::copy_buffer(
                    &c.device,
                    &mut encoder,
                    &b,
                    0,
                    bytes as u64,
                )
                .unwrap();
                ticket.submitted(c.queue.submit([encoder.finish()]));
                let out = ticket.wait(WAIT).unwrap();
                let expected: Vec<u8> = if kind == "generated" {
                    (0..bytes / 4)
                        .flat_map(|i| ((i as f64 * 0.25) as f32).to_le_bytes())
                        .collect()
                } else {
                    vec![13; bytes]
                };
                assert_eq!(out, expected);
            }
            for phase in ["cpu", "full"] {
                let n = if phase == "cpu" { 128 } else { 8 };
                let mut sample = |candidate: bool| {
                    let start = Instant::now();
                    for _ in 0..n {
                        update(&c, &rt, &array, bytes, kind, candidate);
                        if phase == "full" {
                            drain(&c);
                        }
                    }
                    let ns = start.elapsed().as_nanos() as f64 / n as f64;
                    if phase == "cpu" {
                        drain(&c);
                    }
                    ns
                };
                let warm = Instant::now();
                while warm.elapsed() < Duration::from_millis(80) {
                    sample(false);
                    sample(true);
                }
                for round in 0..24 {
                    let order = if round % 2 == 0 {
                        [false, true, true, false]
                    } else {
                        [true, false, false, true]
                    };
                    let s = order.map(&mut sample);
                    let (a, b) = if round % 2 == 0 {
                        ((s[0] + s[3]) / 2., (s[1] + s[2]) / 2.)
                    } else {
                        ((s[1] + s[2]) / 2., (s[0] + s[3]) / 2.)
                    };
                    rows.push(format!("{kind},{bytes},{phase},{round},{a},{b}"));
                }
            }
        }
    }
    println!("case,bytes,phase,round,a_ns,b_ns");
    for row in rows {
        println!("{row}");
    }
}
