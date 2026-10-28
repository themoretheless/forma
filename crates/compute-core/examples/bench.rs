//! Compute throughput benchmark: runs the generic `scale_add` elementwise
//! kernel over a large f32 array and reports GB/s (read + write), plus a
//! dot-product pass (zip_mul + block_sum) as a reduction sample.
//!
//! Run: cargo run --release --offline --example bench -p compute-core

use compute_core::gpu_compute::GpuContext;
use compute_core::shaders::{BLOCK_SUM_WGSL, SCALE_ADD_WGSL, ZIP_MUL_WGSL};
use compute_core::{Binding, Kernel, read_f32, storage_f32, storage_f32_zeroed, uniform_f32};
use std::time::Instant;

const WARMUP: usize = 3;
const ITERATIONS: usize = 10;

fn scale_add_params(count: u32, scale: f32, offset: f32) -> Vec<f32> {
    let mut floats = vec![0.0f32; 4];
    floats[0] = f32::from_le_bytes(count.to_le_bytes());
    floats[1] = scale;
    floats[2] = offset;
    floats
}

fn main() {
    let Some(context) = GpuContext::new() else {
        eprintln!("compute-core bench: no GPU adapter available");
        return;
    };
    let device = &context.device;
    let queue = &context.queue;
    // --- Elementwise map: output = input * 2.5 - 1.25 ---
    let kernel = Kernel::tuned(
        &context,
        "scale_add",
        SCALE_ADD_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
        128,
        256,
    )
    .expect("scale_add builds");

    // Sized to the single-dispatch limit: 65535 workgroups times the tuned
    // workgroup size (~8.4M elements on a 128-wide Metal workgroup).
    let elements = kernel.max_dispatch_invocations() as usize;
    println!(
        "compute-core bench: {} elements, backend {}, workgroup {} (metal {}, default {})",
        elements,
        context.backend_label(),
        kernel.workgroup_size(),
        128,
        256
    );
    let input: Vec<f32> = (0..elements).map(|i| i as f32 * 1e-6).collect();
    let params = uniform_f32(
        device,
        queue,
        &scale_add_params(elements as u32, 2.5, -1.25),
    );
    let input_buf = storage_f32(device, queue, &input);
    let output_buf = storage_f32_zeroed(device, queue, elements);

    // Warmup: pipeline caches hot, queue reaches steady state.
    for _ in 0..WARMUP {
        kernel.dispatch(
            device,
            queue,
            &[&params, &input_buf, &output_buf],
            elements as u32,
        );
    }
    let bytes = (elements * 4 * 2) as f64; // read + write

    // Sustained throughput: submit the whole batch, flush once at the end.
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        kernel.dispatch(
            device,
            queue,
            &[&params, &input_buf, &output_buf],
            elements as u32,
        );
    }
    let _ = read_f32(device, queue, &output_buf, 1);
    let elapsed = start.elapsed().as_secs_f64() / ITERATIONS as f64;
    println!(
        "scale_add (elementwise map): {:7.2} GB/s sustained ({:.2} ms/iter)",
        bytes / elapsed / 1e9,
        elapsed * 1e3
    );

    // --- Reduction sample: dot product via zip_mul + block_sum ---
    let mul = Kernel::new(
        device,
        "zip_mul",
        ZIP_MUL_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )
    .expect("zip_mul builds");
    let sum = Kernel::new(
        device,
        "block_sum",
        BLOCK_SUM_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )
    .expect("block_sum builds");

    let b: Vec<f32> = (0..elements).map(|i| (i % 1024) as f32 * -1e-3).collect();
    let b_buf = storage_f32(device, queue, &b);
    let products = storage_f32_zeroed(device, queue, elements);
    let groups = sum.workgroup_count(elements as u32);
    let partials = storage_f32_zeroed(device, queue, groups as usize);

    // Warmup once, then time the sustained batch.
    mul.dispatch(
        device,
        queue,
        &[&params, &input_buf, &b_buf, &products],
        elements as u32,
    );
    sum.dispatch(
        device,
        queue,
        &[&params, &products, &partials],
        elements as u32,
    );
    let _ = read_f32(device, queue, &partials, 1);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        mul.dispatch(
            device,
            queue,
            &[&params, &input_buf, &b_buf, &products],
            elements as u32,
        );
        sum.dispatch(
            device,
            queue,
            &[&params, &products, &partials],
            elements as u32,
        );
    }
    let _ = read_f32(device, queue, &partials, 1);
    let elapsed = start.elapsed().as_secs_f64() / ITERATIONS as f64;
    let red_bytes = (elements * 4 * 3) as f64; // two reads per mul + write, then read for sum
    println!(
        "dot product (zip_mul + block_sum): {:7.2} GB/s effective, {:.2} ms/iter",
        red_bytes / elapsed / 1e9,
        elapsed * 1e3
    );

    // --- Vectorized elementwise map: vec4 lanes, 16 bytes per thread ---
    let kernel4 = Kernel::tuned(
        &context,
        "scale_add4",
        compute_core::shaders::SCALE_ADD4_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
        128,
        256,
    )
    .expect("scale_add4 builds");
    let lanes = kernel4.max_dispatch_invocations() as usize;
    let input4: Vec<f32> = (0..lanes * 4).map(|i| i as f32 * 1e-6).collect();
    let params4 = uniform_f32(device, queue, &scale_add_params(lanes as u32, 2.5, -1.25));
    let input4_buf = storage_f32(device, queue, &input4);
    let output4_buf = storage_f32_zeroed(device, queue, lanes * 4);
    for _ in 0..WARMUP {
        kernel4.dispatch(
            device,
            queue,
            &[&params4, &input4_buf, &output4_buf],
            lanes as u32,
        );
    }
    let bytes4 = (lanes * 16 * 2) as f64; // 16 bytes read + 16 written per lane
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        kernel4.dispatch(
            device,
            queue,
            &[&params4, &input4_buf, &output4_buf],
            lanes as u32,
        );
    }
    let _ = read_f32(device, queue, &output4_buf, 1);
    let elapsed = start.elapsed().as_secs_f64() / ITERATIONS as f64;
    println!(
        "scale_add4 (vec4 map):         {:7.2} GB/s sustained ({:.2} ms/iter)",
        bytes4 / elapsed / 1e9,
        elapsed * 1e3
    );
}
