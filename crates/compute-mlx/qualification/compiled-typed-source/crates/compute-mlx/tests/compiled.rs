#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxCompiledProgram, MlxDtype, MlxError, MlxTensor};
use tensor_core::{BinaryOp, HasShape, Shape, UnaryOp};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

fn backend() -> Option<MlxBackend> {
    let available = MlxBackend::new_gpu().and_then(|b| {
        if b.compile_available() {
            Ok(b)
        } else {
            Err(MlxError::CompileUnavailable)
        }
    });
    match available {
        Ok(b) => Some(b),
        Err(error) => {
            eprintln!("SKIP MLX compiled programs unavailable: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
            None
        }
    }
}

fn traces(program: &MlxCompiledProgram, runs: usize) {
    // getenv presence disables native compilation, including a value of "0".
    // Tests never call MLX's process-global enable/disable functions.
    let expected = if std::env::var_os("MLX_DISABLE_COMPILE").is_some() {
        runs
    } else {
        usize::from(runs != 0)
    };
    assert_eq!(program.trace_count(), expected);
}

fn values(b: &MlxBackend, t: &MlxTensor, dims: &[usize], expected: &[f32]) {
    assert_eq!(t.shape(), &shape(dims));
    assert_eq!(t.dtype(), MlxDtype::F32);
    assert_eq!(b.read_f32(t).unwrap(), expected);
}

fn square_reference(x: &[f32], weights: &[f32], bias: f32) -> Vec<f32> {
    x.iter()
        .enumerate()
        .map(|(i, &x)| {
            let shifted = f64::from(x) * f64::from(weights[i % weights.len()]) + f64::from(bias);
            (shifted * shifted) as f32
        })
        .collect()
}

#[test]
fn changed_resident_inputs_replay_map_and_reductions_with_duplicate_outputs() {
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    let x = builder.input(shape(&[3, 5])).unwrap();
    let weight = builder.input(shape(&[5])).unwrap();
    let bias = builder.input(shape(&[])).unwrap();
    let multiplied = builder.binary(x, weight, BinaryOp::Multiply).unwrap();
    let shifted = builder.binary(multiplied, bias, BinaryOp::Add).unwrap();
    let squared = builder.unary(shifted, UnaryOp::Square).unwrap();
    let total = builder.sum_axes(squared, &[0, 1], false).unwrap();
    let rows = builder.sum_axes(squared, &[1], true).unwrap();
    let program = builder
        .compile(&[squared, total, squared, x, rows])
        .unwrap();
    traces(&program, 0);

    let initial: Vec<_> = (0..15).map(|i| i as f32 * 0.25 - 1.).collect();
    let weights = [-0.5, 0.25, 1.5, 2., -1.];
    let x0 = b.upload_f32(shape(&[3, 5]), &initial).unwrap();
    let w0 = b.upload_f32(shape(&[5]), &weights).unwrap();
    let bias0 = b.upload_f32(shape(&[]), &[-0.25]).unwrap();
    let first = program.run(&[&x0, &w0, &bias0]).unwrap();
    assert_eq!(first.len(), 5);
    traces(&program, 1);
    let expected0 = square_reference(&initial, &weights, -0.25);

    // New values are produced entirely on the GPU. None is an original-input
    // buffer mutation or a second eval of the already-created first result.
    let two = b.upload_f32(shape(&[]), &[2.]).unwrap();
    let x1 = b.binary(BinaryOp::Add, &x0, &two).unwrap();
    let w1 = b.unary(UnaryOp::Negate, &w0).unwrap();
    let bias1 = b.unary(UnaryOp::Negate, &bias0).unwrap();
    let next: Vec<_> = initial.iter().map(|x| x + 2.).collect();
    let next_weights: Vec<_> = weights.iter().map(|x| -x).collect();
    let expected1 = square_reference(&next, &next_weights, 0.25);
    assert_ne!(expected0, expected1);
    let second = program.run(&[&x1, &w1, &bias1]).unwrap();
    traces(&program, 2);
    // Evaluate the second graph first, then the old graph. Both must retain
    // their own input snapshots even while sharing the compiled function.
    for (outputs, input, expected) in [(&second, &next, &expected1), (&first, &initial, &expected0)]
    {
        values(&b, &outputs[0], &[3, 5], expected);
        values(&b, &outputs[2], &[3, 5], expected);
        values(&b, &outputs[3], &[3, 5], input);
        let total = expected.iter().map(|&x| f64::from(x)).sum::<f64>() as f32;
        values(&b, &outputs[1], &[], &[total]);
        let rows: Vec<_> = expected
            .chunks(5)
            .map(|row| row.iter().map(|&x| f64::from(x)).sum::<f64>() as f32)
            .collect();
        values(&b, &outputs[4], &[3, 1], &rows);
    }
    let third = program.run(&[&x0, &w0, &bias0]).unwrap();
    traces(&program, 3);
    values(&b, &third[0], &[3, 5], &expected0);
    values(&b, &second[0], &[3, 5], &expected1);
}

#[test]
fn same_shape_new_transpose_and_zero_strides_reuse_the_native_trace() {
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    let input = builder.input(shape(&[3, 5])).unwrap();
    let squared = builder.unary(input, UnaryOp::Square).unwrap();
    let columns = builder.sum_axes(input, &[0], false).unwrap();
    let identity = builder.sum_axes(input, &[], false).unwrap();
    let permuted = builder.permute(input, &[1, 0]).unwrap();
    let flattened = builder.reshape(permuted, shape(&[15])).unwrap();
    let program = builder
        .compile(&[squared, columns, identity, flattened])
        .unwrap();
    let dense_values: Vec<_> = (0..15).map(|i| i as f32 - 7.).collect();
    let dense = b.upload_f32(shape(&[3, 5]), &dense_values).unwrap();
    let physical_values: Vec<_> = (0..15).map(|i| i as f32 * 0.5 + 1.).collect();
    let physical = b.upload_f32(shape(&[5, 3]), &physical_values).unwrap();
    let transposed = b.permute(&physical, &[1, 0]).unwrap();
    let physical_reference = &physical_values;
    let transposed_values: Vec<_> = (0..3)
        .flat_map(|row| (0..5).map(move |column| physical_reference[column * 3 + row]))
        .collect();
    let repeated = [1., -2., 3., -4., 5.];
    let row = b.upload_f32(shape(&[1, 5]), &repeated).unwrap();
    let broadcast = b.broadcast_to(&row, shape(&[3, 5])).unwrap();
    let repeated_values = repeated.repeat(3);
    for (iteration, (input, expected)) in [
        (&dense, &dense_values),
        (&transposed, &transposed_values),
        (&broadcast, &repeated_values),
        (&dense, &dense_values),
    ]
    .into_iter()
    .enumerate()
    {
        let outputs = program.run(&[input]).unwrap();
        traces(&program, iteration + 1);
        let squares: Vec<_> = expected.iter().map(|x| x * x).collect();
        let sums: Vec<_> = (0..5)
            .map(|col| (0..3).map(|row| expected[row * 5 + col]).sum())
            .collect();
        values(&b, &outputs[0], &[3, 5], &squares);
        values(&b, &outputs[1], &[5], &sums);
        values(&b, &outputs[2], &[3, 5], expected);
        let transposed: Vec<_> = (0..5)
            .flat_map(|column| (0..3).map(move |row| expected[row * 5 + column]))
            .collect();
        values(&b, &outputs[3], &[15], &transposed);
    }
}

#[test]
fn shape_nodes_and_batched_matmul_follow_independent_coordinates() {
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    let left = builder.input(shape(&[2, 3])).unwrap();
    let right = builder.input(shape(&[4, 2])).unwrap();
    let a = builder.permute(left, &[1, 0]).unwrap();
    let a = builder.reshape(a, shape(&[1, 3, 2])).unwrap();
    let a = builder.broadcast_to(a, shape(&[2, 3, 2])).unwrap();
    let bval = builder.permute(right, &[1, 0]).unwrap();
    let product = builder.matmul(a, bval).unwrap();
    let rows = builder.sum_axes(product, &[2], false).unwrap();
    let flattened = builder.reshape(product, shape(&[24])).unwrap();
    let program = builder.compile(&[product, rows, flattened]).unwrap();
    let av = [1., 2., 3., 4., 5., 6.];
    let bv = [1., 0., 0., 1., 2., -1., -0.5, 3.];
    let left = b.upload_f32(shape(&[2, 3]), &av).unwrap();
    let right = b.upload_f32(shape(&[4, 2]), &bv).unwrap();
    let changed = b.unary(UnaryOp::Negate, &right).unwrap();
    let mut expected = Vec::new();
    for _batch in 0..2 {
        for row in 0..3 {
            for col in 0..4 {
                expected.push(
                    (0..2)
                        .map(|inner| {
                            f64::from(av[inner * 3 + row]) * f64::from(bv[col * 2 + inner])
                        })
                        .sum::<f64>() as f32,
                );
            }
        }
    }
    for (i, right) in [&right, &changed].into_iter().enumerate() {
        let outputs = program.run(&[&left, right]).unwrap();
        traces(&program, i + 1);
        let wanted: Vec<_> = expected
            .iter()
            .map(|&x| if i == 0 { x } else { -x })
            .collect();
        values(&b, &outputs[0], &[2, 3, 4], &wanted);
        values(&b, &outputs[2], &[24], &wanted);
        let row_sums: Vec<_> = wanted.chunks(4).map(|row| row.iter().sum()).collect();
        values(&b, &outputs[1], &[2, 3], &row_sums);
    }
}

#[test]
fn vector_matmul_and_empty_contractions_keep_shared_shape_rules() {
    let Some(b) = backend() else { return };
    for k in [0, 3] {
        let mut builder = b.program();
        let vector = builder.input(shape(&[k])).unwrap();
        let matrix = builder.input(shape(&[2, k])).unwrap();
        let columns = builder.permute(matrix, &[1, 0]).unwrap();
        let dot = builder.matmul(vector, vector).unwrap();
        let mv = builder.matmul(matrix, vector).unwrap();
        let vm = builder.matmul(vector, columns).unwrap();
        let program = builder.compile(&[dot, mv, vm]).unwrap();
        let x: Vec<_> = (0..k).map(|i| i as f32 - 1.).collect();
        let a: Vec<_> = (0..2 * k).map(|i| i as f32 + 1.).collect();
        let x = b.upload_f32(shape(&[k]), &x).unwrap();
        let a = b.upload_f32(shape(&[2, k]), &a).unwrap();
        let outputs = program.run(&[&x, &a]).unwrap();
        traces(&program, 1);
        values(&b, &outputs[0], &[], &[if k == 0 { 0. } else { 2. }]);
        let expected = if k == 0 { [0., 0.] } else { [2., 2.] };
        values(&b, &outputs[1], &[2], &expected);
        values(&b, &outputs[2], &[2], &expected);
    }
    let mut builder = b.program();
    let empty = builder.input(shape(&[2, 0, 3])).unwrap();
    let scalar = builder.input(shape(&[])).unwrap();
    let broadcast = builder.broadcast_to(scalar, shape(&[2, 0, 3])).unwrap();
    let sum = builder.sum_axes(empty, &[1], false).unwrap();
    let total = builder.sum_axes(empty, &[0, 1, 2], false).unwrap();
    let retained_empty = builder.sum_axes(empty, &[0], true).unwrap();
    let program = builder
        .compile(&[broadcast, sum, total, retained_empty])
        .unwrap();
    let input = b.upload_f32(shape(&[2, 0, 3]), &[]).unwrap();
    let scalar = b.upload_f32(shape(&[]), &[9.]).unwrap();
    let outputs = program.run(&[&input, &scalar]).unwrap();
    values(&b, &outputs[0], &[2, 0, 3], &[]);
    values(&b, &outputs[1], &[2, 3], &[0.; 6]);
    values(&b, &outputs[2], &[], &[0.]);
    values(&b, &outputs[3], &[1, 0, 3], &[]);

    // A zero batch with positive M/K/N must use the validated empty result;
    // passing this shape to native eager matmul has crashed older MLX builds.
    let mut builder = b.program();
    let left = builder.input(shape(&[0, 2, 3])).unwrap();
    let right = builder.input(shape(&[1, 3, 4])).unwrap();
    let product = builder.matmul(left, right).unwrap();
    let total = builder.sum_axes(product, &[0, 1, 2], false).unwrap();
    let program = builder.compile(&[product, total]).unwrap();
    let left = b.upload_f32(shape(&[0, 2, 3]), &[]).unwrap();
    let right = b.upload_f32(shape(&[1, 3, 4]), &[2.; 12]).unwrap();
    let outputs = program.run(&[&left, &right]).unwrap();
    traces(&program, 1);
    values(&b, &outputs[0], &[0, 2, 4], &[]);
    values(&b, &outputs[1], &[], &[0.]);
}

#[test]
fn invalid_builder_values_and_shapes_do_not_corrupt_valid_nodes() {
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    assert!(matches!(
        builder.input(shape(&[i32::MAX as usize + 1])),
        Err(MlxError::TooLarge)
    ));
    #[cfg(target_pointer_width = "64")]
    assert!(matches!(
        builder.input(shape(&[i32::MAX as usize, i32::MAX as usize])),
        Err(MlxError::TooLarge)
    ));
    // Rejected input declarations must not consume slots: the valid program
    // below still accepts exactly its three subsequently declared inputs.
    let input = builder.input(shape(&[2, 3])).unwrap();
    let scalar = builder.input(shape(&[])).unwrap();
    let mut other = b.program();
    let foreign = other.input(shape(&[2, 3])).unwrap();
    assert!(matches!(
        builder.unary(foreign, UnaryOp::Abs),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        builder.binary(input, foreign, BinaryOp::Add),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        builder.sum_axes(foreign, &[0], false),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        builder.reshape(foreign, shape(&[6])),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        builder.permute(foreign, &[1, 0]),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        builder.broadcast_to(foreign, shape(&[1, 2, 3])),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        builder.matmul(input, foreign),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        other.compile(&[input]),
        Err(MlxError::ForeignValue)
    ));
    assert!(builder.reshape(input, shape(&[5])).is_err());
    assert!(builder.permute(input, &[0, 0]).is_err());
    assert!(builder.broadcast_to(input, shape(&[2, 4])).is_err());
    assert!(builder.sum_axes(input, &[2], false).is_err());
    assert!(builder.sum_axes(input, &[1, 1], false).is_err());
    assert!(builder.matmul(scalar, input).is_err());
    assert!(builder.matmul(input, input).is_err());
    let invalid_empty = builder.input(shape(&[4, 0])).unwrap();
    assert!(builder.matmul(input, invalid_empty).is_err());
    let valid = builder.unary(input, UnaryOp::Square).unwrap();
    let program = builder.compile(&[valid]).unwrap();
    let input = b
        .upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])
        .unwrap();
    let scalar = b.upload_f32(shape(&[]), &[1.]).unwrap();
    let empty = b.upload_f32(shape(&[4, 0]), &[]).unwrap();
    let result = program.run(&[&input, &scalar, &empty]).unwrap();
    traces(&program, 1);
    values(&b, &result[0], &[2, 3], &[1., 4., 9., 16., 25., 36.]);
}

