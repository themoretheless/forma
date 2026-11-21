#![cfg(not(target_arch = "wasm32"))]
use compute_core::{
    Binding, Kernel,
    binary64::{ARITHMETIC_WGSL, TRANSCENDENTAL_WGSL},
    gpu_compute::GpuContext,
    storage_u32, try_read_u32,
};
const ENTRY: &str = r#"
@group(0) @binding(0) var<storage,read> input_a: array<D64>;
@group(0) @binding(1) var<storage,read> input_b: array<D64>;
@group(0) @binding(2) var<storage,read_write> output: array<D64>;
@group(0) @binding(3) var<storage,read> params: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i=id.x;
    if i>=arrayLength(&input_a) { return; }
    let a=input_a[i]; let b=input_b[i];
    switch params[0] {
        case 0u: { output[i]=d64_add(a,b); }
        case 1u: { output[i]=d64_sub(a,b); }
        case 2u: { output[i]=d64_mul(a,b); }
        case 3u: { output[i]=d64_div(a,b); }
        case 4u: { output[i]=d64_sqrt(a); }
        case 5u: { output[i]=D64(u32(d64_eq(a,b)),u32(d64_lt(a,b))); }
        case 6u: { output[i]=d64_exp(a); }
        case 7u: { output[i]=d64_log(a); }
        case 8u, 9u: { output[i]=d64_sincos(a,params[0]==9u); }
        default: { output[i]=a; }
    }
}
"#;
fn source() -> String {
    format!("{ARITHMETIC_WGSL}\n{TRANSCENDENTAL_WGSL}\n{ENTRY}")
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
#[test]
fn binary64_wgsl_validates_without_float_arithmetic() {
    let module = naga::front::wgsl::parse_str(&source()).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
    assert!(!ARITHMETIC_WGSL.contains("f32"));
    assert!(!ARITHMETIC_WGSL.contains("f16"));
    assert!(!TRANSCENDENTAL_WGSL.contains("f32"));
    assert!(!TRANSCENDENTAL_WGSL.contains("f16"));
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
#[test]
fn native_gpu_binary64_matches_cpu_bits_for_basic_arithmetic() {
    let Some(gpu) = GpuContext::new() else {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "required binary64 GPU unavailable"
        );
        eprintln!("SKIP binary64: no GPU");
        return;
    };
    eprintln!("binary64 device: {:?}", gpu.backend_report());
    let cases = cases();
    let words = |right: bool| {
        cases
            .iter()
            .flat_map(|&(a, b)| {
                let bits = if right { b.to_bits() } else { a.to_bits() };
                [bits as u32, (bits >> 32) as u32]
            })
            .collect::<Vec<_>>()
    };
    let a = storage_u32(&gpu.device, &gpu.queue, &words(false)).unwrap();
    let b = storage_u32(&gpu.device, &gpu.queue, &words(true)).unwrap();
    let out = storage_u32(&gpu.device, &gpu.queue, &vec![0; cases.len() * 2]).unwrap();
    let wrap_entry = |preamble: &str, body: &str| {
        format!(
            "{preamble}\n\
             @group(0) @binding(0) var<storage,read> input_a: array<D64>;\n\
             @group(0) @binding(1) var<storage,read> input_b: array<D64>;\n\
             @group(0) @binding(2) var<storage,read_write> output: array<D64>;\n\
             @group(0) @binding(3) var<storage,read> params: array<u32>;\n\
             @compute @workgroup_size(256)\n\
             fn main(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
                 let i = id.x;\n\
                 if i >= arrayLength(&input_a) {{ return; }}\n\
                 let a = input_a[i]; let b = input_b[i];\n\
                 {body}\n\
             }}"
        )
    };
    let arith = compile_op_kernel(
        &gpu.device,
        "integer binary64 arithmetic",
        &wrap_entry(
            ARITHMETIC_WGSL,
            "switch params[0] {\n\
                case 0u: { output[i] = d64_add(a, b); }\n\
                case 1u: { output[i] = d64_sub(a, b); }\n\
                case 2u: { output[i] = d64_mul(a, b); }\n\
                case 3u: { output[i] = d64_div(a, b); }\n\
                case 4u: { output[i] = d64_sqrt(a); }\n\
                case 5u: { output[i] = D64(u32(d64_eq(a, b)), u32(d64_lt(a, b))); }\n\
                default: { output[i] = a; }\n\
            }",
        ),
    );
    let transc = format!("{ARITHMETIC_WGSL}\n{TRANSCENDENTAL_WGSL}");
    let exp_kernel = compile_op_kernel(
        &gpu.device,
        "integer binary64 exp",
        &wrap_entry(&transc, "output[i] = d64_exp(a);"),
    );
    let log_kernel = compile_op_kernel(
        &gpu.device,
        "integer binary64 log",
        &wrap_entry(&transc, "output[i] = d64_log(a);"),
    );
    let sincos_kernel = compile_op_kernel(
        &gpu.device,
        "integer binary64 sincos",
        &wrap_entry(&transc, "output[i] = d64_sincos(a, params[0] == 9u);"),
    );
    for op in 0..10 {
        let params = storage_u32(&gpu.device, &gpu.queue, &[op]).unwrap();
        let kernel = match op {
            0..=5 => &arith,
            6 => &exp_kernel,
            7 => &log_kernel,
            _ => &sincos_kernel,
        };
        kernel.dispatch(
            &gpu.device,
            &gpu.queue,
            &[&a, &b, &out, &params],
            cases.len() as u32,
        );
        let result = try_read_u32(&gpu.device, &gpu.queue, &out, cases.len() * 2).unwrap();
        for (i, (&(a, b), actual)) in cases
            .iter()
            .zip(result.as_chunks::<2>().0.iter())
            .enumerate()
        {
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
        eprintln!(
            "binary64 op={op}: {} {} comparisons passed",
            cases.len(),
            if op < 6 { "exact" } else { "tolerance" }
        );
    }
}

#[test]
fn native_gpu_software_binary64_tensor_contract() {
    let Some(gpu) = GpuContext::new() else {
        assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
        eprintln!("SKIP binary64 tensors: no GPU");
        return;
    };
    let runtime = compute_core::ComputeRuntime::new(&gpu).unwrap();
    tensor_core::conformance::check_f64_backend(&runtime).unwrap();
}
