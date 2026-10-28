use compute_core::{ComputeError, ComputeRuntime, MatrixView, gpu_compute::GpuContext};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    if context.is_none() {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "COMPUTE_REQUIRE_GPU set but no GPU adapter found"
        );
        eprintln!("matrix test skipped: no GPU adapter");
    }
    context
}

fn reference(a: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<(f64, f64)> {
    let mut out = vec![(0.0, 0.0); m * n];
    for row in 0..m {
        for col in 0..n {
            for inner in 0..k {
                let product = f64::from(a[row * k + inner]) * f64::from(b[inner * n + col]);
                out[row * n + col].0 += product;
                out[row * n + col].1 += product.abs();
            }
        }
    }
    out
}

fn check(actual: &[f32], expected: &[(f64, f64)]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&got, &(want, magnitude))) in actual.iter().zip(expected).enumerate() {
        assert!(
            (f64::from(got) - want).abs() <= 2e-6 * magnitude.max(1.0),
            "element {i}: {got} != {want}, absolute product sum {magnitude}"
        );
    }
}

#[test]
fn tiled_matrix_products_match_f64_reference_at_tile_boundaries() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    for (m, k, n) in [
        (1, 1, 1),
        (1, 31, 17),
        (17, 1, 31),
        (15, 15, 15),
        (16, 16, 16),
        (17, 17, 17),
        (31, 33, 19),
        (32, 32, 32),
        (65, 257, 63),
    ] {
        let av: Vec<f32> = (0..m * k)
            .map(|i| ((i * 17 % 73) as f32 - 36.0) / 17.0)
            .collect();
        let bv: Vec<f32> = (0..k * n)
            .map(|i| ((i * 7 % 41) as f32 - 20.0) / 13.0)
            .collect();
        let a = runtime.upload(&av).unwrap();
        let b = runtime.upload(&bv).unwrap();
        let storage = runtime.upload(&vec![1234.0f32; m * n + 7]).unwrap();
        let output = storage.prefix(m * n).unwrap();
        let mut program = runtime.program();
        program
            .matmul_into(
                MatrixView::new(&a, m, k).unwrap(),
                MatrixView::new(&b, k, n).unwrap(),
                MatrixView::new(&output, m, n).unwrap(),
            )
            .unwrap();
        let actual = program
            .submit_read(&storage)
            .unwrap()
            .wait(TIMEOUT)
            .unwrap();
        check(&actual[..m * n], &reference(&av, &bv, m, k, n));
        assert_eq!(&actual[m * n..], &[1234.0; 7]);
    }
}

#[test]
fn matrix_chains_reuse_inputs_and_need_only_final_scalar_readback() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let m = 17;
    let k = 23;
    let n = 19;
    let p = 7;
    let initial: Vec<f32> = (0..m * k).map(|i| (i % 5) as f32 - 2.0).collect();
    let bv: Vec<f32> = (0..k * n).map(|i| (i % 3) as f32 - 1.0).collect();
    let cv: Vec<f32> = (0..n * p).map(|i| (i % 7) as f32 - 3.0).collect();
    let a = runtime.upload(&initial).unwrap();
    let b = runtime.upload(&bv).unwrap();
    let c = runtime.upload(&cv).unwrap();
    let mut program = runtime.program();
    let first = program
        .matmul(
            MatrixView::new(&a, m, k).unwrap(),
            MatrixView::new(&b, k, n).unwrap(),
        )
        .unwrap();
    assert_eq!((first.rows(), first.columns()), (m, n));
    let second = program
        .matmul(first.view(), MatrixView::new(&c, n, p).unwrap())
        .unwrap();
    let mapped = program.affine(second.values(), 2.0, 1.0).unwrap();
    let total = program.sum(&mapped).unwrap();
    let mut pending = Vec::new();
    for offset in [0.0, 3.0, -4.0] {
        let av: Vec<f32> = initial.iter().map(|v| v + offset).collect();
        runtime.write(&a, 0, &av).unwrap();
        let first_cpu: Vec<f32> = reference(&av, &bv, m, k, n)
            .iter()
            .map(|x| x.0 as f32)
            .collect();
        let expected = reference(&first_cpu, &cv, m, n, p)
            .iter()
            .map(|x| x.0 * 2.0 + 1.0)
            .sum::<f64>();
        pending.push((program.submit_read(&total).unwrap(), expected));
    }
    for (read, expected) in pending.into_iter().rev() {
        assert_eq!(read.wait(TIMEOUT).unwrap(), [expected as f32]);
    }
}

#[test]
fn empty_dimensions_and_zero_inner_dimension_have_defined_reusable_results() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    for (m, k, n) in [
        (0, 5, 7),
        (3, 5, 0),
        (0, 0, 0),
        (17, 0, 19),
        (128, 0, 128),
        (512, 0, 128),
    ] {
        let a = runtime.zeros::<f32>(m * k).unwrap();
        let b = runtime.zeros::<f32>(k * n).unwrap();
        let output = runtime.zeros::<f32>(m * n).unwrap();
        let mut program = runtime.program();
        program
            .matmul_into(
                MatrixView::new(&a, m, k).unwrap(),
                MatrixView::new(&b, k, n).unwrap(),
                MatrixView::new(&output, m, n).unwrap(),
            )
            .unwrap();
        for value in [7.0, -3.0] {
            runtime.write(&output, 0, &vec![value; m * n]).unwrap();
            assert_eq!(
                program.submit_read(&output).unwrap().wait(TIMEOUT).unwrap(),
                vec![0.0; m * n]
            );
        }
    }
}

