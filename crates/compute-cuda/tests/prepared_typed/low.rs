use super::*;
use tensor_core::TensorLowOpsBackend;

fn decode(dtype: LowDtype, bits: u16) -> f32 {
    if dtype == LowDtype::Bf16 {
        return f32::from_bits(u32::from(bits) << 16);
    }
    let sign = if bits & 0x8000 == 0 { 1.0 } else { -1.0 };
    let exp = (bits >> 10) & 31;
    let fraction = bits & 1023;
    match exp {
        0 => sign * f32::from(fraction) * 2.0f32.powi(-24),
        31 => {
            if fraction == 0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + f32::from(fraction) / 1024.0) * 2.0f32.powi(i32::from(exp) - 15),
    }
}

pub fn raw_bits_casts_and_views(rt: &CudaRuntime, dtype: LowDtype) -> Result {
    let bits: Vec<_> = (0..=u16::MAX).collect();
    let input = rt.upload_low(dtype, shape(&[256, 256]), &bits)?;
    let mut graph = rt.program();
    let x = graph.input_low(dtype, input.layout().clone())?;
    let transpose = graph.permute(x, &[1, 0])?;
    let flat = graph.reshape(transpose, shape(&[65536]))?;
    let decoded = graph.cast_to_f32(x)?;
    let encoded = graph.cast_to_low(decoded, dtype)?;
    let is_nan = graph.compare_low(x, x, CompareOp::NotEqual)?;
    let selected = graph.select_low(is_nan, x, transpose)?;
    let mut program = graph.prepare(
        &[x, flat, decoded, encoded, selected],
        CudaPrepareOptions::default(),
    )?;
    for _ in 0..2 {
        let result = program.run_typed(&[(&input).into()])?;
        assert_eq!(rt.read_low_bits(result[0].as_low()?)?, bits);
        let expected: Vec<_> = (0..65536)
            .map(|i| bits[(i % 256) * 256 + i / 256])
            .collect();
        assert_eq!(rt.read_low_bits(result[1].as_low()?)?, expected);
        let decoded = rt.read_f32(result[2].as_f32()?)?;
        let encoded = rt.read_low_bits(result[3].as_low()?)?;
        let selected = rt.read_low_bits(result[4].as_low()?)?;
        for (i, &raw) in bits.iter().enumerate() {
            let expected = decode(dtype, raw);
            if expected.is_nan() {
                assert!(decoded[i].is_nan(), "{dtype:?} NaN {raw:04x}");
                assert!(decode(dtype, encoded[i]).is_nan());
            } else {
                assert_eq!(
                    decoded[i].to_bits(),
                    expected.to_bits(),
                    "{dtype:?} decode {raw:04x}"
                );
                assert_eq!(encoded[i], raw, "{dtype:?} encode {raw:04x}");
            }
            assert_eq!(
                selected[i],
                if expected.is_nan() {
                    raw
                } else {
                    bits[(i % 256) * 256 + i / 256]
                }
            );
        }
    }
    Ok(())
}

