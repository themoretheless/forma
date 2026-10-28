use compute_core::{BinaryOp, ComputeError, ComputeRuntime, UnaryOp, gpu_compute::GpuContext};
use std::time::{Duration, Instant};

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    if context.is_none() {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "COMPUTE_REQUIRE_GPU set but no GPU adapter found"
        );
        eprintln!("runtime test skipped: no GPU adapter");
    }
    context
}
const TIMEOUT: Duration = Duration::from_secs(10);

#[test]
fn typed_arrays_preserve_u32_bits_and_support_partial_updates() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let values = [0u32, 1, u32::MAX, 0x7fc00000, 0x80000000];
    let array = rt.upload(&values).unwrap();
    rt.write(&array, 1, &[77, 88]).unwrap();
    assert_eq!(
        rt.read(&array).unwrap().wait(TIMEOUT).unwrap(),
        [0, 77, 88, 0x7fc00000, 0x80000000]
    );
    let prefix = array.prefix(3).unwrap();
    assert_eq!(
        rt.read(&prefix).unwrap().wait(TIMEOUT).unwrap(),
        [0, 77, 88]
    );
    assert!(matches!(
        rt.write(&prefix, 3, &[1]),
        Err(ComputeError::OutOfBounds)
    ));
    assert!(matches!(
        rt.write(&array, usize::MAX, &[1]),
        Err(ComputeError::OutOfBounds)
    ));
    assert!(matches!(array.prefix(6), Err(ComputeError::OutOfBounds)));
    let zeros = rt.zeros::<f32>(17).unwrap();
    assert_eq!(
        rt.read(&zeros).unwrap().wait(TIMEOUT).unwrap(),
        vec![0.0; 17]
    );
}

#[test]
fn binary_operations_match_cpu_for_odd_length_arrays() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let av: Vec<f32> = (0..1027).map(|i| (i % 19) as f32 - 9.0).collect();
    let bv: Vec<f32> = (0..1027).map(|i| (i % 7 + 1) as f32).collect();
    let a = rt.upload(&av).unwrap();
    let b = rt.upload(&bv).unwrap();
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        let mut program = rt.program();
        let output = program.binary(op, &a, &b).unwrap();
        let actual = program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap();
        for ((actual, a), b) in actual.iter().zip(&av).zip(&bv) {
            let expected = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Subtract => a - b,
                BinaryOp::Multiply => a * b,
                BinaryOp::Divide => a / b,
                BinaryOp::Min => a.min(*b),
                BinaryOp::Max => a.max(*b),
            };
            assert!(
                (actual - expected).abs() <= 1e-6 * expected.abs().max(1.0),
                "{op:?}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn program_reuses_allocations_and_overlapping_readbacks_capture_each_submission() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let n = 4097;
    let input = rt.zeros::<f32>(n).unwrap();
    let workspace = rt.zeros::<f32>(n + 13).unwrap();
    rt.write(&workspace, n, &[99.0; 13]).unwrap();
    let intermediate = workspace.prefix(n).unwrap();
    let mut program = rt.program();
    program
        .affine_into(&input, 2.0, 1.0, &intermediate)
        .unwrap();
    let total = program.dot(&intermediate, &input).unwrap();
    let output = program.affine(&total, 0.5, -3.0).unwrap();
    let mut reads = Vec::new();
    for iteration in 0..4 {
        let values: Vec<f32> = (0..n).map(|i| ((i + iteration) % 7) as f32).collect();
        rt.write(&input, 0, &values).unwrap();
        let expected = values.iter().map(|x| (x * 2.0 + 1.0) * x).sum::<f32>() * 0.5 - 3.0;
        reads.push((program.submit_read(&output).unwrap(), expected));
    }
    // Await in reverse order; no ticket may observe a later submission's value.
    for (read, expected) in reads.into_iter().rev() {
        assert_eq!(read.wait(TIMEOUT).unwrap(), [expected]);
    }
    let all = rt.read(&workspace).unwrap().wait(TIMEOUT).unwrap();
    assert_eq!(&all[n..], &[99.0; 13]);
}

#[test]
fn errors_do_not_poison_a_program() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let other = ComputeRuntime::new(&context).unwrap();
    let a = rt.upload(&[1.0f32, 2.0]).unwrap();
    let b = rt.upload(&[3.0f32, 4.0, 5.0]).unwrap();
    let foreign = other.upload(&[1.0f32, 2.0]).unwrap();
    let mut program = rt.program();
    assert!(matches!(
        program.binary(BinaryOp::Add, &a, &b),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.affine_into(&a, 2.0, 0.0, &a.clone()),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.binary_into(BinaryOp::Add, &a, &a, &a),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.affine(&foreign, 1.0, 0.0),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(rt.read(&foreign), Err(ComputeError::ForeignArray)));
    assert!(matches!(
        rt.write(&foreign, 0, &[3.0]),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        rt.zeros::<f32>(usize::MAX),
        Err(ComputeError::TooLarge { .. })
    ));
    let output = program.binary(BinaryOp::Multiply, &a, &a).unwrap();
    assert_eq!(
        program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap(),
        [1.0, 4.0]
    );
}