#[test]
fn runtime_input_rejection_happens_before_native_tracing() {
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    let input = builder.input(shape(&[2, 3])).unwrap();
    let output = builder.unary(input, UnaryOp::Square).unwrap();
    let program = builder.compile(&[output]).unwrap();
    let correct = b.upload_f32(shape(&[2, 3]), &[1.; 6]).unwrap();
    let wrong_shape = b.upload_f32(shape(&[3, 2]), &[1.; 6]).unwrap();
    let wrong_dtype = b.upload_u32(shape(&[2, 3]), &[1; 6]).unwrap();
    let independent = MlxBackend::new_gpu().unwrap();
    let foreign = independent.upload_f32(shape(&[2, 3]), &[1.; 6]).unwrap();
    for expected_traces in [0, 1] {
        assert!(matches!(
            program.run(&[]),
            Err(MlxError::ProgramInputCount {
                expected: 1,
                actual: 0
            })
        ));
        assert!(matches!(
            program.run(&[&correct, &correct]),
            Err(MlxError::ProgramInputCount {
                expected: 1,
                actual: 2
            })
        ));
        assert!(matches!(
            program.run(&[&wrong_shape]),
            Err(MlxError::ProgramInputShape { index: 0, .. })
        ));
        assert!(matches!(program.run(&[&wrong_dtype]), Err(MlxError::Dtype)));
        assert!(matches!(
            program.run(&[&foreign]),
            Err(MlxError::ForeignContext)
        ));
        assert_eq!(program.trace_count(), expected_traces);
        if expected_traces == 0 {
            let result = program.run(&[&correct]).unwrap();
            values(&b, &result[0], &[2, 3], &[1.; 6]);
        }
    }
}