pub fn arithmetic_rounding_and_reductions(rt: &CudaRuntime, dtype: LowDtype) -> Result {
    let input = rt.cast_to_low(&rt.upload_f32(shape(&[2, 2]), &[0.5, 1., 2., 4.])?, dtype)?;
    let input = rt.permute_low(&input, &[1, 0])?;
    let two = rt.cast_to_low(&rt.upload_f32(shape(&[]), &[2.])?, dtype)?;
    let one = rt.cast_to_low(&rt.upload_f32(shape(&[]), &[1.])?, dtype)?;
    let half_ulp = if dtype == LowDtype::F16 {
        2.0f32.powi(-11)
    } else {
        2.0f32.powi(-8)
    };
    let tiny = rt.cast_to_low(&rt.upload_f32(shape(&[]), &[half_ulp])?, dtype)?;
    let many = rt.cast_to_low(&rt.upload_f32(shape(&[8197]), &vec![1.; 8197])?, dtype)?;
    let empty = rt.upload_low(dtype, shape(&[3, 0]), &[])?;
    let mut graph = rt.program();
    let x = graph.input_low(dtype, input.layout().clone())?;
    let y = graph.input_low(dtype, two.layout().clone())?;
    let a = graph.input_low(dtype, one.layout().clone())?;
    let t = graph.input_low(dtype, tiny.layout().clone())?;
    let m = graph.input_low(dtype, many.layout().clone())?;
    let e = graph.input_low(dtype, empty.layout().clone())?;
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
        outputs.push(graph.unary_low(x, op)?);
    }
    for op in binary {
        outputs.push(graph.binary_low(x, y, op)?);
    }
    let first = graph.binary_low(a, t, BinaryOp::Add)?;
    outputs.push(graph.binary_low(first, t, BinaryOp::Add)?);
    outputs.push(graph.reduce_low_f32(m, ReduceOp::Sum, &[0], false)?);
    outputs.push(graph.mean_low_f32(m, &[0], false)?);
    outputs.push(graph.reduce_low_f32(x, ReduceOp::Sum, &[1], false)?);
    outputs.push(graph.mean_low(x, &[1], false)?);
    outputs.push(graph.reduce_low(e, ReduceOp::Product, &[1], false)?);
    outputs.push(graph.reduce_low_f32(e, ReduceOp::Sum, &[1], false)?);
    outputs.push(graph.reduce_low_f32(x, ReduceOp::Sum, &[], false)?);
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    for _ in 0..2 {
        let result = program.run_typed(&[
            (&input).into(),
            (&two).into(),
            (&one).into(),
            (&tiny).into(),
            (&many).into(),
            (&empty).into(),
        ])?;
        for (i, op) in unary.into_iter().enumerate() {
            let eager = rt.unary_low(op, &input)?;
            assert_eq!(
                rt.read_low_bits(result[i].as_low()?)?,
                rt.read_low_bits(&eager)?,
                "{dtype:?} {op:?}"
            );
        }
        for (i, op) in binary.into_iter().enumerate() {
            let eager = rt.binary_low(op, &input, &two)?;
            assert_eq!(
                rt.read_low_bits(result[9 + i].as_low()?)?,
                rt.read_low_bits(&eager)?,
                "{dtype:?} {op:?}"
            );
        }
        let low_f32 = |index: usize| -> Result<Vec<f32>> {
            rt.read_f32(&rt.cast_to_f32(result[index].as_low()?)?)
        };
        assert_eq!(
            low_f32(15)?,
            [1.],
            "each arithmetic node must round before its consumer"
        );
        assert_eq!(
            rt.read_f32(result[16].as_f32()?)?,
            [8197.],
            "low first pass must accumulate into f32"
        );
        assert_eq!(rt.read_f32(result[17].as_f32()?)?, [1.]);
        assert_eq!(rt.read_f32(result[18].as_f32()?)?, [2.5, 5.]);
        assert_eq!(low_f32(19)?, [1.25, 2.5]);
        assert_eq!(low_f32(20)?, [1.; 3]);
        assert_eq!(rt.read_f32(result[21].as_f32()?)?, [0.; 3]);
        assert_eq!(rt.read_f32(result[22].as_f32()?)?, [0.5, 2., 1., 4.]);
    }
    Ok(())
}

pub fn matrix_products(rt: &CudaRuntime, dtype: LowDtype) -> Result {
    let ulp = if dtype == LowDtype::F16 {
        2.0f32.powi(-10)
    } else {
        2.0f32.powi(-7)
    };
    let vector = rt.cast_to_low(
        &rt.upload_f32(shape(&[4]), &[99., 1. + ulp, 1., 99.])?,
        dtype,
    )?;
    let vector = rt.narrow_low(&vector, 0, 1, 2)?;
    let zero_a = rt.upload_low(dtype, shape(&[2, 0]), &[])?;
    let zero_b = rt.upload_low(dtype, shape(&[0, 3]), &[])?;
    let mut graph = rt.program();
    let x = graph.input_low(dtype, vector.layout().clone())?;
    let a = graph.input_low(dtype, zero_a.layout().clone())?;
    let b = graph.input_low(dtype, zero_b.layout().clone())?;
    let dot = graph.matmul_low_f32(x, x)?;
    let low = graph.matmul_low(x, x)?;
    let zero = graph.matmul_low_f32(a, b)?;
    let prepared = graph.prepare(&[dot, low, zero], CudaPrepareOptions::default());
    if !rt.low_precision_support(dtype).matmul_f32 {
        assert!(prepared.is_err());
        let mut graph = rt.program();
        let a = graph.input_low(dtype, zero_a.layout().clone())?;
        let b = graph.input_low(dtype, zero_b.layout().clone())?;
        let zero = graph.matmul_low_f32(a, b)?;
        assert!(
            graph
                .prepare(&[zero], CudaPrepareOptions::default())
                .is_err(),
            "unsupported dtype must fail even for K=0"
        );
        return Ok(());
    }
    let mut program = prepared?;
    for _ in 0..2 {
        let result = program.run_typed(&[(&vector).into(), (&zero_a).into(), (&zero_b).into()])?;
        assert_eq!(
            rt.read_f32(result[0].as_f32()?)?,
            [(1. + ulp) * (1. + ulp) + 1.]
        );
        assert_eq!(
            rt.read_low_bits(result[1].as_low()?)?,
            rt.read_low_bits(&rt.matmul_low(&vector, &vector)?)?
        );
        assert_eq!(rt.read_f32(result[2].as_f32()?)?, [0.; 6]);
    }
    Ok(())
}