#[test]
fn empty_arrays_and_singletons_have_defined_results() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    for values in [vec![], vec![7.0]] {
        let a = rt.upload(&values).unwrap();
        let mut program = rt.program();
        let mapped = program.affine(&a, 2.0, 1.0).unwrap();
        let total = program.sum(&mapped).unwrap();
        assert_eq!(
            program.submit_read(&total).unwrap().wait(TIMEOUT).unwrap(),
            [values.iter().map(|v| v * 2.0 + 1.0).sum::<f32>()]
        );
        let actual = rt.read(&a).unwrap().wait(TIMEOUT).unwrap();
        assert_eq!(actual, values);
    }
}

#[test]
fn nonblocking_read_and_abandoned_tickets() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let a = rt.upload(&[2.0f32, 3.0, 4.0]).unwrap();
    let mut program = rt.program();
    let output = program.dot(&a, &a).unwrap();
    drop(program.submit_read(&output).unwrap());
    let mut read = program.submit_read(&output).unwrap();
    let start = Instant::now();
    loop {
        if let Some(values) = read.try_read().unwrap() {
            assert_eq!(values, [29.0]);
            break;
        }
        assert!(
            start.elapsed() < TIMEOUT,
            "nonblocking read never completed"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        read.try_read(),
        Err(ComputeError::ReadbackConsumed)
    ));
}

#[test]
fn grid_strided_arrays_cover_more_than_one_dispatch_grid() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let n = 65535 * 256 + 17;
    let a = rt.zeros::<f32>(n).unwrap();
    rt.write(&a, n - 1, &[7.0]).unwrap();
    let mut program = rt.program();
    let scaled = program.affine(&a, 2.0, 0.0).unwrap();
    let product = program.binary(BinaryOp::Multiply, &scaled, &a).unwrap();
    let total = program.sum(&product).unwrap();
    assert_eq!(
        program.submit_read(&total).unwrap().wait(TIMEOUT).unwrap(),
        [98.0]
    );
}

#[test]
fn unary_math_matches_cpu() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let values: Vec<f32> = (1..=513).map(|i| i as f32 / 100.0).collect();
    let input = rt.upload(&values).unwrap();
    for op in [
        UnaryOp::Negate,
        UnaryOp::Abs,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Reciprocal,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ] {
        let mut program = rt.program();
        let output = program.unary(op, &input).unwrap();
        let actual = program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap();
        for (actual, x) in actual.iter().zip(&values) {
            let expected = match op {
                UnaryOp::Negate => -x,
                UnaryOp::Abs => x.abs(),
                UnaryOp::Square => x * x,
                UnaryOp::Sqrt => x.sqrt(),
                UnaryOp::Reciprocal => x.recip(),
                UnaryOp::Exp => x.exp(),
                UnaryOp::Log => x.ln(),
                UnaryOp::Sin => x.sin(),
                UnaryOp::Cos => x.cos(),
            };
            assert!(
                (actual - expected).abs() < expected.abs().max(1.0) * 2e-5,
                "{op:?}({x}) = {actual}, expected {expected}"
            );
        }
    }
}

#[test]
fn normalization_and_broadcasting_stay_on_gpu() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let input = rt.upload(&[3.0f32, -4.0]).unwrap();
    let mut program = rt.program();
    let squared_length = program.dot(&input, &input).unwrap();
    let length = program.unary(UnaryOp::Sqrt, &squared_length).unwrap();
    let normalized = program.binary(BinaryOp::Divide, &input, &length).unwrap();
    let reversed = program.binary(BinaryOp::Subtract, &length, &input).unwrap();
    let actual = program
        .submit_read(&normalized)
        .unwrap()
        .wait(TIMEOUT)
        .unwrap();
    assert!((actual[0] - 0.6).abs() < 1e-6 && (actual[1] + 0.8).abs() < 1e-6);
    assert_eq!(
        rt.read(&reversed).unwrap().wait(TIMEOUT).unwrap(),
        [2.0, 9.0]
    );
    let empty = rt.zeros::<f32>(0).unwrap();
    let empty_output = program.binary(BinaryOp::Add, &length, &empty).unwrap();
    assert!(
        program
            .submit_read(&empty_output)
            .unwrap()
            .wait(TIMEOUT)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        program.dot(&input, &length),
        Err(ComputeError::LengthMismatch { .. })
    ));
}
