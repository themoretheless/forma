use compute_cuda::{CudaError, CudaPrepareOptions, CudaRuntime, CudaTensor};
use tensor_core::{BinaryOp, MatmulPrecision, ReduceOp, Shape, TensorBackend, UnaryOp};

type Result<T = ()> = std::result::Result<T, CudaError>;
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn exact(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(f64::from(a), b, "element {i}");
    }
}

/// Host-only runs explicitly skip this entry. Required-device qualification
/// must execute every case below; driver, compiler and cuBLAS errors fail it.
#[test]
fn native_cuda_prepared_contract() -> Result {
    let rt = match CudaRuntime::new() {
        Ok(rt) => rt,
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required native prepared CUDA execution unavailable: {error}"
            );
            eprintln!("SKIP native prepared CUDA qualification: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    eprintln!("Prepared CUDA hardware: {:?}", rt.capabilities());
    canonical_arithmetic_and_views(&rt)?;
    changed_inputs_and_independent_outputs(&rt)?;
    reductions_and_empty_identities(&rt)?;
    matrix_layouts_vectors_and_precision(&rt)?;
    validation_preserves_outputs_and_allows_retry(&rt)?;
    rt.synchronize()
}

fn canonical_arithmetic_and_views(rt: &CudaRuntime) -> Result {
    let mut input = rt.upload_f32(shape(&[2, 3]), &[0.25, 0.5, 1., 1.5, 2., 4.])?;
    let scalar = rt.upload_f32(shape(&[]), &[2.])?;
    let mut graph = rt.program();
    let x = graph.input(input.layout().clone())?;
    let y = graph.input(scalar.layout().clone())?;
    let unary = [
        UnaryOp::Negate,
        UnaryOp::Abs,
        UnaryOp::Square,
        UnaryOp::Sqrt,
        UnaryOp::Reciprocal,
        UnaryOp::Exp,
        UnaryOp::Log,
        UnaryOp::Sin,
        UnaryOp::Cos,
    ];
    let binary = [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ];
    let mut outputs = Vec::new();
    for op in unary {
        outputs.push(graph.unary(x, op)?);
    }
    for op in binary {
        outputs.push(graph.binary(x, y, op)?);
    }
    let transpose = graph.permute(x, &[1, 0])?;
    outputs.push(graph.reshape(transpose, shape(&[6]))?);
    outputs.push(graph.broadcast_to(y, shape(&[2, 3]))?);
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    for values in [[0.25, 0.5, 1., 1.5, 2., 4.], [4., 2., 1.5, 1., 0.5, 0.25]] {
        rt.write_f32(&mut input, &values)?;
        let actual = program.run(&[&input, &scalar])?;
        for (i, op) in unary.into_iter().enumerate() {
            let result = rt.read_f32(&actual[i])?;
            for (&a, &x) in result.iter().zip(&values) {
                let x = f64::from(x);
                let expected = match op {
                    UnaryOp::Negate => -x,
                    UnaryOp::Abs => x.abs(),
                    UnaryOp::Square => x * x,
                    UnaryOp::Sqrt => x.sqrt(),
                    UnaryOp::Reciprocal => 1. / x,
                    UnaryOp::Exp => x.exp(),
                    UnaryOp::Log => x.ln(),
                    UnaryOp::Sin => x.sin(),
                    UnaryOp::Cos => x.cos(),
                };
                assert!(
                    (f64::from(a) - expected).abs() <= 2e-7 + 2e-6 * expected.abs(),
                    "{op:?}: {a} != {expected}"
                );
            }
        }
        for (i, op) in binary.into_iter().enumerate() {
            let expected: Vec<_> = values
                .iter()
                .map(|&x| {
                    let x = f64::from(x);
                    match op {
                        BinaryOp::Add => x + 2.,
                        BinaryOp::Subtract => x - 2.,
                        BinaryOp::Multiply => x * 2.,
                        BinaryOp::Divide => x / 2.,
                        BinaryOp::Min => x.min(2.),
                        BinaryOp::Max => x.max(2.),
                    }
                })
                .collect();
            exact(&rt.read_f32(&actual[unary.len() + i])?, &expected);
        }
        exact(
            &rt.read_f32(&actual[15])?,
            &[0, 3, 1, 4, 2, 5].map(|i| f64::from(values[i])),
        );
        exact(&rt.read_f32(&actual[16])?, &[2.; 6]);
    }
    Ok(())
}

fn changed_inputs_and_independent_outputs(rt: &CudaRuntime) -> Result {
    let (rows, cols) = (13, 17);
    let physical = rt.upload_f32(shape(&[cols, rows]), &vec![0.; rows * cols])?;
    let signature = rt.permute(&physical, &[1, 0])?;
    let mut graph = rt.program();
    let x = graph.input(signature.layout().clone())?;
    let weights = rt.upload_f32(
        shape(&[cols]),
        &(0..cols).map(|i| (i % 3) as f32 / 4.).collect::<Vec<_>>(),
    )?;
    let bias = rt.upload_f32(shape(&[]), &[0.5])?;
    let w = graph.input(weights.layout().clone())?;
    let b = graph.input(bias.layout().clone())?;
    let product = graph.binary(x, w, BinaryOp::Multiply)?;
    let shifted = graph.binary(product, b, BinaryOp::Add)?;
    let y = graph.unary(shifted, UnaryOp::Square)?;
    let total = graph.sum_axes(y, &[0, 1], false)?;
    let flattened = graph.reshape(x, shape(&[rows * cols]))?;
    let mut program = graph.prepare(&[y, total, x, y, flattened], CudaPrepareOptions::default())?;
    assert!(program.stats().scratch_bytes > 0);
    assert!(program.stats().metadata_bytes > 0);
    drop(signature);
    drop(physical);

    let mut old: Option<(CudaTensor, Vec<f64>)> = None;
    for iteration in 0..3 {
        let values: Vec<f32> = (0..rows * cols)
            .map(|i| ((i * 7 + iteration * 3) % 9) as f32 / 4. - 1.)
            .collect();
        let mut storage = rt.upload_f32(shape(&[cols, rows]), &values)?;
        let input = rt.permute(&storage, &[1, 0])?;
        let logical: Vec<f64> = (0..rows * cols)
            .map(|i| f64::from(values[(i % cols) * rows + i / cols]))
            .collect();
        let expected: Vec<f64> = logical
            .iter()
            .enumerate()
            .map(|(i, &v)| (v * ((i % cols) % 3) as f64 / 4. + 0.5).powi(2))
            .collect();
        let outputs = program.run(&[&input, &weights, &bias])?;
        exact(&rt.read_f32(&outputs[0])?, &expected);
        exact(&rt.read_f32(&outputs[1])?, &[expected.iter().sum()]);
        exact(&rt.read_f32(&outputs[2])?, &logical);
        exact(&rt.read_f32(&outputs[3])?, &expected);
        exact(&rt.read_f32(&outputs[4])?, &logical);
        if let Some((previous, reference)) = old.take() {
            exact(&rt.read_f32(&previous)?, &reference);
        }
        old = Some((outputs[0].clone(), expected));
        drop(input);
        // The prepared program must not retain an input clone after replay.
        rt.write_f32(&mut storage, &vec![123.; rows * cols])?;
    }
    if let Some((previous, reference)) = old {
        drop(program);
        exact(&rt.read_f32(&previous)?, &reference);
    }
    Ok(())
}

fn reductions_and_empty_identities(rt: &CudaRuntime) -> Result {
    let n = 131_077;
    let values: Vec<f32> = (0..n).map(|i| (i % 17) as f32 / 4. - 2.).collect();
    let input = rt.upload_f32(shape(&[n]), &values)?;
    let mut graph = rt.program();
    let x = graph.input(input.layout().clone())?;
    let total = graph.sum_axes(x, &[0], false)?;
    let mean = graph.mean(x, &[0], true)?;
    let minimum = graph.reduce(x, ReduceOp::Min, &[0], false)?;
    let maximum = graph.reduce(x, ReduceOp::Max, &[0], false)?;
    let mut program = graph.prepare(
        &[total, mean, minimum, maximum],
        CudaPrepareOptions::default(),
    )?;
    let sum: f64 = values.iter().map(|&v| f64::from(v)).sum();
    for _ in 0..2 {
        let outputs = program.run(&[&input])?;
        exact(&rt.read_f32(&outputs[0])?, &[sum]);
        assert_eq!(rt.read_f32(&outputs[1])?, [(sum / n as f64) as f32]);
        exact(&rt.read_f32(&outputs[2])?, &[-2.]);
        exact(&rt.read_f32(&outputs[3])?, &[2.]);
    }

    let physical: Vec<f32> = (0..3 * 257).map(|i| (i % 11) as f32 / 4. - 1.25).collect();
    let strided = rt.permute(&rt.upload_f32(shape(&[257, 3]), &physical)?, &[1, 0])?;
    let mut graph = rt.program();
    let x = graph.input(strided.layout().clone())?;
    let sum = graph.sum_axes(x, &[1], true)?;
    let mean = graph.mean(x, &[1], false)?;
    let identity = graph.reduce(x, ReduceOp::Product, &[], false)?;
    let mut prepared = graph.prepare(&[sum, mean, identity], CudaPrepareOptions::default())?;
    let expected: Vec<f64> = (0..3)
        .map(|row| (0..257).map(|col| f64::from(physical[col * 3 + row])).sum())
        .collect();
    for _ in 0..2 {
        let outputs = prepared.run(&[&strided])?;
        exact(&rt.read_f32(&outputs[0])?, &expected);
        assert_eq!(
            rt.read_f32(&outputs[1])?,
            expected
                .iter()
                .map(|&v| (v / 257.) as f32)
                .collect::<Vec<_>>()
        );
        exact(
            &rt.read_f32(&outputs[2])?,
            &(0..3 * 257)
                .map(|i| f64::from(physical[(i % 257) * 3 + i / 257]))
                .collect::<Vec<_>>(),
        );
    }

    let empty = rt.upload_f32(shape(&[2, 0]), &[])?;
    let right = rt.upload_f32(shape(&[0, 3]), &[])?;
    let mut graph = rt.program();
    let x = graph.input(empty.layout().clone())?;
    let b = graph.input(right.layout().clone())?;
    let sum = graph.reduce(x, ReduceOp::Sum, &[1], false)?;
    let product = graph.reduce(x, ReduceOp::Product, &[1], false)?;
    assert!(graph.mean(x, &[1], false).is_err());
    assert!(graph.reduce(x, ReduceOp::Min, &[1], false).is_err());
    let matrix = graph.matmul(x, b, MatmulPrecision::F32)?;
    let mut program = graph.prepare(&[sum, product, matrix, x], CudaPrepareOptions::default())?;
    let mut sum = rt.upload_f32(shape(&[2]), &[99.; 2])?;
    let mut product = rt.upload_f32(shape(&[2]), &[99.; 2])?;
    let mut matrix = rt.upload_f32(shape(&[2, 3]), &[99.; 6])?;
    let mut empty_output = rt.upload_f32(shape(&[2, 0]), &[])?;
    for _ in 0..2 {
        program.run_into(
            &[&empty, &right],
            &mut [&mut sum, &mut product, &mut matrix, &mut empty_output],
        )?;
        exact(&rt.read_f32(&sum)?, &[0.; 2]);
        exact(&rt.read_f32(&product)?, &[1.; 2]);
        exact(&rt.read_f32(&matrix)?, &[0.; 6]);
        assert!(rt.read_f32(&empty_output)?.is_empty());
        rt.write_f32(&mut sum, &[77.; 2])?;
        rt.write_f32(&mut product, &[77.; 2])?;
        rt.write_f32(&mut matrix, &[77.; 6])?;
    }
    Ok(())
}

fn matrix_layouts_vectors_and_precision(rt: &CudaRuntime) -> Result {
    // Logical [2,3,5] @ [1,5,4], both matrix operands noncontiguous.
    let av: Vec<f32> = (0..30).map(|i| (i % 7) as f32 / 4. - 0.75).collect();
    let bv: Vec<f32> = (0..20).map(|i| (i % 5) as f32 / 4. - 0.5).collect();
    let a = rt.permute(&rt.upload_f32(shape(&[2, 5, 3]), &av)?, &[0, 2, 1])?;
    let b = rt.permute(&rt.upload_f32(shape(&[1, 4, 5]), &bv)?, &[0, 2, 1])?;
    let expected: Vec<f64> = (0..24)
        .map(|i| {
            let (batch, row, col) = (i / 12, (i / 4) % 3, i % 4);
            (0..5)
                .map(|k| f64::from(av[batch * 15 + k * 3 + row]) * f64::from(bv[col * 5 + k]))
                .sum()
        })
        .collect();
    for precision in [
        MatmulPrecision::F32,
        MatmulPrecision::AllowTf32,
        MatmulPrecision::AllowF16,
        MatmulPrecision::AllowBf16,
    ] {
        let mut graph = rt.program();
        let left = graph.input(a.layout().clone())?;
        let right = graph.input(b.layout().clone())?;
        let result = graph.matmul(left, right, precision)?;
        let prepared = graph.prepare(&[result], CudaPrepareOptions::default());
        if rt.matmul_policy(precision).is_err() {
            assert!(matches!(prepared, Err(CudaError::UnsupportedPrecision(_))));
            continue;
        }
        let mut prepared = prepared?;
        assert_eq!(prepared.stats().gemm_calls, 2);
        let actual = prepared.run(&[&a, &b])?;
        exact(&rt.read_f32(&actual[0])?, &expected);
        exact(&rt.read_f32(&rt.matmul(&a, &b, precision)?)?, &expected);
    }
    let vector = rt.narrow(
        &rt.upload_f32(shape(&[5]), &[99., 1., 2., 3., 99.])?,
        0,
        1,
        3,
    )?;
    let twos = rt.broadcast_to(&rt.upload_f32(shape(&[]), &[2.])?, shape(&[3]))?;
    let mut graph = rt.program();
    let a = graph.input(vector.layout().clone())?;
    let b = graph.input(twos.layout().clone())?;
    let result = graph.matmul(a, b, MatmulPrecision::F32)?;
    let mut program = graph.prepare(&[result], CudaPrepareOptions::default())?;
    let result = program.run(&[&vector, &twos])?;
    assert_eq!(result[0].shape(), &shape(&[]));
    exact(&rt.read_f32(&result[0])?, &[12.]);
    Ok(())
}

fn validation_preserves_outputs_and_allows_retry(rt: &CudaRuntime) -> Result {
    let mut input = rt.upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])?;
    let mut graph = rt.program();
    let x = graph.input(input.layout().clone())?;
    let y = graph.affine(x, 2., 1.)?;
    let mut program = graph.prepare(&[y], CudaPrepareOptions::default())?;
    let mut output = rt.upload_f32(shape(&[2, 3]), &[77.; 6])?;
    assert!(program.run_into(&[], &mut [&mut output]).is_err());
    let wrong_shape = rt.upload_f32(shape(&[3, 2]), &[0.; 6])?;
    assert!(
        program
            .run_into(&[&wrong_shape], &mut [&mut output])
            .is_err()
    );
    let wrong_stride = rt.permute(&wrong_shape, &[1, 0])?;
    assert!(
        program
            .run_into(&[&wrong_stride], &mut [&mut output])
            .is_err()
    );
    let other = CudaRuntime::new()?;
    let foreign = other.upload_f32(shape(&[2, 3]), &[0.; 6])?;
    assert!(program.run_into(&[&foreign], &mut [&mut output]).is_err());
    let alias = output.clone();
    assert!(program.run_into(&[&input], &mut [&mut output]).is_err());
    drop(alias);
    let mut input_alias = input.clone();
    assert!(
        program
            .run_into(&[&input], &mut [&mut input_alias])
            .is_err()
    );
    drop(input_alias);
    exact(&rt.read_f32(&output)?, &[77.; 6]);
    let backing = rt.upload_f32(shape(&[7]), &[77.; 7])?;
    let mut offset_output = rt.reshape(&rt.narrow(&backing, 0, 1, 6)?, shape(&[2, 3]))?;
    drop(backing);
    assert!(
        program
            .run_into(&[&input], &mut [&mut offset_output])
            .is_err()
    );
    exact(&rt.read_f32(&offset_output)?, &[77.; 6]);
    for iteration in 0..2 {
        let values: Vec<f32> = (0..6).map(|i| (i + iteration * 7) as f32).collect();
        rt.write_f32(&mut input, &values)?;
        program.run_into(&[&input], &mut [&mut output])?;
        exact(
            &rt.read_f32(&output)?,
            &values
                .iter()
                .map(|&v| f64::from(v) * 2. + 1.)
                .collect::<Vec<_>>(),
        );
    }
    Ok(())
}
