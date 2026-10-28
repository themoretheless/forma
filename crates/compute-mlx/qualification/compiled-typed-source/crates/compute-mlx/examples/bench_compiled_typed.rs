//! Eager versus compiled typed graphs, with changed resident inputs every replay.
//! Wall timing includes fresh result construction, evaluation, both full reads,
//! and synchronization. Upload, first compilation, validation and drop are excluded.
use compute_mlx::{
    MlxBackend, MlxCompiledProgram, MlxDtype, MlxLowTensor, MlxProgramInput, MlxProgramOutput,
    MlxTensor,
};
use half::{bf16, f16};
use std::time::{Duration, Instant};
use tensor_core::{
    BinaryOp, CompareOp, HasLowDtype, HasShape, LowDtype, ReduceOp, Shape, TensorIndexBackend,
    TensorLowBackend, TensorLowOpsBackend, TensorReduceBackend, UnaryOp,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SAMPLES: usize = 31;
const WARMUP: Duration = Duration::from_millis(200);
const PATHS: [&str; 2] = ["eager", "compiled"];

#[derive(Clone, Copy)]
enum Case {
    LowArithmetic(LowDtype),
    LowMatmul(LowDtype),
    Unsigned(usize),
}
impl Case {
    fn name(self) -> String {
        let dtype = |d| match d {
            LowDtype::F16 => "f16",
            LowDtype::Bf16 => "bf16",
        };
        match self {
            Self::LowArithmetic(d) => format!("low_arithmetic_{}", dtype(d)),
            Self::LowMatmul(d) => format!("low_matmul_{}", dtype(d)),
            Self::Unsigned(3) => "u32_select_sum_small".into(),
            Self::Unsigned(_) => "u32_select_sum_large".into(),
        }
    }
}
enum Input {
    Tensor(MlxTensor),
    Low(MlxLowTensor),
}
impl Input {
    fn tensor(&self) -> &MlxTensor {
        match self {
            Self::Tensor(t) => t,
            Self::Low(_) => panic!("expected native tensor"),
        }
    }
    fn low(&self) -> &MlxLowTensor {
        match self {
            Self::Low(t) => t,
            Self::Tensor(_) => panic!("expected low tensor"),
        }
    }
    fn as_argument(&self) -> MlxProgramInput<'_> {
        match self {
            Self::Tensor(t) => MlxProgramInput::Tensor(t),
            Self::Low(t) => MlxProgramInput::Low(t),
        }
    }
}
#[derive(Debug, PartialEq)]
enum Expected {
    F32(Vec<f64>),
    U32(Vec<u32>),
    Low(LowDtype, Vec<u16>),
}
struct Reference {
    shape: Vec<usize>,
    values: Expected,
}
struct Resident {
    inputs: Vec<Input>,
    expected: Vec<Reference>,
    layout: &'static str,
}
enum ReadValues {
    F32(Vec<f32>),
    U32(Vec<u32>),
    Low(Vec<u16>),
}
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}
fn low_upload(
    backend: &MlxBackend,
    dtype: LowDtype,
    dims: &[usize],
    values: &[f32],
) -> Result<MlxLowTensor> {
    let bits: Vec<_> = values.iter().map(|&x| encode(dtype, x)).collect();
    Ok(backend.upload_low(dtype, Shape::new(dims.to_vec())?, &bits)?)
}
fn low_arithmetic(backend: &MlxBackend, dtype: LowDtype, version: usize) -> Result<Resident> {
    let (rows, columns) = (513, 257);
    let transposed = version == 1;
    let physical: Vec<f32> = (0..rows * columns)
        .map(|i| (((i * 7 + i / 5 + version * 3) % 5) as i32 - 2) as f32 / 4.)
        .collect();
    let x = if transposed {
        let raw = low_upload(backend, dtype, &[columns, rows], &physical)?;
        backend.permute_low(&raw, &[1, 0])?
    } else {
        low_upload(backend, dtype, &[rows, columns], &physical)?
    };
    let weights: Vec<f32> = (0..columns)
        .map(|i| {
            if (i + version).is_multiple_of(2) {
                0.5
            } else {
                1.
            }
        })
        .collect();
    let w = low_upload(backend, dtype, &[columns], &weights)?;
    let b = low_upload(backend, dtype, &[], &[0.25])?;
    let values: Vec<f64> = (0..rows * columns)
        .map(|i| {
            let address = if transposed {
                (i % columns) * rows + i / columns
            } else {
                i
            };
            let z = f64::from(physical[address]) * f64::from(weights[i % columns]) + 0.25;
            z * z
        })
        .collect();
    // Every intermediate and low result is exactly representable in both formats.
    // The largest absolute reduction numerator is 36*N < 2^24, denominator 64.
    assert!(36 * rows * columns < 1 << 24);
    let sum = values.iter().sum();
    let bits = values.iter().map(|&x| encode(dtype, x as f32)).collect();
    Ok(Resident {
        inputs: vec![Input::Low(x), Input::Low(w), Input::Low(b)],
        expected: vec![
            Reference {
                shape: vec![rows, columns],
                values: Expected::Low(dtype, bits),
            },
            Reference {
                shape: vec![],
                values: Expected::F32(vec![sum]),
            },
        ],
        layout: if transposed {
            "transposed"
        } else {
            "contiguous"
        },
    })
}
fn low_matrix_input(
    backend: &MlxBackend,
    dtype: LowDtype,
    batches: usize,
    rows: usize,
    columns: usize,
    version: usize,
    seed: usize,
) -> Result<(MlxLowTensor, Vec<f64>)> {
    let physical: Vec<f32> = (0..batches * rows * columns)
        .map(|i| (((i * 11 + i / 7 + version * 5 + seed) % 9) as i32 - 4) as f32 / 4.)
        .collect();
    let transposed = version == 1;
    let tensor = if transposed {
        let raw = low_upload(backend, dtype, &[batches, columns, rows], &physical)?;
        backend.permute_low(&raw, &[0, 2, 1])?
    } else {
        low_upload(backend, dtype, &[batches, rows, columns], &physical)?
    };
    let logical = (0..batches * rows * columns)
        .map(|i| {
            let batch = i / (rows * columns);
            let row = (i / columns) % rows;
            let col = i % columns;
            let address = batch * rows * columns
                + if transposed {
                    col * rows + row
                } else {
                    row * columns + col
                };
            f64::from(physical[address])
        })
        .collect();
    Ok((tensor, logical))
}
fn low_matmul(backend: &MlxBackend, dtype: LowDtype, version: usize) -> Result<Resident> {
    let (batches, m, k, n) = (2, 17, 65, 33);
    let (a, left) = low_matrix_input(backend, dtype, batches, m, k, version, 1)?;
    let (b, right) = low_matrix_input(backend, dtype, 1, k, n, version, 4)?;
    let mut values = Vec::with_capacity(batches * m * n);
    for batch in 0..batches {
        for row in 0..m {
            for col in 0..n {
                let mut value = 0f64;
                for inner in 0..k {
                    value += left[(batch * m + row) * k + inner] * right[inner * n + col];
                }
                values.push(value);
            }
        }
    }
    // Products have denominator16, absolute numerator<=16; this bounds every
    // matrix partial and the absolute sum over all products used by total.
    assert!(16 * k * batches * m * n < 1 << 24);
    let sum = values.iter().sum();
    Ok(Resident {
        inputs: vec![Input::Low(a), Input::Low(b)],
        expected: vec![
            Reference {
                shape: vec![batches, m, n],
                values: Expected::F32(values),
            },
            Reference {
                shape: vec![],
                values: Expected::F32(vec![sum]),
            },
        ],
        layout: if version == 1 {
            "both_transposed"
        } else {
            "contiguous"
        },
    })
}
fn unsigned(backend: &MlxBackend, rows: usize, version: usize) -> Result<Resident> {
    let columns = 257;
    let choices = [
        0,
        1,
        0x0100_0000,
        0x0100_0001,
        0x8000_0001,
        u32::MAX,
        17,
        0xffff_fffd,
    ];
    let physical: Vec<u32> = (0..rows * columns)
        .map(|i| choices[(i * 3 + i / 11 + version * 5) % choices.len()])
        .collect();
    let transposed = version == 1;
    let x = if transposed {
        let raw = backend.upload_u32(Shape::new(vec![columns, rows])?, &physical)?;
        backend.permute(&raw, &[1, 0])?
    } else {
        backend.upload_u32(Shape::new(vec![rows, columns])?, &physical)?
    };
    let threshold = 0x0100_0000 + version as u32;
    let otherwise = 0xffff_fffd - version as u32;
    let cutoff = backend.upload_u32(Shape::new(vec![])?, &[threshold])?;
    let no = backend.upload_u32(Shape::new(vec![])?, &[otherwise])?;
    let values: Vec<u32> = (0..rows * columns)
        .map(|i| {
            let address = if transposed {
                (i % columns) * rows + i / columns
            } else {
                i
            };
            let value = physical[address];
            if value > threshold { value } else { otherwise }
        })
        .collect();
    let sum = values.iter().fold(0u32, |sum, &x| sum.wrapping_add(x));
    Ok(Resident {
        inputs: vec![Input::Tensor(x), Input::Tensor(cutoff), Input::Tensor(no)],
        expected: vec![
            Reference {
                shape: vec![rows, columns],
                values: Expected::U32(values),
            },
            Reference {
                shape: vec![],
                values: Expected::U32(vec![sum]),
            },
        ],
        layout: if transposed {
            "transposed"
        } else {
            "contiguous"
        },
    })
}
fn resident(backend: &MlxBackend, case: Case, version: usize) -> Result<Resident> {
    let resident = match case {
        Case::LowArithmetic(dtype) => low_arithmetic(backend, dtype, version)?,
        Case::LowMatmul(dtype) => low_matmul(backend, dtype, version)?,
        Case::Unsigned(rows) => unsigned(backend, rows, version)?,
    };
    for input in &resident.inputs {
        match input {
            Input::Tensor(t) => backend.eval(t)?,
            Input::Low(t) => {
                backend.read_low_bits(t)?;
            }
        }
    }
    backend.synchronize()?;
    Ok(resident)
}
fn compile(backend: &MlxBackend, case: Case, input: &Resident) -> Result<MlxCompiledProgram> {
    let mut graph = backend.program();
    let mut placeholders = Vec::new();
    for tensor in &input.inputs {
        placeholders.push(match tensor {
            Input::Low(t) => graph.input_low(t.low_dtype(), t.shape().clone())?,
            Input::Tensor(t) => graph.input_u32(t.shape().clone())?,
        });
    }
    let x = placeholders[0];
    let (y, total) = match case {
        Case::LowArithmetic(_) => {
            let product = graph.binary_low(x, placeholders[1], BinaryOp::Multiply)?;
            let shifted = graph.binary_low(product, placeholders[2], BinaryOp::Add)?;
            let y = graph.unary_low(shifted, UnaryOp::Square)?;
            (y, graph.reduce_low_f32(y, ReduceOp::Sum, &[0, 1], false)?)
        }
        Case::LowMatmul(_) => {
            let y = graph.matmul_low_f32(x, placeholders[1])?;
            (y, graph.sum_axes(y, &[0, 1, 2], false)?)
        }
        Case::Unsigned(_) => {
            let mask = graph.compare_u32(x, placeholders[1], CompareOp::Greater)?;
            let y = graph.select_u32(mask, x, placeholders[2])?;
            (y, graph.reduce_u32(y, ReduceOp::Sum, &[0, 1], false)?)
        }
    };
    let program = graph.compile(&[y, total])?;
    assert_eq!(program.trace_count(), 0);
    Ok(program)
}
fn eager(backend: &MlxBackend, case: Case, inputs: &[Input]) -> Result<Vec<MlxProgramOutput>> {
    Ok(match case {
        Case::LowArithmetic(_) => {
            let product =
                backend.binary_low(BinaryOp::Multiply, inputs[0].low(), inputs[1].low())?;
            let shifted = backend.binary_low(BinaryOp::Add, &product, inputs[2].low())?;
            let y = backend.unary_low(UnaryOp::Square, &shifted)?;
            let total = backend.reduce_low_f32(ReduceOp::Sum, &y, &[0, 1], false)?;
            vec![MlxProgramOutput::Low(y), MlxProgramOutput::Tensor(total)]
        }
        Case::LowMatmul(_) => {
            let y = backend.matmul_low_f32(inputs[0].low(), inputs[1].low())?;
            let total = backend.sum_axes(&y, &[0, 1, 2], false)?;
            vec![MlxProgramOutput::Tensor(y), MlxProgramOutput::Tensor(total)]
        }
        Case::Unsigned(_) => {
            let mask =
                backend.compare_u32(CompareOp::Greater, inputs[0].tensor(), inputs[1].tensor())?;
            let y = backend.select_u32(&mask, inputs[0].tensor(), inputs[2].tensor())?;
            let total = backend.reduce_u32(ReduceOp::Sum, &y, &[0, 1], false)?;
            vec![MlxProgramOutput::Tensor(y), MlxProgramOutput::Tensor(total)]
        }
    })
}
fn validate(
    case: &str,
    path: usize,
    output: &MlxProgramOutput,
    actual: &ReadValues,
    expected: &Reference,
) {
    let shape = match output {
        MlxProgramOutput::Tensor(t) => t.shape(),
        MlxProgramOutput::Low(t) => t.shape(),
    };
    assert_eq!(shape.dims(), expected.shape, "{case} {} shape", PATHS[path]);
    match (actual, &expected.values) {
        (ReadValues::F32(values), Expected::F32(reference)) => {
            assert_eq!(values.len(), reference.len());
            for (i, (&actual, &expected)) in values.iter().zip(reference).enumerate() {
                assert_eq!(
                    f64::from(actual),
                    expected,
                    "{case} {} element {i}",
                    PATHS[path]
                );
            }
        }
        (ReadValues::U32(values), Expected::U32(reference)) => {
            assert_eq!(values, reference, "{case} {} u32", PATHS[path])
        }
        (ReadValues::Low(values), Expected::Low(dtype, reference)) => {
            let MlxProgramOutput::Low(tensor) = output else {
                panic!("expected low output")
            };
            assert_eq!(tensor.low_dtype(), *dtype);
            assert_eq!(values, reference, "{case} {} raw low bits", PATHS[path]);
        }
        _ => panic!("{case} {} output dtype mismatch", PATHS[path]),
    }
}
fn execute(
    backend: &MlxBackend,
    case: Case,
    name: &str,
    program: &MlxCompiledProgram,
    input: &Resident,
    path: usize,
) -> Result<f64> {
    let start = Instant::now();
    // Both branches create fresh lazy output handles on every execution.
    let outputs = if path == 0 {
        eager(backend, case, &input.inputs)?
    } else {
        let arguments: Vec<_> = input.inputs.iter().map(Input::as_argument).collect();
        program.run_typed(&arguments)?
    };
    let actual = outputs
        .iter()
        .map(|output| -> Result<ReadValues> {
            Ok(match output {
                MlxProgramOutput::Low(tensor) => ReadValues::Low(backend.read_low_bits(tensor)?),
                MlxProgramOutput::Tensor(tensor) => match tensor.dtype() {
                    MlxDtype::F32 => ReadValues::F32(backend.read_f32(tensor)?),
                    MlxDtype::U32 => ReadValues::U32(backend.read_u32(tensor)?),
                    _ => panic!("unexpected native tensor dtype"),
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    backend.synchronize()?;
    let elapsed = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(outputs.len(), 2);
    assert_eq!(actual.len(), input.expected.len());
    for ((output, values), expected) in outputs.iter().zip(&actual).zip(&input.expected) {
        validate(name, path, output, values, expected);
    }
    if path == 1 {
        assert_eq!(program.trace_count(), 1, "{name} retraced");
    }
    Ok(elapsed)
}
fn report(case: &str, path: usize, samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    let p90 = sorted[(sorted.len() * 9).div_ceil(10) - 1];
    println!(
        "case={case} path={} host_median_ms={median:.6} host_p90_ms={p90:.6} samples_ms={samples:?}",
        PATHS[path]
    );
    median
}
fn main() -> Result<()> {
    let backend = MlxBackend::new_gpu()?;
    assert!(backend.compile_available(), "MLX compile ABI unavailable");
    println!("runtime=MLX{} backend=Metal", backend.version());
    println!(
        "method=wall_only resident_input_versions=2 fresh_outputs_every_replay=true samples={SAMPLES} rotated_order=true input_version_sequence=0,0,1,1 warmup_ms={} includes=graph_invocation,allocation,evaluation,two_full_readbacks,synchronize excludes=upload,initial_compile,oracle,validation,trace_check,result_destruction",
        WARMUP.as_millis()
    );
    println!(
        "oracle=independent_f64_and_wrapping_u32 exact_output_checks=true low_arithmetic_elements=131841 numerator_bound=4746276 denominator=64 matmul_product_sum_numerator_bound=1166880 denominator=16"
    );
    for case in [
        Case::LowArithmetic(LowDtype::F16),
        Case::LowArithmetic(LowDtype::Bf16),
        Case::LowMatmul(LowDtype::F16),
        Case::LowMatmul(LowDtype::Bf16),
        Case::Unsigned(3),
        Case::Unsigned(513),
    ] {
        let name = case.name();
        let resident = [resident(&backend, case, 0)?, resident(&backend, case, 1)?];
        assert_ne!(
            resident[0].expected[0].values, resident[1].expected[0].values,
            "{name} stale input fixture"
        );
        let program = compile(&backend, case, &resident[0])?;
        println!(
            "case={name} output_shape={:?} input_layouts={:?} initial_trace_count={}",
            resident[0].expected[0].shape,
            [resident[0].layout, resident[1].layout],
            program.trace_count()
        );
        for input in &resident {
            for path in 0..2 {
                execute(&backend, case, &name, &program, input, path)?;
            }
        }
        let warm = Instant::now();
        let mut round = 0;
        while warm.elapsed() < WARMUP {
            for offset in 0..2 {
                execute(
                    &backend,
                    case,
                    &name,
                    &program,
                    &resident[(round / 2) % 2],
                    (round + offset) % 2,
                )?;
            }
            round += 1;
        }
        let mut samples = [Vec::with_capacity(SAMPLES), Vec::with_capacity(SAMPLES)];
        for iteration in 0..SAMPLES {
            for offset in 0..2 {
                let path = (iteration + offset) % 2;
                samples[path].push(execute(
                    &backend,
                    case,
                    &name,
                    &program,
                    &resident[(iteration / 2) % 2],
                    path,
                )?);
            }
        }
        let eager = report(&name, 0, &samples[0]);
        let compiled = report(&name, 1, &samples[1]);
        assert_eq!(program.trace_count(), 1);
        println!(
            "case={name} eager_over_compiled_host_median={:.6} final_trace_count={} validated_measured_invocations={} validated_measured_output_buffers={}",
            eager / compiled,
            program.trace_count(),
            2 * SAMPLES,
            4 * SAMPLES
        );
    }
    println!(
        "complete cases=6 paths=2 measured_invocations=372 measured_output_buffers=744 validation_failures=0 each_final_trace_count=1"
    );
    Ok(())
}
