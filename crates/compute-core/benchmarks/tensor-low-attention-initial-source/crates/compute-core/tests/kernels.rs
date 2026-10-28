//! compute-core kernel tests: naga validation of the shipped WGSL, plus GPU
//! correctness against CPU references. GPU tests skip without an adapter.

use compute_core::gpu_compute::GpuContext;
use compute_core::shaders::{
    ALL, BLOCK_SUM_WGSL, SCALE_ADD_WGSL, SCALE_ADD4_WGSL, ZIP_MUL_WGSL, ZIP_MUL4_WGSL,
};
use compute_core::{
    Binding, Kernel, read_f32, reduce_f32, storage_f32, storage_f32_zeroed, uniform_f32,
};

fn validate(name: &str, source: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|error| panic!("{name}: WGSL parse failed: {error}"));
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name}: WGSL validation failed: {error}"));
}

#[test]
fn shipped_kernels_validate_with_naga() {
    for (name, source) in ALL {
        validate(name, source);
    }
}

#[test]
fn tuned_workgroup_substitution_stays_valid() {
    // Every power-of-two size a backend tuner may pick must still validate.
    let Some(context) = GpuContext::new() else {
        return;
    };
    for size in [64u32, 128, 256, 512] {
        let source = SCALE_ADD_WGSL.replacen(
            "const WG: u32 = 256;",
            &format!("const WG: u32 = {size};"),
            1,
        );
        validate(&format!("scale_add@WG{size}"), &source);
        let kernel = Kernel::with_workgroup_size(
            &context.device,
            "scale_add",
            SCALE_ADD_WGSL,
            "main",
            &[
                Binding::Uniform,
                Binding::StorageRead,
                Binding::StorageReadWrite,
            ],
            size,
        );
        assert!(kernel.is_ok(), "WG{size} kernel failed: {:?}", kernel.err());
    }
}

#[test]
fn non_power_of_two_workgroup_is_rejected() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let kernel = Kernel::with_workgroup_size(
        &context.device,
        "scale_add",
        SCALE_ADD_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
        192,
    );
    assert!(
        kernel.is_err(),
        "non-power-of-two workgroup must be rejected"
    );
}

#[test]
fn broken_wgsl_is_an_error_not_a_panic() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let kernel = Kernel::new(&context.device, "broken", "fn oops( {", "main", &[]);
    assert!(kernel.is_err());
}

/// Params layout of scale_add: count(u32), scale(f32), offset(f32), pad.
fn scale_add_params(count: u32, scale: f32, offset: f32) -> Vec<f32> {
    let mut floats = vec![0.0f32; 4];
    floats[0] = f32::from_le_bytes(count.to_le_bytes());
    floats[1] = scale;
    floats[2] = offset;
    floats
}

#[test]
fn scale_add_matches_cpu_reference() {
    let Some(context) = GpuContext::new() else {
        eprintln!("compute-core: no GPU adapter available, skipping");
        return;
    };
    eprintln!("compute-core: computing on {}", context.backend_label());
    let device = &context.device;
    let queue = &context.queue;
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

    let input: Vec<f32> = (0..1000).map(|i| (i as f32 - 500.0) * 0.25).collect();
    let params = uniform_f32(
        device,
        queue,
        &scale_add_params(input.len() as u32, 2.5, -1.25),
    );
    let input_buf = storage_f32(device, queue, &input);
    let output_buf = storage_f32_zeroed(device, queue, input.len());
    kernel.dispatch(
        device,
        queue,
        &[&params, &input_buf, &output_buf],
        input.len() as u32,
    );

    let output = read_f32(device, queue, &output_buf, input.len());
    for (i, (&got, &x)) in output.iter().zip(&input).enumerate() {
        let want = x * 2.5 - 1.25;
        assert!(
            (got - want).abs() < 1e-5,
            "scale_add mismatch at {i}: got {got}, want {want}"
        );
    }
}