#[test]
fn unevaluated_outputs_keep_input_and_context_lifetimes_after_program_drop() {
    let Some(b) = backend() else { return };
    let reader = b.clone();
    let mut builder = b.program();
    let input = builder.input(shape(&[3])).unwrap();
    let squared = builder.unary(input, UnaryOp::Square).unwrap();
    let total = builder.sum_axes(squared, &[0], false).unwrap();
    let program = builder.compile(&[squared, total, input]).unwrap();
    let source = b.upload_f32(shape(&[3]), &[2., -3., 4.]).unwrap();
    drop(b);
    // The compiled program remains callable after its constructing backend is
    // dropped. Its outputs must then outlive both program and input handles.
    let outputs = program.run(&[&source]).unwrap();
    drop(source);
    drop(program);
    values(&reader, &outputs[0], &[3], &[4., 9., 16.]);
    values(&reader, &outputs[1], &[], &[29.]);
    values(&reader, &outputs[2], &[3], &[2., -3., 4.]);
    reader.synchronize().unwrap();
}

#[test]
fn empty_output_list_replays_without_evaluated_result_handles() {
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    builder.input(shape(&[])).unwrap();
    let program = builder.compile(&[]).unwrap();
    for run in 1..=3 {
        let input = b.upload_f32(shape(&[]), &[run as f32]).unwrap();
        assert!(program.run(&[&input]).unwrap().is_empty());
        traces(&program, run);
    }
    let empty = b.program().compile(&[]).unwrap();
    assert!(empty.run(&[]).unwrap().is_empty());
    traces(&empty, 1);
}

