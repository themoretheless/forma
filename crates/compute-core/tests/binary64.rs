#![cfg(not(target_arch = "wasm32"))]
use compute_core::{
    Binding, Kernel,
    binary64::{
        ARITHMETIC_BASE_WGSL, ARITHMETIC_WGSL, TRANSCENDENTAL_WGSL, div_wgsl, exp_wgsl, log_wgsl,
        sincos_wgsl, sqrt_wgsl,
    },
    gpu_compute::{GpuContext, u32_le_bytes},
    try_read_u32,
    wgpu::{BufferDescriptor, BufferUsages},
};
const ENTRY: &str = r#"
@group(0) @binding(0) var<storage,read> input_a: array<D64>;
@group(0) @binding(1) var<storage,read> input_b: array<D64>;
@group(0) @binding(2) var<storage,read_write> output: array<D64>;
@group(0) @binding(3) var<storage,read> params: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i < arrayLength(&input_a) {
        let a = input_a[i];
        let b = input_b[i];
        let op = params[0];
        if op == 0u { output[i] = d64_add(a, b); }
        else if op == 1u { output[i] = d64_sub(a, b); }
        else if op == 2u { output[i] = d64_mul(a, b); }
        else if op == 3u { output[i] = d64_div(a, b); }
        else if op == 4u { output[i] = d64_sqrt(a); }
        else if op == 5u { output[i] = D64(u32(d64_eq(a, b)), u32(d64_lt(a, b))); }
        else if op == 6u { output[i] = d64_exp(a); }
        else if op == 7u { output[i] = d64_log(a); }
        else if op == 8u || op == 9u { output[i] = d64_sincos(a, op == 9u); }
        else { output[i] = a; }
    }
}
"#;
fn source() -> String {
    format!("{ARITHMETIC_WGSL}\n{TRANSCENDENTAL_WGSL}\n{ENTRY}")
}
fn wrap_op_entry(preamble: &str, body: &str) -> String {
    format!(
        "{preamble}\n\
         @group(0) @binding(0) var<storage, read> input_a: array<D64>;\n\
         @group(0) @binding(1) var<storage, read> input_b: array<D64>;\n\
         @group(0) @binding(2) var<storage, read_write> output: array<D64>;\n\
         @group(0) @binding(3) var<storage, read> params: array<u32>;\n\
         @compute @workgroup_size(256)\n\
         fn main(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
             let i = id.x;\n\
             if i < params[1] {{\n\
                 let a = input_a[i];\n\
                 let b = input_b[i];\n\
                 {body}\n\
             }}\n\
         }}"
    )
}
fn op_kernel_specs() -> [(&'static str, String); 6] {
    [
        (
            "integer binary64 base arithmetic",
            wrap_op_entry(
                ARITHMETIC_BASE_WGSL,
                "let op = params[0];\n\
                 if op == 0u { output[i] = d64_add(a, b); }\n\
                 else if op == 1u { output[i] = d64_sub(a, b); }\n\
                 else if op == 2u { output[i] = d64_mul(a, b); }\n\
                 else if op == 5u { output[i] = D64(u32(d64_eq(a, b)), u32(d64_lt(a, b))); }\n\
                 else { output[i] = a; }",
            ),
        ),
        (
            "integer binary64 div",
            wrap_op_entry(&div_wgsl(), "output[i] = d64_div(a, b);"),
        ),
        (
            "integer binary64 sqrt",
            wrap_op_entry(&sqrt_wgsl(), "output[i] = d64_sqrt(a);"),
        ),
        (
            "integer binary64 exp",
            wrap_op_entry(&exp_wgsl(), "output[i] = d64_exp(a);"),
        ),
        (
            "integer binary64 log",
            wrap_op_entry(&log_wgsl(), "output[i] = d64_log(a);"),
        ),
        (
            "integer binary64 sincos",
            wrap_op_entry(
                &sincos_wgsl(),
                "output[i] = d64_sincos(a, params[0] == 9u);",
            ),
        ),
    ]
}
fn attach_diagnostics(device: &compute_core::wgpu::Device) {
    device.on_uncaptured_error(std::sync::Arc::new(|err| {
        eprintln!("binary64 uncaptured error: {err}");
    }));
    device.set_device_lost_callback(|reason, msg| {
        eprintln!("binary64 device lost ({reason:?}): {msg}");
    });
}
fn compile_op_kernel(device: &compute_core::wgpu::Device, label: &str, wgsl: &str) -> Kernel {
    Kernel::new(
        device,
        label,
        wgsl,
        "main",
        &[
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
            Binding::StorageRead,
        ],
    )
    .unwrap()
}
fn validate_wgsl_and_hlsl_sm51(label: &str, wgsl: &str) {
    let module = naga::front::wgsl::parse_str(wgsl)
        .unwrap_or_else(|err| panic!("{label} WGSL parse failed: {err}"));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|err| panic!("{label} WGSL validation failed: {err:?}"));
    let hlsl = naga::back::hlsl::write_string(
        &module,
        &info,
        &naga::back::hlsl::Options {
            shader_model: naga::back::hlsl::ShaderModel::V5_1,
            ..Default::default()
        },
        &naga::back::hlsl::PipelineOptions::default(),
    )
    .unwrap_or_else(|err| panic!("{label} HLSL SM 5.1 emit failed: {err}"));
    assert!(
        !hlsl.contains("switch("),
        "{label} emitted switch() in SM 5.1 HLSL"
    );
}
#[test]
fn binary64_wgsl_validates_without_float_arithmetic() {
    validate_wgsl_and_hlsl_sm51("binary64 full", &source());
    for (label, wgsl) in op_kernel_specs() {
        validate_wgsl_and_hlsl_sm51(label, &wgsl);
    }
    let tensor_core = format!(
        "{ARITHMETIC_WGSL}\n{}\n{}",
        include_str!("../src/binary64/index.wgsl"),
        include_str!("../src/binary64/tensor.wgsl"),
    );
    validate_wgsl_and_hlsl_sm51("software binary64 tensor", &tensor_core);
    for (name, text) in [
        ("ARITHMETIC_BASE_WGSL", ARITHMETIC_BASE_WGSL),
        ("ARITHMETIC_WGSL", ARITHMETIC_WGSL),
        ("TRANSCENDENTAL_WGSL", TRANSCENDENTAL_WGSL),
    ] {
        assert!(!text.contains("f32"), "{name} contains f32");
        assert!(!text.contains("f16"), "{name} contains f16");
    }
}
fn random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
fn cases() -> Vec<(f64, f64)> {
    let special = [
        0,
        1,
        2,
        3,
        0x000f_ffff_ffff_ffff,
        0x0010_0000_0000_0000,
        0x0010_0000_0000_0001,
        0x3ca0_0000_0000_0000,
        0x3cb0_0000_0000_0000,
        0x3fe0_0000_0000_0000,
        0x3fef_ffff_ffff_ffff,
        0x3ff0_0000_0000_0000,
        0x3ff0_0000_0000_0001,
        0x3ff0_0000_0000_0002,
        0x4000_0000_0000_0000,
        0x4199_21fb_5444_2d18,
        0x7fef_ffff_ffff_ffff,
        0x7ff0_0000_0000_0000,
        0x7ff8_1234_5678_9abc,
        0x7ff0_0000_0000_0001,
    ];
    let mut cases = Vec::new();
    for a in special {
        for b in special {
            for sa in [0, 1 << 63] {
                for sb in [0, 1 << 63] {
                    cases.push((f64::from_bits(a | sa), f64::from_bits(b | sb)));
                }
            }
        }
    }
    let mut seed = 0xd63d_e6a9_3372_4901;
    for _ in 0..20_000 {
        let a = random(&mut seed);
        let b = random(&mut seed);
        cases.push((f64::from_bits(a), f64::from_bits(b)));
        // Nearby magnitudes, opposite signs and close exponents exercise
        // cancellation and rounding instead of mostly swallowed operands.
        let near = (a & 0xfff0_0000_0000_0000) | (b & 0x000f_ffff_ffff_ffff);
        cases.push((f64::from_bits(a), f64::from_bits(near ^ (1 << 63))));
    }
    cases
}
static GPU_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn native_gpu_binary64_matches_cpu_bits_for_basic_arithmetic() {
    let _guard = GPU_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = GpuContext::new() else {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "required binary64 GPU unavailable"
        );
        eprintln!("SKIP binary64: no GPU");
        return;
    };
    attach_diagnostics(&gpu.device);
    eprintln!("binary64 device: {:?}", gpu.backend_report());
    let cases = cases();
    let specs = op_kernel_specs();
    let mut kernels: [Option<Kernel>; 6] = std::array::from_fn(|_| None);
    let slot_for = |op: u32| -> usize {
        match op {
            0 | 1 | 2 | 5 => 0,
            3 => 1,
            4 => 2,
            6 => 3,
            7 => 4,
            _ => 5,
        }
    };
    const CHUNK: usize = 4_096;
    let usage = BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC;
    let chunk_bytes = (CHUNK * 8) as u64;
    let create_storage = |label: &'static str, size: u64| {
        gpu.device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let a_buf = create_storage("binary64 a", chunk_bytes);
    let b_buf = create_storage("binary64 b", chunk_bytes);
    let out_buf = create_storage("binary64 out", chunk_bytes);
    let params_buf = create_storage("binary64 params", 8);
    for (chunk_idx, chunk) in cases.chunks(CHUNK).enumerate() {
        let words = |right: bool| {
            chunk
                .iter()
                .flat_map(|&(a, b)| {
                    let bits = if right { b.to_bits() } else { a.to_bits() };
                    [bits as u32, (bits >> 32) as u32]
                })
                .collect::<Vec<_>>()
        };
        gpu.queue
            .write_buffer(&a_buf, 0, &u32_le_bytes(&words(false)));
        gpu.queue
            .write_buffer(&b_buf, 0, &u32_le_bytes(&words(true)));
        for op in 0..10 {
            let slot = slot_for(op);
            let kernel = kernels[slot].get_or_insert_with(|| {
                let (label, ref wgsl) = specs[slot];
                compile_op_kernel(&gpu.device, label, wgsl)
            });
            gpu.queue
                .write_buffer(&params_buf, 0, &u32_le_bytes(&[op, chunk.len() as u32]));
            kernel.dispatch(
                &gpu.device,
                &gpu.queue,
                &[&a_buf, &b_buf, &out_buf, &params_buf],
                chunk.len() as u32,
            );
            let result = try_read_u32(&gpu.device, &gpu.queue, &out_buf, chunk.len() * 2).unwrap();
            for (local_i, (&(a, b), actual)) in chunk
                .iter()
                .zip(result.as_chunks::<2>().0.iter())
                .enumerate()
            {
                let i = chunk_idx * CHUNK + local_i;
                if op == 5 {
                    assert_eq!(
                        *actual,
                        [u32::from(a == b), u32::from(a < b)],
                        "comparison {i} {a:?} {b:?}"
                    );
                    continue;
                }
                let expected = match op {
                    0 => a + b,
                    1 => a - b,
                    2 => a * b,
                    3 => a / b,
                    4 => a.sqrt(),
                    6 => a.exp(),
                    7 => a.ln(),
                    8 => a.sin(),
                    9 => a.cos(),
                    _ => unreachable!(),
                };
                let bits = u64::from(actual[0]) | (u64::from(actual[1]) << 32);
                if expected.is_nan() {
                    assert!(
                        f64::from_bits(bits).is_nan(),
                        "op={op} case={i} expected NaN"
                    );
                } else if op >= 6 && expected.is_finite() {
                    let got = f64::from_bits(bits);
                    let tolerance = if op >= 8 {
                        4e-15
                    } else {
                        8e-15 * expected.abs() + f64::from_bits(4)
                    };
                    assert!(
                        got.is_finite() && (got - expected).abs() <= tolerance,
                        "op={op} case={i} a={a:?} got={got:?} expected={expected:?}"
                    );
                } else {
                    assert_eq!(
                        bits,
                        expected.to_bits(),
                        "op={op} case={i} a={a:?} b={b:?} got={:?} want={expected:?}",
                        f64::from_bits(bits)
                    );
                }
            }
        }
    }
    for op in 0..10 {
        eprintln!(
            "binary64 op={op}: {} {} comparisons passed",
            cases.len(),
            if op < 6 { "exact" } else { "tolerance" }
        );
    }
}

#[test]
fn native_gpu_software_binary64_tensor_contract() {
    let _guard = GPU_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Some(gpu) = GpuContext::new() else {
        assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
        eprintln!("SKIP binary64 tensors: no GPU");
        return;
    };
    attach_diagnostics(&gpu.device);
    let runtime = compute_core::ComputeRuntime::new(&gpu).unwrap();
    tensor_core::conformance::check_f64_backend(&runtime).unwrap();
}