#[test]
fn matrix_workloads_stride_beyond_one_dispatch_grid() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let rows = 65535 * 256 + 1;
    let av: Vec<f32> = (0..rows).map(|i| (i % 17) as f32 - 8.0).collect();
    let a = runtime.upload(&av).unwrap();
    let b = runtime.upload(&[3.0f32]).unwrap();
    let mut program = runtime.program();
    let output = program
        .matmul(
            MatrixView::new(&a, rows, 1).unwrap(),
            MatrixView::new(&b, 1, 1).unwrap(),
        )
        .unwrap();
    let actual = program
        .submit_read(output.values())
        .unwrap()
        .wait(TIMEOUT)
        .unwrap();
    for (i, (&got, &input)) in actual.iter().zip(&av).enumerate() {
        assert_eq!(got, input * 3.0, "row {i}");
    }
}

#[test]
fn matrix_validation_rejects_shapes_overflow_foreign_arrays_and_aliases() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    let other = ComputeRuntime::new(&context).unwrap();
    let values = runtime.upload(&[1.0f32, 2.0, 3.0, 4.0]).unwrap();
    let out = runtime.zeros::<f32>(4).unwrap();
    let empty = runtime.zeros::<f32>(0).unwrap();
    let foreign = other.zeros::<f32>(4).unwrap();
    assert!(matches!(
        MatrixView::new(&values, 2, 3),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        MatrixView::new(&empty, usize::MAX, 2),
        Err(ComputeError::OutOfBounds)
    ));
    assert!(matches!(
        MatrixView::new(&empty, 0, usize::MAX),
        Err(ComputeError::OutOfBounds)
    ));
    let square = MatrixView::new(&values, 2, 2).unwrap();
    let column = MatrixView::new(&values, 4, 1).unwrap();
    let foreign_square = MatrixView::new(&foreign, 2, 2).unwrap();
    let mut program = runtime.program();
    assert!(matches!(
        program.matmul(square, column),
        Err(ComputeError::LengthMismatch { .. })
    ));
    assert!(matches!(
        program.matmul(square, foreign_square),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.matmul(foreign_square, square),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.matmul_into(square, square, foreign_square),
        Err(ComputeError::ForeignArray)
    ));
    assert!(matches!(
        program.matmul_into(square, square, square),
        Err(ComputeError::AliasedOutput)
    ));
    assert!(matches!(
        program.matmul_into(square, square, MatrixView::new(&out, 1, 4).unwrap()),
        Err(ComputeError::ShapeMismatch { .. })
    ));
    let huge_a = MatrixView::new(&empty, u32::MAX as usize, 0).unwrap();
    let huge_b = MatrixView::new(&empty, 0, u32::MAX as usize).unwrap();
    assert!(matches!(
        program.matmul(huge_a, huge_b),
        Err(ComputeError::TooLarge { .. }) | Err(ComputeError::OutOfBounds)
    ));
    let valid = program.matmul(square, square).unwrap();
    assert_eq!(
        program
            .submit_read(valid.values())
            .unwrap()
            .wait(TIMEOUT)
            .unwrap(),
        [7.0, 10.0, 15.0, 22.0]
    );
}

#[test]
fn selected_matrix_kernels_match_reference_and_restore_reused_outputs() {
    let Some(context) = context() else { return };
    let runtime = ComputeRuntime::new(&context).unwrap();
    for (m, k, n) in [
        (1, 512, 1),
        (8, 2048, 8),
        (32, 256, 32),
        (33, 256, 32),
        (128, 32, 128),
        (64, 128, 64),
        (513, 33, 129),
    ] {
        let a_values: Vec<f32> = (0..m * k)
            .map(|i| ((i * 17 % 73) as f32 - 36.0) / 17.0)
            .collect();
        let b_values: Vec<f32> = (0..k * n)
            .map(|i| ((i * 7 % 41) as f32 - 20.0) / 13.0)
            .collect();
        let a = runtime.upload(&a_values).unwrap();
        let b = runtime.upload(&b_values).unwrap();
        let storage = runtime.upload(&vec![1234.0; m * n + 7]).unwrap();
        let output = storage.prefix(m * n).unwrap();
        let mut program = runtime.program();
        program
            .matmul_into(
                MatrixView::new(&a, m, k).unwrap(),
                MatrixView::new(&b, k, n).unwrap(),
                MatrixView::new(&output, m, n).unwrap(),
            )
            .unwrap();
        for scale in [1.0, -0.25] {
            let changed: Vec<f32> = a_values.iter().map(|x| x * scale).collect();
            runtime.write(&a, 0, &changed).unwrap();
            let actual = program
                .submit_read(&storage)
                .unwrap()
                .wait(TIMEOUT)
                .unwrap();
            check(&actual[..m * n], &reference(&changed, &b_values, m, k, n));
            assert_eq!(&actual[m * n..], &[1234.0; 7]);
        }
    }
}