#[test]
fn disabled_native_compilation_still_replays_fresh_values_in_child() {
    if std::env::var_os("COMPUTE_MLX_COMPILED_CHILD").is_none() {
        return;
    }
    assert!(std::env::var_os("MLX_DISABLE_COMPILE").is_some());
    let Some(b) = backend() else { return };
    let mut builder = b.program();
    let x = builder.input(shape(&[2])).unwrap();
    let y = builder.unary(x, UnaryOp::Square).unwrap();
    let program = builder.compile(&[y]).unwrap();
    for run in 1..=3 {
        let input = b
            .upload_f32(shape(&[2]), &[run as f32, -(run as f32)])
            .unwrap();
        let outputs = program.run(&[&input]).unwrap();
        values(&b, &outputs[0], &[2], &[(run * run) as f32; 2]);
        assert_eq!(program.trace_count(), run);
    }
    eprintln!("disabled compiler: 3 fresh executions, 3 callback traces");
}

#[test]
fn disabled_mode_is_qualified_without_mutating_parent_global_state() {
    let Some(b) = backend() else { return };
    drop(b);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "disabled_native_compilation_still_replays_fresh_values_in_child",
            "--nocapture",
        ])
        .env("MLX_DISABLE_COMPILE", "1")
        .env("COMPUTE_MLX_COMPILED_CHILD", "1")
        .env("COMPUTE_REQUIRE_MLX", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("compiled disabled-mode child:\n{stdout}{stderr}");
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains("3 fresh executions, 3 callback traces"));
}
