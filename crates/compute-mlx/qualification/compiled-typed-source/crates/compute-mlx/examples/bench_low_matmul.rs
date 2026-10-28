//! Compare fresh MLX low-input matmul graphs with identical f32 result readbacks.
//! Uploads and first-use compilation are outside timing. Each sample includes
//! graph construction, allocations, evaluation, full readback and synchronization.
use compute_mlx::{MlxBackend, MlxLowTensor};
use half::{bf16, f16};
use std::time::{Duration, Instant};
use tensor_core::{HasShape, Layout, LowDtype, MatmulPrecision, Shape, TensorLowBackend};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SAMPLES: usize = 31;
const WARMUP: Duration = Duration::from_millis(200);
const PATHS: [&str; 2] = ["cast_both_then_f32_matmul", "direct_low_to_f32_matmul"];

struct InputSpec {
    physical: &'static [usize],
    permutation: Option<&'static [usize]>,
    broadcast: Option<&'static [usize]>,
}
impl InputSpec {
    const fn dense(physical: &'static [usize]) -> Self {
        Self {
            physical,
            permutation: None,
            broadcast: None,
        }
    }
}
struct Case {
    name: &'static str,
    left: InputSpec,
    right: InputSpec,
    output: &'static [usize],
    // Explicit oracle geometry, independent of the backend's matmul planner.
    batches: usize,
    m: usize,
    k: usize,
    n: usize,
}
struct Input {
    tensor: MlxLowTensor,
    logical: Vec<f64>,
    physical_elements: usize,
}
fn encode(dtype: LowDtype, x: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(x).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(x).to_bits(),
    }
}
fn input(backend: &MlxBackend, dtype: LowDtype, spec: &InputSpec, seed: usize) -> Result<Input> {
    let shape = Shape::new(spec.physical.to_vec())?;
    let values: Vec<f32> = (0..shape.numel())
        .map(|i| (((i * 17 + (i / 7) * 3 + seed) % 33) as i32 - 16) as f32 / 16.)
        .collect();
    let bits: Vec<u16> = values.iter().map(|&x| encode(dtype, x)).collect();
    let mut tensor = backend.upload_low(dtype, shape.clone(), &bits)?;
    let mut layout = Layout::contiguous(shape)?;
    if let Some(axes) = spec.permutation {
        tensor = backend.permute_low(&tensor, axes)?;
        layout = layout.permute(axes)?;
    }
    if let Some(dims) = spec.broadcast {
        let shape = Shape::new(dims.to_vec())?;
        tensor = backend.broadcast_low(&tensor, shape.clone())?;
        layout = layout.broadcast_to(shape)?;
    }
    let logical = (0..layout.shape().numel())
        .map(|i| Ok(f64::from(values[layout.element_offset(i)?])))
        .collect::<Result<Vec<_>>>()?;
    // Force evaluation of resident low inputs/views before either timing path.
    // Never retain an evaluated *result* and use it as the next timed graph.
    let actual = backend.read_low_bits(&tensor)?;
    assert_eq!(actual.len(), logical.len());
    for (i, (&actual, &expected)) in actual.iter().zip(&logical).enumerate() {
        assert_eq!(actual, encode(dtype, expected as f32), "input element {i}");
    }
    backend.synchronize()?;
    Ok(Input {
        tensor,
        logical,
        physical_elements: values.len(),
    })
}
fn oracle(case: &Case, left: &[f64], right: &[f64]) -> Vec<f64> {
    assert_eq!(left.len(), case.batches * case.m * case.k);
    assert_eq!(right.len(), case.batches * case.k * case.n);
    let mut expected = Vec::with_capacity(case.batches * case.m * case.n);
    for batch in 0..case.batches {
        for row in 0..case.m {
            for col in 0..case.n {
                let mut sum = 0f64;
                for inner in 0..case.k {
                    sum += left[(batch * case.m + row) * case.k + inner]
                        * right[(batch * case.k + inner) * case.n + col];
                }
                expected.push(sum);
            }
        }
    }
    expected
}
fn execute(
    backend: &MlxBackend,
    case: &Case,
    inputs: [&Input; 2],
    expected: &[f64],
    path: usize,
) -> Result<f64> {
    let start = Instant::now();
    // Construct a fresh operation graph EVERY replay. MLX eval caches array
    // results; timing eval repeatedly on one result would measure cached work.
    let output = if path == 0 {
        let left = backend.cast_to_f32(&inputs[0].tensor)?;
        let right = backend.cast_to_f32(&inputs[1].tensor)?;
        backend.matmul(&left, &right, MatmulPrecision::F32)?
    } else {
        backend.matmul_low_f32(&inputs[0].tensor, &inputs[1].tensor)?
    };
    let actual = backend.read_f32(&output)?;
    backend.synchronize()?;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
    // All validation is after the timer, for every warmup and measured result.
    assert_eq!(output.shape().dims(), case.output, "{} shape", case.name);
    assert_eq!(actual.len(), expected.len(), "{} length", case.name);
    for (i, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(
            f64::from(actual),
            expected,
            "{} {} element {i}",
            case.name,
            PATHS[path]
        );
    }
    Ok(elapsed_ms)
}
fn report(case: &str, dtype: LowDtype, path: usize, values: &[f64]) -> (f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let p90 = sorted[(sorted.len() * 9).div_ceil(10) - 1];
    println!(
        "case={case} dtype={dtype:?} path={} host_median_ms={median:.6} host_p90_ms={p90:.6} samples_ms={values:?}",
        PATHS[path]
    );
    (median, p90)
}
fn main() -> Result<()> {
    let backend = MlxBackend::new_gpu()?;
    println!(
        "runtime=MLX{} backend=Metal native_low_storage=true",
        backend.version()
    );
    println!(
        "method=wall_only fresh_graph_every_replay=true resident_low_inputs=true f32_output=true samples={SAMPLES} rotated_order=true warmup_ms={} includes=graph,allocation,evaluation,full_readback,synchronize excludes=upload,first_compile,oracle,validation,result_destruction",
        WARMUP.as_millis()
    );
    println!("oracle=independent_f64 exact_dyadic_inputs=integer/16 exact_f32_comparison=true");
    for case in [
        Case {
            name: "vector_dot",
            left: InputSpec::dense(&[257]),
            right: InputSpec::dense(&[257]),
            output: &[],
            batches: 1,
            m: 1,
            k: 257,
            n: 1,
        },
        Case {
            name: "decode_vector",
            left: InputSpec::dense(&[1025]),
            right: InputSpec::dense(&[1025, 129]),
            output: &[129],
            batches: 1,
            m: 1,
            k: 1025,
            n: 129,
        },
        Case {
            name: "matrix_vector",
            left: InputSpec::dense(&[129, 1025]),
            right: InputSpec::dense(&[1025]),
            output: &[129],
            batches: 1,
            m: 129,
            k: 1025,
            n: 1,
        },
        Case {
            name: "odd_gemm",
            left: InputSpec::dense(&[37, 65]),
            right: InputSpec::dense(&[65, 51]),
            output: &[37, 51],
            batches: 1,
            m: 37,
            k: 65,
            n: 51,
        },
        Case {
            name: "moderate_gemm",
            left: InputSpec::dense(&[128, 257]),
            right: InputSpec::dense(&[257, 192]),
            output: &[128, 192],
            batches: 1,
            m: 128,
            k: 257,
            n: 192,
        },
        Case {
            name: "batch_transpose_broadcast",
            left: InputSpec {
                physical: &[2, 65, 17],
                permutation: Some(&[0, 2, 1]),
                broadcast: None,
            },
            right: InputSpec {
                physical: &[1, 21, 65],
                permutation: Some(&[0, 2, 1]),
                broadcast: Some(&[2, 65, 21]),
            },
            output: &[2, 17, 21],
            batches: 2,
            m: 17,
            k: 65,
            n: 21,
        },
    ] {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            assert!(backend.low_precision_support(dtype).matmul_f32);
            let left = input(&backend, dtype, &case.left, 3)?;
            let right = input(&backend, dtype, &case.right, 19)?;
            let expected = oracle(&case, &left.logical, &right.logical);
            let physical_input_bytes = 2 * (left.physical_elements + right.physical_elements);
            let logical_removed_bytes = 4 * (left.logical.len() + right.logical.len());
            println!(
                "case={} dtype={dtype:?} a={:?} b={:?} output={:?} physical_low_input_bytes={physical_input_bytes} logical_removed_f32_operand_bytes={logical_removed_bytes} output_bytes={}",
                case.name,
                left.tensor.shape().dims(),
                right.tensor.shape().dims(),
                case.output,
                expected.len() * 4
            );
            for path in 0..2 {
                execute(&backend, &case, [&left, &right], &expected, path)?;
            }
            let warm = Instant::now();
            let mut round = 0usize;
            while warm.elapsed() < WARMUP {
                for offset in 0..2 {
                    execute(
                        &backend,
                        &case,
                        [&left, &right],
                        &expected,
                        (round + offset) % 2,
                    )?;
                }
                round += 1;
            }
            let mut samples = [Vec::with_capacity(SAMPLES), Vec::with_capacity(SAMPLES)];
            for iteration in 0..SAMPLES {
                for offset in 0..2 {
                    let path = (iteration + offset) % 2;
                    samples[path].push(execute(&backend, &case, [&left, &right], &expected, path)?);
                }
            }
            let baseline = report(case.name, dtype, 0, &samples[0]);
            let direct = report(case.name, dtype, 1, &samples[1]);
            println!(
                "case={} dtype={dtype:?} baseline_over_direct_host_median={:.6} validated_measured_outputs={}",
                case.name,
                baseline.0 / direct.0,
                2 * SAMPLES
            );
        }
    }
    println!("complete cases=12 paths=2 measured_outputs=744 validation_failures=0");
    Ok(())
}
