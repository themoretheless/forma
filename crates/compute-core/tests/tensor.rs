use compute_core::{
    BinaryOp, ComputeError, ComputeRuntime, GpuTensor, TensorComputeError, UnaryOp,
    gpu_compute::GpuContext,
    tensor_core::{Layout, Shape},
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    if context.is_none() {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "required GPU unavailable"
        );
    }
    context
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn upload(rt: &ComputeRuntime, dims: &[usize], values: &[f32]) -> GpuTensor {
    GpuTensor::from_array(rt.upload(values).unwrap(), shape(dims)).unwrap()
}
fn read(program: &compute_core::ComputeProgram<'_>, tensor: &GpuTensor) -> Vec<f32> {
    program
        .submit_read(tensor.values())
        .unwrap()
        .wait(TIMEOUT)
        .unwrap()
}
fn close(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (f64::from(a) - e).abs() <= 3e-5 * e.abs().max(1.0),
            "index {i}: {a} vs {e}"
        );
    }
}
#[test]
fn view_chain_broadcast_reduce_and_repeated_execution_preserve_sentinels() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let raw = rt.zeros::<f32>(30).unwrap();
    let input = GpuTensor::from_layout(
        raw.clone(),
        Layout::new(shape(&[2, 3, 4]), vec![12, 4, 1], 3).unwrap(),
    )
    .unwrap();
    let view = input.permute(&[2, 0, 1]).unwrap().narrow(0, 1, 2).unwrap();
    let bias = upload(&rt, &[3], &[10., 20., 30.]);
    let output_raw = rt.upload(&[-999.0f32; 6]).unwrap();
    let output = GpuTensor::from_layout(
        output_raw.clone(),
        Layout::new(shape(&[2]), vec![1], 2).unwrap(),
    )
    .unwrap();
    let mut program = rt.program();
    let added = program.tensor_binary(BinaryOp::Add, &view, &bias).unwrap();
    let squared = program.tensor_unary(UnaryOp::Square, &added).unwrap();
    program
        .tensor_sum_into(&squared, &[0, 2], false, &output)
        .unwrap();
    for iteration in 0..3 {
        let values: Vec<f32> = (0..30).map(|i| (i + iteration) as f32).collect();
        rt.write(&raw, 0, &values).unwrap();
        let actual = program
            .submit_read(&output_raw)
            .unwrap()
            .wait(TIMEOUT)
            .unwrap();
        let expected: Vec<f64> = (0..2)
            .map(|row| {
                (1..3)
                    .flat_map(|c| (0..3).map(move |col| (c, col)))
                    .map(|(c, col)| {
                        let x =
                            f64::from(values[3 + row * 12 + col * 4 + c]) + (col + 1) as f64 * 10.;
                        x * x
                    })
                    .sum()
            })
            .collect();
        close(&actual[2..4], &expected);
        assert_eq!(&actual[..2], &[-999.; 2]);
        assert_eq!(&actual[4..], &[-999.; 2]);
    }
}
#[test]
fn all_unary_and_binary_ops_accept_transposes_and_axis_broadcast() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let values = [0.25, 0.5, 0.75, 1., 1.25, 1.5];
    let input = upload(&rt, &[2, 3], &values).permute(&[1, 0]).unwrap();
    let logical = [0.25f64, 1., 0.5, 1.25, 0.75, 1.5];
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
        let mut p = rt.program();
        let result = p.tensor_unary(op, &input).unwrap();
        let expected: Vec<f64> = logical
            .iter()
            .map(|&x| match op {
                UnaryOp::Negate => -x,
                UnaryOp::Abs => x.abs(),
                UnaryOp::Square => x * x,
                UnaryOp::Sqrt => x.sqrt(),
                UnaryOp::Reciprocal => 1. / x,
                UnaryOp::Exp => x.exp(),
                UnaryOp::Log => x.ln(),
                UnaryOp::Sin => x.sin(),
                UnaryOp::Cos => x.cos(),
            })
            .collect();
        close(&read(&p, &result), &expected);
    }
    let b = upload(&rt, &[3, 1], &[2., 3., 4.]);
    for op in [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Divide,
        BinaryOp::Min,
        BinaryOp::Max,
    ] {
        let mut p = rt.program();
        let result = p.tensor_binary(op, &input, &b).unwrap();
        let expected: Vec<f64> = logical
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let y = (i / 2 + 2) as f64;
                match op {
                    BinaryOp::Add => x + y,
                    BinaryOp::Subtract => x - y,
                    BinaryOp::Multiply => x * y,
                    BinaryOp::Divide => x / y,
                    BinaryOp::Min => x.min(y),
                    BinaryOp::Max => x.max(y),
                }
            })
            .collect();
        close(&read(&p, &result), &expected);
    }
}
#[test]
fn arbitrary_axis_sets_keepdims_scalar_and_empty_reductions() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let values: Vec<f32> = (0..30).map(|i| i as f32 - 15.).collect();
    let input = upload(&rt, &[2, 3, 5], &values)
        .permute(&[2, 0, 1])
        .unwrap();
    for axes in [
        vec![],
        vec![0],
        vec![1],
        vec![2],
        vec![0, 2],
        vec![2, 1],
        vec![2, 0, 1],
    ] {
        for keep in [false, true] {
            let mut p = rt.program();
            let result = p.tensor_sum(&input, &axes, keep).unwrap();
            let mut expected = vec![0.0f64; result.shape().numel()];
            for x in 0..5 {
                for y in 0..2 {
                    for z in 0..3 {
                        let coords = [x, y, z];
                        let mut output_index = 0;
                        for (axis, &coord) in coords.iter().enumerate() {
                            if !axes.contains(&axis) {
                                output_index = output_index * input.shape().dims()[axis] + coord;
                            }
                        }
                        expected[output_index] += f64::from(values[y * 15 + z * 5 + x]);
                    }
                }
            }
            close(&read(&p, &result), &expected);
        }
    }
    let empty = upload(&rt, &[2, 0, 3], &[]);
    let mut p = rt.program();
    let result = p.tensor_sum(&empty, &[1], false).unwrap();
    assert_eq!(result.shape().dims(), &[2, 3]);
    assert_eq!(read(&p, &result), vec![0.; 6]);
    let mut p = rt.program();
    let result = p.tensor_sum(&empty, &[0], true).unwrap();
    assert!(read(&p, &result).is_empty());
    let scalar = upload(&rt, &[], &[7.]);
    let mut p = rt.program();
    let result = p.tensor_sum(&scalar, &[], false).unwrap();
    assert_eq!(read(&p, &result), vec![7.]);
    assert!(p.tensor_sum(&scalar, &[0], false).is_err());
}
#[test]
fn rank_twelve_views_materialize_without_fixed_rank_limit() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let mut dims = vec![1; 12];
    dims[0] = 2;
    dims[11] = 257;
    let values: Vec<f32> = (0..514).map(|i| i as f32).collect();
    let input = upload(&rt, &dims, &values);
    let axes: Vec<usize> = (0..12).rev().collect();
    let view = input.permute(&axes).unwrap();
    let mut p = rt.program();
    let materialized = p.tensor_materialize(&view).unwrap();
    let expected: Vec<f64> = (0..257)
        .flat_map(|i| [i as f64, (i + 257) as f64])
        .collect();
    close(&read(&p, &materialized), &expected);
    assert!(view.reshape(shape(&[514])).is_err());
    assert_eq!(
        materialized.reshape(shape(&[514])).unwrap().shape().dims(),
        &[514]
    );
}
#[test]
fn batched_matmul_broadcasts_and_handles_transposes_odd_tiles() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let (m, k, n) = (17, 19, 23);
    let av: Vec<f32> = (0..2 * k * m)
        .map(|i| ((i * 13 % 31) as f32 - 15.) / 16.)
        .collect();
    let bv: Vec<f32> = (0..3 * k * n)
        .map(|i| ((i * 7 % 23) as f32 - 11.) / 16.)
        .collect();
    let a = upload(&rt, &[2, 1, k, m], &av)
        .permute(&[0, 1, 3, 2])
        .unwrap();
    let b = upload(&rt, &[3, k, n], &bv);
    let mut p = rt.program();
    let output = p.tensor_matmul(&a, &b).unwrap();
    assert_eq!(output.shape().dims(), &[2, 3, m, n]);
    let mut expected = Vec::new();
    for x in 0..2 {
        for y in 0..3 {
            for row in 0..m {
                for col in 0..n {
                    expected.push(
                        (0..k)
                            .map(|inner| {
                                f64::from(av[x * k * m + inner * m + row])
                                    * f64::from(bv[y * k * n + inner * n + col])
                            })
                            .sum(),
                    );
                }
            }
        }
    }
    close(&read(&p, &output), &expected);
}
#[test]
fn axis_sum_and_batched_matmul_cross_workgroup_dispatch_limit() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let n = 65539;
    let values: Vec<f32> = (0..n * 2).map(|i| (i % 17) as f32).collect();
    let input = upload(&rt, &[n, 2], &values);
    let mut p = rt.program();
    let sums = p.tensor_sum(&input, &[1], false).unwrap();
    let expected: Vec<f64> = values
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| f64::from(v[0] + v[1]))
        .collect();
    close(&read(&p, &sums), &expected);
    let a = upload(&rt, &[1, 1, 1], &[2.]);
    let b = upload(&rt, &[1, 1, 1], &[3.])
        .broadcast_to(shape(&[n, 1, 1]))
        .unwrap();
    let mut p = rt.program();
    let output = p.tensor_matmul(&a, &b).unwrap();
    assert_eq!(read(&p, &output), vec![6.; n]);
}
#[test]
fn zero_contractions_overwrite_reused_outputs_and_zero_batch_is_empty() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let a = upload(&rt, &[2, 3, 0], &[]);
    let b = upload(&rt, &[1, 0, 5], &[]);
    let output = upload(&rt, &[2, 3, 5], &[-99.; 30]);
    let mut p = rt.program();
    p.tensor_matmul_into(&a, &b, &output).unwrap();
    for _ in 0..2 {
        rt.write(output.values(), 0, &[-99.; 30]).unwrap();
        assert_eq!(read(&p, &output), vec![0.; 30]);
    }
    let a = upload(&rt, &[0, 3, 7], &[]);
    let b = upload(&rt, &[1, 7, 5], &[1.; 35]);
    let mut p = rt.program();
    let out = p.tensor_matmul(&a, &b).unwrap();
    assert_eq!(out.shape().dims(), &[0, 3, 5]);
    assert!(read(&p, &out).is_empty());
}
#[test]
fn invalid_metadata_ownership_shapes_and_aliases_leave_program_usable() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let a = upload(&rt, &[2, 3], &[1.; 6]);
    let foreign = upload(&other, &[2, 3], &[1.; 6]);
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_unary(UnaryOp::Abs, &foreign),
        Err(TensorComputeError::Compute(ComputeError::ForeignArray))
    ));
    assert!(matches!(
        p.tensor_materialize_into(&a, &a),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    let bad_output = GpuTensor::from_layout(
        rt.zeros(6).unwrap(),
        Layout::new(shape(&[2, 3]), vec![1, 2], 0).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        p.tensor_materialize_into(&a, &bad_output),
        Err(TensorComputeError::OutputNotContiguous)
    ));
    assert!(p.tensor_sum(&a, &[0, 0], false).is_err());
    assert!(p.tensor_sum(&a, &[2], false).is_err());
    assert!(
        p.tensor_binary(BinaryOp::Add, &a, &upload(&rt, &[4], &[1.; 4]))
            .is_err()
    );
    assert!(p.tensor_matmul(&a, &a).is_err());
    assert!(p.tensor_matmul(&upload(&rt, &[3], &[1.; 3]), &a).is_err());
    assert!(
        GpuTensor::from_layout(
            a.values().clone(),
            Layout::new(shape(&[2, 3]), vec![3, 1], 1).unwrap()
        )
        .is_err()
    );
    assert!(a.permute(&[0, 0]).is_err());
    let out = p.tensor_materialize(&a).unwrap();
    assert_eq!(read(&p, &out), vec![1.; 6]);
}

