//! Eager versus compiled replay from changed resident f32 inputs.
//! Every sample builds fresh outputs. Timing includes graph invocation, evaluation,
//! both full result readbacks and synchronization; uploads and initial compilation
//! are outside timing. Run only on an otherwise idle GPU.
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxTensor};
use std::time::{Duration, Instant};
use tensor_core::{BinaryOp, HasShape, Shape, UnaryOp};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SAMPLES: usize = 31;
const WARMUP: Duration = Duration::from_millis(200);
const PATHS: [&str; 2] = ["eager", "compiled"];

struct Case {
    name: &'static str,
    dims: &'static [usize],
    scalar_weight: bool,
    alternate_transpose: bool,
}
struct Inputs {
    x: MlxTensor,
    w: MlxTensor,
    b: MlxTensor,
    expected_y: Vec<f64>,
    expected_sum: f64,
    layout: &'static str,
}
fn inputs(backend: &MlxBackend, case: &Case, version: usize) -> Result<Inputs> {
    let shape = Shape::new(case.dims.to_vec())?;
    assert!(shape.numel() <= 262_144);
    let transposed = case.alternate_transpose && version % 2 == 1;
    let physical: Vec<f32> = (0..shape.numel())
        .map(|i| (((i * 7 + i / 5 + version * 3) % 5) as i32 - 2) as f32 / 4.)
        .collect();
    let x = if transposed {
        assert_eq!(case.dims.len(), 2);
        let tensor =
            backend.upload_f32(Shape::new(vec![case.dims[1], case.dims[0]])?, &physical)?;
        backend.permute(&tensor, &[1, 0])?
    } else {
        backend.upload_f32(shape.clone(), &physical)?
    };
    let weight_dims = if case.scalar_weight {
        vec![]
    } else {
        vec![*case.dims.last().unwrap()]
    };
    let weight_shape = Shape::new(weight_dims)?;
    let weights: Vec<f32> = (0..weight_shape.numel())
        .map(|i| {
            if (i + version).is_multiple_of(2) {
                0.5
            } else {
                1.0
            }
        })
        .collect();
    let w = backend.upload_f32(weight_shape, &weights)?;
    let b = backend.upload_f32(Shape::new(vec![])?, &[0.25])?;
    // Evaluate every input/view outside timing. These handles are reused, while
    // each eager/compiled replay receives the current version and new outputs.
    for input in [&x, &w, &b] {
        backend.eval(input)?;
    }
    backend.synchronize()?;
    let expected_y: Vec<f64> = (0..shape.numel())
        .map(|flat| {
            let address = if transposed {
                let rows = case.dims[0];
                let cols = case.dims[1];
                (flat % cols) * rows + flat / cols
            } else {
                flat
            };
            let shifted =
                f64::from(physical[address]) * f64::from(weights[flat % weights.len()]) + 0.25;
            shifted * shifted
        })
        .collect();
    let expected_sum = expected_y.iter().sum();
    Ok(Inputs {
        x,
        w,
        b,
        expected_y,
        expected_sum,
        layout: if transposed {
            "transposed"
        } else {
            "contiguous"
        },
    })
}
fn compile(backend: &MlxBackend, input: &Inputs, axes: &[usize]) -> Result<MlxCompiledProgram> {
    let mut graph = backend.program();
    let x = graph.input(input.x.shape().clone())?;
    let w = graph.input(input.w.shape().clone())?;
    let b = graph.input(input.b.shape().clone())?;
    let product = graph.binary(x, w, BinaryOp::Multiply)?;
    let shifted = graph.binary(product, b, BinaryOp::Add)?;
    let y = graph.unary(shifted, UnaryOp::Square)?;
    let total = graph.sum_axes(y, axes, false)?;
    let program = graph.compile(&[y, total])?;
    assert_eq!(program.trace_count(), 0);
    Ok(program)
}
fn execute(
    backend: &MlxBackend,
    program: &MlxCompiledProgram,
    case: &Case,
    input: &Inputs,
    axes: &[usize],
    path: usize,
) -> Result<f64> {
    let start = Instant::now();
    // Never reevaluate the same output as a timing sample: eager builds new
    // operations and compiled binds current input handles to fresh result arrays.
    let outputs = if path == 0 {
        let product = backend.binary(BinaryOp::Multiply, &input.x, &input.w)?;
        let shifted = backend.binary(BinaryOp::Add, &product, &input.b)?;
        let y = backend.unary(UnaryOp::Square, &shifted)?;
        let total = backend.sum_axes(&y, axes, false)?;
        vec![y, total]
    } else {
        program.run(&[&input.x, &input.w, &input.b])?
    };
    let y = backend.read_f32(&outputs[0])?;
    let total = backend.read_f32(&outputs[1])?;
    backend.synchronize()?;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.;
    // Exact reference/shape/trace checks are outside every timing interval.
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].shape().dims(), case.dims);
    assert!(outputs[1].shape().dims().is_empty());
    assert_eq!(y.len(), input.expected_y.len());
    assert_eq!(total.len(), 1);
    for (i, (&actual, &expected)) in y.iter().zip(&input.expected_y).enumerate() {
        assert_eq!(
            f64::from(actual),
            expected,
            "{} {} element {i}",
            case.name,
            PATHS[path]
        );
    }
    assert_eq!(
        f64::from(total[0]),
        input.expected_sum,
        "{} {} total",
        case.name,
        PATHS[path]
    );
    if path == 1 {
        assert_eq!(program.trace_count(), 1, "{} retraced", case.name);
    }
    Ok(elapsed_ms)
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
        "method=wall_only outputs=square(x*w+b),sum(y) resident_input_versions=2 fresh_outputs_every_replay=true samples={SAMPLES} rotated_order=true input_version_sequence=0,0,1,1 warmup_ms={} includes=graph_invocation,allocation,evaluation,two_full_readbacks,synchronize excludes=upload,initial_compile,oracle,validation,trace_check,result_destruction",
        WARMUP.as_millis()
    );
    println!(
        "oracle=independent_f64 exact_f32_comparison=true x=integer[-2,2]/4 w=0.5_or_1 b=0.25 max_elements=262144 partial_sum_numerator_bound=9437184 denominator=64"
    );
    for case in [
        Case {
            name: "small",
            dims: &[257],
            scalar_weight: false,
            alternate_transpose: false,
        },
        Case {
            name: "large",
            dims: &[512, 512],
            scalar_weight: true,
            alternate_transpose: false,
        },
        Case {
            name: "changing_layout_broadcast",
            dims: &[513, 257],
            scalar_weight: false,
            alternate_transpose: true,
        },
    ] {
        let resident = [inputs(&backend, &case, 0)?, inputs(&backend, &case, 1)?];
        // The selected inputs produce distinct results. A stale/captured input
        // graph cannot satisfy both independently prepared references.
        assert_ne!(resident[0].expected_y, resident[1].expected_y);
        let axes: Vec<usize> = (0..case.dims.len()).collect();
        let program = compile(&backend, &resident[0], &axes)?;
        println!(
            "case={} shape={:?} w_shape={:?} input_layouts={:?} output_bytes={} initial_trace_count={}",
            case.name,
            case.dims,
            resident[0].w.shape().dims(),
            [resident[0].layout, resident[1].layout],
            (resident[0].expected_y.len() + 1) * 4,
            program.trace_count()
        );
        // First compilation and both value/layout versions are exercised before
        // the held warmup or any reported measurement.
        for input in &resident {
            for path in 0..2 {
                execute(&backend, &program, &case, input, &axes, path)?;
            }
        }
        assert_eq!(program.trace_count(), 1);
        let warm = Instant::now();
        let mut round = 0usize;
        while warm.elapsed() < WARMUP {
            for offset in 0..2 {
                execute(
                    &backend,
                    &program,
                    &case,
                    &resident[(round / 2) % 2],
                    &axes,
                    (round + offset) % 2,
                )?;
            }
            round += 1;
        }
        // Cycle input versions in pairs while reversing path order each round,
        // so neither layout is tied to always running eager or compiled first.
        let mut samples = [Vec::with_capacity(SAMPLES), Vec::with_capacity(SAMPLES)];
        for iteration in 0..SAMPLES {
            for offset in 0..2 {
                let path = (iteration + offset) % 2;
                samples[path].push(execute(
                    &backend,
                    &program,
                    &case,
                    &resident[(iteration / 2) % 2],
                    &axes,
                    path,
                )?);
            }
        }
        let eager = report(case.name, 0, &samples[0]);
        let compiled = report(case.name, 1, &samples[1]);
        assert_eq!(program.trace_count(), 1);
        println!(
            "case={} eager_over_compiled_host_median={:.6} final_trace_count={} validated_measured_invocations={} validated_measured_output_buffers={}",
            case.name,
            eager / compiled,
            program.trace_count(),
            2 * SAMPLES,
            4 * SAMPLES
        );
    }
    println!(
        "complete cases=3 paths=2 measured_invocations=186 measured_output_buffers=372 validation_failures=0 each_final_trace_count=1"
    );
    Ok(())
}
