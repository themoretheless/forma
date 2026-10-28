#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxError, MlxProgramOutput};
use half::{bf16, f16};
use tensor_core::{HasShape, LowDtype, ScanOptions, Shape, TensorLowBackend};

#[path = "compiled_scan_compaction/compaction.rs"]
mod compaction;
#[path = "compiled_scan_compaction/scans.rs"]
mod scans;
#[path = "compiled_scan_compaction/validation.rs"]
mod validation;

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu().and_then(|b| {
        if b.compile_available() {
            Ok(b)
        } else {
            Err(MlxError::CompileUnavailable)
        }
    }) {
        Ok(b) => Some(b),
        Err(error) => {
            eprintln!("SKIP compiled MLX scan/compaction unavailable: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
            None
        }
    }
}
fn traces(program: &MlxCompiledProgram, runs: usize) {
    assert_eq!(
        program.trace_count(),
        if std::env::var_os("MLX_DISABLE_COMPILE").is_some() {
            runs
        } else {
            usize::from(runs != 0)
        }
    );
}
fn options() -> [ScanOptions; 4] {
    [
        ScanOptions {
            inclusive: true,
            reverse: false,
        },
        ScanOptions {
            inclusive: false,
            reverse: false,
        },
        ScanOptions {
            inclusive: true,
            reverse: true,
        },
        ScanOptions {
            inclusive: false,
            reverse: true,
        },
    ]
}
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}
fn scan_reference<T: Copy>(
    input: &[T],
    dims: &[usize],
    axis: usize,
    opt: ScanOptions,
    zero: T,
    add: impl Fn(T, T) -> T,
) -> Vec<T> {
    let mut out = vec![zero; input.len()];
    let outer: usize = dims[..axis].iter().product();
    let inner: usize = dims[axis + 1..].iter().product();
    let count = dims[axis];
    for row in 0..outer {
        for suffix in 0..inner {
            let mut sum = zero;
            for j in 0..count {
                let column = if opt.reverse { count - 1 - j } else { j };
                let index = (row * count + column) * inner + suffix;
                if opt.inclusive {
                    sum = add(sum, input[index]);
                    out[index] = sum;
                } else {
                    out[index] = sum;
                    sum = add(sum, input[index]);
                }
            }
        }
    }
    out
}
fn floats(b: &MlxBackend, value: &MlxProgramOutput, expected: &[f32]) {
    let actual = b.read_f32(value.as_tensor().unwrap()).unwrap();
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, e, "prefix/value {i}: {a:?} != {e:?}");
    }
}