#[test]
fn zip_mul_then_block_sum_computes_the_dot_product() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;

    let n = 10_000u32;
    let a: Vec<f32> = (0..n).map(|i| (i % 97) as f32 * 0.1).collect();
    let b: Vec<f32> = (0..n).map(|i| (i % 53) as f32 * -0.2).collect();

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

    // products = a * b
    let mut params = vec![0.0f32; 4];
    params[0] = f32::from_le_bytes(n.to_le_bytes());
    let groups = sum.workgroup_count(n);
    params[1] = f32::from_le_bytes(groups.to_le_bytes());
    let params_buf = uniform_f32(device, queue, &params);
    let a_buf = storage_f32(device, queue, &a);
    let b_buf = storage_f32(device, queue, &b);
    let products = storage_f32_zeroed(device, queue, n as usize);
    mul.dispatch(device, queue, &[&params_buf, &a_buf, &b_buf, &products], n);

    // partial reduction on the GPU, final fold on the CPU.
    let partials = storage_f32_zeroed(device, queue, groups as usize);
    sum.dispatch(device, queue, &[&params_buf, &products, &partials], n);
    let partials = read_f32(device, queue, &partials, groups as usize);

    let want: f32 = a.iter().zip(&b).map(|(&x, &y)| x * y).sum();
    let got: f32 = partials.iter().sum();
    assert!(
        (got - want).abs() < want.abs().max(1.0) * 1e-4,
        "dot product mismatch: got {got}, want {want}"
    );
}

#[test]
fn reduction_handles_non_multiple_lengths() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;
    let sum = Kernel::with_workgroup_size(
        device,
        "block_sum",
        BLOCK_SUM_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
        64,
    )
    .expect("block_sum builds");

    // 257 elements over 64-wide workgroups: 5 groups, last one partial.
    let n = 257u32;
    let data: Vec<f32> = (1..=n).map(|i| i as f32).collect();
    let mut params = vec![0.0f32; 4];
    params[0] = f32::from_le_bytes(n.to_le_bytes());
    let groups = sum.workgroup_count(n);
    params[1] = f32::from_le_bytes(groups.to_le_bytes());
    let params_buf = uniform_f32(device, queue, &params);
    let input = storage_f32(device, queue, &data);
    assert_eq!(groups, 5);
    let partials = storage_f32_zeroed(device, queue, groups as usize);
    sum.dispatch(device, queue, &[&params_buf, &input, &partials], n);
    let partials = read_f32(device, queue, &partials, groups as usize);
    let want: f32 = data.iter().sum();
    let got: f32 = partials.iter().sum();
    assert_eq!(got, want, "partial sums must fold to the exact total");
}

/// Packs f32 data as vec4 lanes: count is the number of vec4 elements.
fn vec4_params(count: u32, scale: f32, offset: f32) -> Vec<f32> {
    let mut floats = vec![0.0f32; 4];
    floats[0] = f32::from_le_bytes(count.to_le_bytes());
    floats[1] = scale;
    floats[2] = offset;
    floats
}

#[test]
fn scale_add4_matches_cpu_reference() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;
    let kernel = Kernel::tuned(
        &context,
        "scale_add4",
        SCALE_ADD4_WGSL,
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

    let lanes = 999u32; // odd lane count: exercises the tail guard
    let input: Vec<f32> = (0..lanes * 4)
        .map(|i| (i as f32 - 2000.0) * 0.125)
        .collect();
    let params = uniform_f32(device, queue, &vec4_params(lanes, 1.5, 0.25));
    let input_buf = storage_f32(device, queue, &input);
    let output_buf = storage_f32_zeroed(device, queue, input.len());
    kernel.dispatch(device, queue, &[&params, &input_buf, &output_buf], lanes);

    let output = read_f32(device, queue, &output_buf, input.len());
    for (i, (&got, &x)) in output.iter().zip(&input).enumerate() {
        let want = x * 1.5 + 0.25;
        assert!(
            (got - want).abs() < 1e-5,
            "scale_add4 mismatch at {i}: got {got}, want {want}"
        );
    }
}

#[test]
fn zip_mul4_then_reduce_computes_dot_product() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;

    let mul = Kernel::new(
        device,
        "zip_mul4",
        ZIP_MUL4_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )
    .expect("zip_mul4 builds");
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

    let lanes = 2500u32;
    let n = lanes * 4;
    let a: Vec<f32> = (0..n).map(|i| (i % 89) as f32 * 0.1).collect();
    let b: Vec<f32> = (0..n).map(|i| (i % 47) as f32 * -0.3).collect();
    let params = uniform_f32(device, queue, &vec4_params(lanes, 0.0, 0.0));
    let a_buf = storage_f32(device, queue, &a);
    let b_buf = storage_f32(device, queue, &b);
    let products = storage_f32_zeroed(device, queue, n as usize);
    mul.dispatch(device, queue, &[&params, &a_buf, &b_buf, &products], lanes);

    // grid-strided single pass: one group per workgroup covers all lanes.
    let mut sum_params = vec![0.0f32; 4];
    sum_params[0] = f32::from_le_bytes(n.to_le_bytes());
    let groups = sum.workgroup_count(n);
    sum_params[1] = f32::from_le_bytes(groups.to_le_bytes());
    let sum_params = uniform_f32(device, queue, &sum_params);
    let partials = storage_f32_zeroed(device, queue, groups as usize);
    sum.dispatch(device, queue, &[&sum_params, &products, &partials], n);

    let partials = read_f32(device, queue, &partials, groups as usize);
    let want: f32 = a.iter().zip(&b).map(|(&x, &y)| x * y).sum();
    let got: f32 = partials.iter().sum();
    assert!(
        (got - want).abs() < want.abs().max(1.0) * 1e-4,
        "vec4 dot product mismatch: got {got}, want {want}"
    );
}