fn reference_axes(values: &[f32], input: &GpuTensor, axes: &[usize]) -> Vec<f64> {
    let output_shape = input.shape().reduce(axes, false).unwrap();
    let mut output = vec![0.0; output_shape.numel()];
    for index in 0..input.shape().numel() {
        let mut remaining = index;
        let mut output_index = 0;
        let mut output_stride = 1;
        for (axis, &dimension) in input.shape().dims().iter().enumerate().rev() {
            let coordinate = remaining % dimension;
            remaining /= dimension;
            if !axes.contains(&axis) {
                output_index += coordinate * output_stride;
                output_stride *= dimension;
            }
        }
        output[output_index] += f64::from(values[input.layout().element_offset(index).unwrap()]);
    }
    output
}

#[test]
fn hierarchical_axis_reductions_reuse_strided_inputs_and_offset_outputs() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for (dims, permutation, axes) in [
        (vec![17, 257, 33], vec![2, 0, 1], vec![0, 1, 2]),
        (vec![3, 5003, 2], vec![1, 0, 2], vec![0, 2]),
        (vec![65, 257, 65], vec![0, 2, 1], vec![1, 2]),
    ] {
        let count: usize = dims.iter().product();
        let raw = rt.zeros::<f32>(count + 7).unwrap();
        rt.write(&raw, count, &[12345.; 7]).unwrap();
        let input = GpuTensor::from_layout(raw.clone(), Layout::contiguous(shape(&dims)).unwrap())
            .unwrap()
            .permute(&permutation)
            .unwrap();
        let output_shape = input.shape().reduce(&axes, false).unwrap();
        let output_count = output_shape.numel();
        let output_raw = rt.upload(&vec![-999.0f32; output_count + 4]).unwrap();
        let dense = Layout::contiguous(output_shape.clone()).unwrap();
        let output = GpuTensor::from_layout(
            output_raw.clone(),
            Layout::new(output_shape, dense.strides().to_vec(), 2).unwrap(),
        )
        .unwrap();
        let mut program = rt.program();
        program
            .tensor_sum_into(&input, &axes, false, &output)
            .unwrap();
        for iteration in 0..3 {
            let values: Vec<f32> = (0..count)
                .map(|i| ((i + iteration * 7) % 17) as f32 / 16.0 - 0.5)
                .collect();
            rt.write(&raw, 0, &values).unwrap();
            let expected = reference_axes(&values, &input, &axes);
            let actual = program
                .submit_read(&output_raw)
                .unwrap()
                .wait(TIMEOUT)
                .unwrap();
            close(&actual[2..2 + output_count], &expected);
            assert_eq!(&actual[..2], &[-999.; 2]);
            assert_eq!(&actual[2 + output_count..], &[-999.; 2]);
        }
    }
    // More than 256 chunks are requested, but the allocation stays bounded.
    let input = upload(&rt, &[2, 1], &[0.25, -0.5])
        .broadcast_to(shape(&[2, 2_000_003]))
        .unwrap();
    let mut p = rt.program();
    let output = p.tensor_sum(&input, &[1], false).unwrap();
    assert_eq!(read(&p, &output), vec![500000.75, -1000001.5]);
}

#[test]
fn complete_contiguous_tensor_sum_reuses_array_reduction_and_prefixes() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let n = 1_000_003;
    let raw = rt.zeros::<f32>(n + 7).unwrap();
    rt.write(&raw, n, &[99999.; 7]).unwrap();
    let input = GpuTensor::from_layout(raw.clone(), Layout::contiguous(shape(&[1, n, 1])).unwrap())
        .unwrap();
    let output_raw = rt.upload(&[-99.0f32, 12345., 12345.]).unwrap();
    let output = GpuTensor::from_layout(
        output_raw.clone(),
        Layout::contiguous(shape(&[1, 1, 1])).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    p.tensor_sum_into(&input, &[1], true, &output).unwrap();
    for value in [1.0, 2.0] {
        rt.write(&raw, 0, &vec![value; n]).unwrap();
        assert_eq!(
            p.submit_read(&output_raw).unwrap().wait(TIMEOUT).unwrap(),
            vec![value * n as f32, 12345., 12345.]
        );
    }
}