#[test]
fn reduce_f32_handles_lengths_beyond_the_dispatch_limit() {
    // 20M elements: the first pass needs more than 65535 workgroups at
    // WG=256, so reduce_f32 must schedule multiple passes on its own.
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;
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

    let n = 20_000_000u32;
    let data: Vec<f32> = (0..n).map(|i| (i % 1000) as f32).collect();
    let input = storage_f32(device, queue, &data);
    let got = reduce_f32(device, queue, &sum, &input, n);

    // Exact by construction: 20000 full cycles of 0..=999.
    let want: f32 = 499_500.0 * (n / 1000) as f32;
    assert!(
        (got - want).abs() < want.abs() * 1e-5,
        "multi-pass reduce mismatch: got {got}, want {want}"
    );
}

#[test]
fn recorded_chain_reuses_buffers_and_consumes_reduction_on_gpu() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;
    let bindings = [
        Binding::Uniform,
        Binding::StorageRead,
        Binding::StorageReadWrite,
    ];
    let scale = Kernel::new(device, "scale", SCALE_ADD_WGSL, "main", &bindings).unwrap();
    let mul = Kernel::new(
        device,
        "multiply",
        ZIP_MUL_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )
    .unwrap();
    let sum =
        Kernel::with_workgroup_size(device, "sum", BLOCK_SUM_WGSL, "main", &bindings, 64).unwrap();
    let n = 4097;
    let input = storage_f32_zeroed(device, queue, n);
    let scaled = storage_f32_zeroed(device, queue, n);
    let products = storage_f32_zeroed(device, queue, n);
    let result = storage_f32_zeroed(device, queue, 1);
    let params = uniform_f32(device, queue, &scale_add_params(n as u32, 2.0, 1.0));
    let tail_params = uniform_f32(device, queue, &scale_add_params(1, 0.5, -3.0));
    let scale_bind = scale.create_bind_group(device, &[&params, &input, &scaled]);
    let mul_bind = mul.create_bind_group(device, &[&params, &scaled, &input, &products]);
    let reduction = compute_core::Reduction::new(device, queue, &sum, &products, n as u32);
    let tail_bind = scale.create_bind_group(device, &[&tail_params, reduction.output(), &result]);
    let mut batch = compute_core::ComputeBatch::new();
    batch.push(&scale, &scale_bind, scale.workgroup_count(n as u32));
    batch.push(&mul, &mul_bind, mul.workgroup_count(n as u32));
    batch.push_reduction(&reduction);
    batch.push(&scale, &tail_bind, 1);
    drop(reduction); // The batch retains the reduction bindings and buffers.
    for iteration in 0..3 {
        let values: Vec<f32> = (0..n).map(|i| ((i + iteration) % 7) as f32).collect();
        queue.write_buffer(&input, 0, &compute_core::gpu_compute::pack_f32(&values));
        batch.submit(device, queue);
        let want = values.iter().map(|x| (x * 2.0 + 1.0) * x).sum::<f32>() * 0.5 - 3.0;
        assert_eq!(read_f32(device, queue, &result, 1), vec![want]);
    }
}

#[test]
fn reduction_handles_empty_singleton_and_workgroup_one() {
    let Some(context) = GpuContext::new() else {
        return;
    };
    let device = &context.device;
    let queue = &context.queue;
    for size in [1, 64] {
        let sum = Kernel::with_workgroup_size(
            device,
            "sum",
            BLOCK_SUM_WGSL,
            "main",
            &[
                Binding::Uniform,
                Binding::StorageRead,
                Binding::StorageReadWrite,
            ],
            size,
        )
        .unwrap();
        for n in [0, 1, 3, 257] {
            let values: Vec<f32> = (0..n).map(|i| i as f32 + 1.0).collect();
            let input = storage_f32(device, queue, &values);
            assert_eq!(
                reduce_f32(device, queue, &sum, &input, n),
                values.iter().sum::<f32>()
            );
            assert!(read_f32(device, queue, &input, 0).is_empty());
            assert!(compute_core::read_u32(device, queue, &input, 0).is_empty());
        }
    }
}
