use compute_cuda::{CudaError, CudaRuntime};
use tensor_core::{
    AttentionMask, AttentionOptions, BinaryOp, CompareOp, LowDtype, LowStorage, MatmulPrecision,
    ReduceOp, ScanOptions, ScatterOp, Shape, TensorAttentionBackend, TensorBackend,
    TensorIndexBackend, TensorLowAttentionBackend, TensorLowBackend, TensorLowIndexBackend,
    TensorLowOpsBackend, TensorLowScatterBackend, TensorLowStatsBackend, TensorScatterBackend,
    TensorStatsBackend, UnaryOp,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

fn close(actual: &[f32], expected: &[f64], atol: f64, rtol: f64) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.is_finite() && (f64::from(a) - b).abs() <= atol + rtol * b.abs(),
            "element {i}: {a} != {b} (atol {atol}, rtol {rtol})"
        );
    }
}

/// The only optional part is hardware availability. NVRTC compilation errors,
/// CUDA execution failures and wrong results always fail this test.
#[test]
fn native_cuda_tensor_contract() -> Result<(), CudaError> {
    let rt = match CudaRuntime::new() {
        Ok(rt) => rt,
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "CUDA_REQUIRED / COMPUTE_REQUIRE_CUDA requires native hardware: {error}"
            );
            eprintln!("SKIP CUDA runtime qualification: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    eprintln!("CUDA hardware: {:?}", rt.capabilities());
    tensor_core::conformance::check_backend(&rt)?;
    tensor_core::conformance::check_index_backend(&rt)?;
    tensor_core::conformance::check_reduce_backend(&rt)?;
    tensor_core::conformance::check_scatter_backend(&rt)?;
    tensor_core::conformance::check_low_backend(&rt)?;
    tensor_core::conformance::check_low_ops_backend(&rt)?;
    tensor_core::conformance::check_low_index_backend(&rt)?;
    tensor_core::conformance::check_low_scatter_backend(&rt)?;
    tensor_core::conformance::check_stats_backend(&rt)?;
    tensor_core::conformance::check_low_stats_backend(&rt)?;
    tensor_core::conformance::check_attention_backend(&rt)?;
    tensor_core::conformance::check_low_attention_backend(&rt)?;

    // Rank-one promotion preserves a contiguous view's nonzero offset and
    // materializes broadcast strides before cuBLAS reads its matrix operands.
    let vector = rt.narrow(
        &rt.upload_f32(shape(&[5]), &[99., 1.25, 2.5, -1., 99.])?,
        0,
        1,
        3,
    )?;
    let other_vector = rt.narrow(&rt.upload_f32(shape(&[4]), &[99., 2., 3., 4.])?, 0, 1, 3)?;
    let dot = rt.matmul(&vector, &other_vector, MatmulPrecision::F32)?;
    assert_eq!(dot.shape().dims(), &[]);
    assert_eq!(rt.read_f32(&dot)?, [6.]);
    let twos = rt.broadcast_to(&rt.upload_f32(shape(&[]), &[2.])?, shape(&[3]))?;
    assert_eq!(
        rt.read_f32(&rt.matmul(&vector, &twos, MatmulPrecision::F32)?)?,
        [5.5]
    );

    // Odd sizes, two reduction passes, negative values and a strided offset.
    let values: Vec<f32> = (0..1_000_003)
        .map(|i| ((i % 17) as f32 - 8.) * 0.25)
        .collect();
    let input = rt.upload_f32(shape(&[values.len()]), &values)?;
    let total = rt.sum_axes(&input, &[0], false)?;
    close(
        &rt.read_f32(&total)?,
        &[values.iter().map(|&x| f64::from(x)).sum()],
        0.,
        0.,
    );
    let values: Vec<f32> = (0..601 * 517).map(|i| (i % 31) as f32 * 0.25).collect();
    let matrix = rt.upload_f32(shape(&[601, 517]), &values)?;
    let slice = rt.narrow(&matrix, 1, 3, 511)?;
    let transposed = rt.permute(&slice, &[1, 0])?;
    let expected: Vec<f64> = (0..511)
        .map(|col| {
            (0..601)
                .map(|row| f64::from(values[row * 517 + col + 3]))
                .sum()
        })
        .collect();
    close(
        &rt.read_f32(&rt.sum_axes(&transposed, &[1], false)?)?,
        &expected,
        0.,
        0.,
    );
    let total = rt.sum_axes(&transposed, &[1, 0], true)?;
    assert_eq!(total.shape().dims(), &[1, 1]);
    close(&rt.read_f32(&total)?, &[expected.iter().sum()], 0., 0.);

    // View materialization and unary math must not lose signed zero.
    let zeros = rt.upload_f32(shape(&[2, 2]), &[-0., 3., -0., 7.])?;
    let reordered = rt.read_f32(&rt.materialize(&rt.permute(&zeros, &[1, 0])?)?)?;
    assert_eq!(reordered[0].to_bits(), (-0.0f32).to_bits());
    assert_eq!(reordered[1].to_bits(), (-0.0f32).to_bits());
    let positive_zero = rt.upload_f32(shape(&[]), &[0.])?;
    assert_eq!(
        rt.read_f32(&rt.unary(UnaryOp::Negate, &positive_zero)?)?[0].to_bits(),
        (-0.0f32).to_bits()
    );

    // Repeated writes are stream ordered; active views prevent hidden mutation.
    let mut updated = rt.upload_f32(shape(&[3]), &[1., 2., 3.])?;
    let old = rt.affine(&updated, 2., -1.)?;
    let alias = updated.clone();
    assert!(matches!(
        rt.write_f32(&mut updated, &[4., 5., 6.]),
        Err(CudaError::SharedOutput)
    ));
    drop(alias);
    rt.write_f32(&mut updated, &[4., 5., 6.])?;
    let new = rt.affine(&updated, 2., -1.)?;
    assert_eq!(rt.read_f32(&old)?, [1., 3., 5.]);
    assert_eq!(rt.read_f32(&new)?, [7., 9., 11.]);
    assert_eq!(rt.read_f32(&rt.dot(&updated, &updated)?)?, [77.]);
    let other = CudaRuntime::new()?;
    assert!(matches!(
        other.read_f32(&updated),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.binary(BinaryOp::Add, &updated, &updated),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(
        rt.broadcast_to(&positive_zero, shape(&[usize::MAX]))
            .is_err()
    );

    // A low mantissa bit that FP16/TF32 would drop must survive strict F32.
    let precise_a = rt.upload_f32(shape(&[1, 2]), &[1.000_244_1, -1.])?;
    let precise_b = rt.upload_f32(shape(&[2, 1]), &[1., 1.])?;
    assert_eq!(
        rt.read_f32(&rt.matmul(&precise_a, &precise_b, MatmulPrecision::F32)?)?,
        [0.000_244_140_63]
    );

    // Reduced precision is opt-in, tested against uploaded f32 values in f64.
    // These tolerances describe this bounded fixture, not a universal guarantee.
    let (m, k, n) = (33, 65, 31);
    let av: Vec<f32> = (0..m * k).map(|i| (i as f32 * 0.07).sin()).collect();
    let bv: Vec<f32> = (0..k * n).map(|i| (i as f32 * 0.11).cos()).collect();
    let a = rt.upload_f32(shape(&[m, k]), &av)?;
    let b = rt.upload_f32(shape(&[k, n]), &bv)?;
    let reference: Vec<f64> = (0..m * n)
        .map(|i| {
            (0..k)
                .map(|inner| f64::from(av[i / n * k + inner]) * f64::from(bv[inner * n + i % n]))
                .sum()
        })
        .collect();
    for (mode, atol, rtol) in [
        (MatmulPrecision::F32, 2e-5, 3e-6),
        (MatmulPrecision::AllowTf32, 0.02, 0.003),
        (MatmulPrecision::AllowF16, 0.02, 0.003),
        (MatmulPrecision::AllowBf16, 0.1, 0.02),
    ] {
        match rt.matmul_policy(mode) {
            Ok(_) => close(
                &rt.read_f32(&rt.matmul(&a, &b, mode)?)?,
                &reference,
                atol,
                rtol,
            ),
            Err(CudaError::UnsupportedPrecision(reason)) => {
                assert!(matches!(
                    rt.matmul(&a, &b, mode),
                    Err(CudaError::UnsupportedPrecision(_))
                ));
                eprintln!("Precision {mode:?} unsupported on this device: {reason}");
            }
            Err(error) => return Err(error),
        }
    }
    check_indexing_edges(&rt, &other)?;
    check_scatter_edges(&rt, &other)?;
    check_low_edges(&rt, &other)?;
    check_low_ops_edges(&rt, &other)?;
    check_low_indexing_edges(&rt, &other)?;
    check_low_scatter_edges(&rt, &other)?;
    check_statistics_edges(&rt, &other)?;
    check_low_statistics_edges(&rt, &other)?;
    check_attention_edges(&rt, &other)?;
    check_low_attention_edges(&rt, &other)?;
    rt.synchronize()
}

fn check_low_statistics_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        // Offset and transposed native16 storage, with enough output rows to
        // exercise bounded-grid reuse. Every selected integer is exactly low.
        let rows = 2053;
        let values: Vec<f32> = (0..rows * 9)
            .map(|i| {
                if i % 9 == 0 || i % 9 == 8 {
                    99.
                } else {
                    (i / 9 % 16) as f32 + (i % 9) as f32 - 4.
                }
            })
            .collect();
        let backing = rt.cast_to_low(&rt.upload_f32(shape(&[rows, 9]), &values)?, dtype)?;
        let before = rt.read_low_bits(&backing)?;
        let input = rt.permute_low(&rt.narrow_low(&backing, 1, 1, 7)?, &[1, 0])?;
        for _ in 0..2 {
            let moments = rt.moments_low_f32(&input, &[0], false)?;
            assert_eq!(
                rt.read_f32(&moments.mean)?,
                (0..rows).map(|i| (i % 16) as f32).collect::<Vec<_>>()
            );
            assert_eq!(rt.read_f32(&moments.variance)?, vec![4.; rows]);
            let normalized = rt.layer_norm_low_f32(&input, &[0], 0.25)?;
            let expected: Vec<f64> = (0..7 * rows)
                .map(|i| (i / rows) as f64 - 3.)
                .map(|x| x / 4.25f64.sqrt())
                .collect();
            close(&rt.read_f32(&normalized)?, &expected, 1e-7, 2e-6);
            let probabilities = rt.softmax_low_f32(&input, &[0])?;
            let sums = rt.sum_axes(&probabilities, &[0], false)?;
            close(&rt.read_f32(&sums)?, &vec![1.; rows], 2e-6, 2e-6);
        }
        assert_eq!(rt.read_low_bits(&backing)?, before);

        // Singleton identities preserve finite bits, including a nonzero offset
        // into signed zeros and the smallest native subnormal of each format.
        let raw = [0x7fffu16, 0x8000, 1, 0x8001, 0, 0x7fff];
        let raw = rt.upload_low(dtype, shape(&[raw.len()]), &raw)?;
        let singleton = rt.narrow_low(&raw, 0, 1, 4)?;
        let singleton = rt.reshape_low(&singleton, shape(&[4, 1]))?;
        let widened = rt.read_f32(&rt.cast_to_f32(&singleton)?)?;
        for axes in [&[][..], &[1][..]] {
            let lse = rt.read_f32(&rt.logsumexp_low_f32(&singleton, axes, false)?)?;
            let moments = rt.moments_low_f32(&singleton, axes, false)?;
            let mean = rt.read_f32(&moments.mean)?;
            assert_eq!(
                lse.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                widened.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(
                mean.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                widened.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(
                rt.read_low_bits(&rt.logsumexp_low(&singleton, axes, false)?)?,
                [0x8000, 1, 0x8001, 0]
            );
        }
        let foreign = other.upload_low(dtype, shape(&[2]), &[0, 1])?;
        assert!(rt.softmax_low_f32(&foreign, &[0]).is_err());
        assert!(rt.log_softmax_low_f32(&foreign, &[0]).is_err());
        assert!(rt.logsumexp_low_f32(&foreign, &[0], false).is_err());
        assert!(rt.moments_low_f32(&foreign, &[0], false).is_err());
        assert!(rt.layer_norm_low_f32(&foreign, &[0], 1e-5).is_err());
    }
    Ok(())
}

fn check_low_scatter_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let (rows, width, count) = (3, 257, 65_539);
        let backing_bits: Vec<u16> = (0..rows * (width + 2)).map(|i| (i * 137) as u16).collect();
        let backing = rt.upload_low(dtype, shape(&[rows, width + 2]), &backing_bits)?;
        let base = rt.narrow_low(&backing, 1, 1, width)?;
        let ids: Vec<u32> = (0..count)
            .map(|i| {
                if i % 17 == 0 {
                    u32::MAX
                } else {
                    (i % width) as u32
                }
            })
            .collect();
        let indices_storage: Vec<u32> = ids.iter().flat_map(|&id| [u32::MAX, id]).collect();
        let indices_storage = rt.upload_u32(shape(&[count, 2]), &indices_storage)?;
        let indices = rt.narrow(&indices_storage, 1, 1, 1)?;
        let physical: Vec<u16> = (0..count * rows).map(|i| (i * 73 + 19) as u16).collect();
        let updates_storage = rt.upload_low(dtype, shape(&[count, rows, 1]), &physical)?;
        let updates = rt.permute_low(&updates_storage, &[1, 0, 2])?;
        let mut expected: Vec<u16> = (0..rows)
            .flat_map(|row| {
                let backing_bits = &backing_bits;
                (0..width).map(move |col| backing_bits[row * (width + 2) + col + 1])
            })
            .collect();
        for (j, &id) in ids.iter().enumerate() {
            if (id as usize) < width {
                for row in 0..rows {
                    expected[row * width + id as usize] = physical[j * rows + row];
                }
            }
        }
        for _ in 0..3 {
            let result = rt.scatter_low(ScatterOp::Replace, &base, &indices, &updates, 1)?;
            assert_eq!(rt.read_low_bits(&result.values)?, expected);
            assert_eq!(result.values.storage_bytes(), (rows * width + 1) * 2);
            assert_eq!(
                rt.read_u32(&result.invalid_count)?,
                [count.div_ceil(17) as u32]
            );
        }
        assert_eq!(rt.read_low_bits(&backing)?, backing_bits);
        assert_eq!(rt.read_low_bits(&updates_storage)?, physical);

        // Both updates and base may alias, but the output copy must precede
        // scatter writes so the original strided storage remains unchanged.
        let reverse = rt.upload_u32(
            shape(&[width]),
            &(0..width as u32).rev().collect::<Vec<_>>(),
        )?;
        let alias = rt.scatter_low(ScatterOp::Replace, &base, &reverse, &base, 1)?;
        let expected: Vec<u16> = (0..rows)
            .flat_map(|row| {
                let backing_bits = &backing_bits;
                (0..width).map(move |col| backing_bits[row * (width + 2) + width - col])
            })
            .collect();
        assert_eq!(rt.read_low_bits(&alias.values)?, expected);

        // Contended tiny updates must accumulate in f32, not repeatedly round
        // in low storage; BF16's smallest values are f32 subnormals.
        let tiny = match dtype {
            LowDtype::F16 => f32::from_bits(0x3380_0000),
            LowDtype::Bf16 => f32::from_bits(0x0001_0000),
        };
        let base = rt.upload_low(dtype, shape(&[3]), &[0, 0, 0])?;
        let tiny_update = rt.upload_low(dtype, shape(&[]), &[1])?;
        let ids: Vec<u32> = (0..count).map(|i| (i % 3) as u32).collect();
        let indices = rt.upload_u32(shape(&[count]), &ids)?;
        let expected: Vec<f32> = (0..3)
            .map(|slot| {
                (f64::from(tiny) * ids.iter().filter(|&&i| i == slot).count() as f64) as f32
            })
            .collect();
        let f32_result = rt.scatter_low_f32(ScatterOp::Add, &base, &indices, &tiny_update, 0)?;
        assert_eq!(rt.read_f32(&f32_result.values)?, expected);
        let low_result = rt.scatter_low(ScatterOp::Add, &base, &indices, &tiny_update, 0)?;
        let expected_low = rt.cast_to_low(&rt.upload_f32(shape(&[3]), &expected)?, dtype)?;
        assert_eq!(
            rt.read_low_bits(&low_result.values)?,
            rt.read_low_bits(&expected_low)?
        );
        assert_eq!(rt.read_u32(&low_result.invalid_count)?, [0]);

        let foreign = other.upload_low(dtype, shape(&[]), &[1])?;
        let foreign_indices = other.upload_u32(shape(&[]), &[0])?;
        assert!(matches!(
            rt.scatter_low(ScatterOp::Replace, &base, &indices, &foreign, 0),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.scatter_low_f32(ScatterOp::Add, &base, &foreign_indices, &tiny_update, 0),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            other.scatter_low(ScatterOp::Min, &base, &indices, &tiny_update, 0),
            Err(CudaError::ForeignRuntime)
        ));
        let opposite = rt.upload_low(
            if dtype == LowDtype::F16 {
                LowDtype::Bf16
            } else {
                LowDtype::F16
            },
            shape(&[]),
            &[1],
        )?;
        assert!(matches!(
            rt.scatter_low(ScatterOp::Replace, &base, &indices, &opposite, 0),
            Err(CudaError::Contract(
                tensor_core::TensorError::LowDtypeMismatch { .. }
            ))
        ));
        assert!(matches!(
            rt.scatter_low_f32(ScatterOp::Add, &base, &indices, &opposite, 0),
            Err(CudaError::Contract(
                tensor_core::TensorError::LowDtypeMismatch { .. }
            ))
        ));
    }
    Ok(())
}

fn check_low_indexing_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        // Nonzero native16 offset through three scan hierarchy levels. Every
        // prefix is a small binary fraction, so f32 accumulation must be exact.
        let n = 131_077;
        let positive = match dtype {
            LowDtype::F16 => 0x3800,
            LowDtype::Bf16 => 0x3f00,
        };
        let mut bits = vec![0x7fff; n + 2];
        for (i, bit) in bits[1..=n].iter_mut().enumerate() {
            *bit = positive | if i % 2 == 0 { 0 } else { 0x8000 };
        }
        let backing = rt.upload_low(dtype, shape(&[n + 2]), &bits)?;
        let input = rt.narrow_low(&backing, 0, 1, n)?;
        for reverse in [false, true] {
            for inclusive in [false, true] {
                let options = ScanOptions { inclusive, reverse };
                let mut expected = vec![0f32; n];
                let mut sum = 0.;
                for position in 0..n {
                    let i = if reverse { n - 1 - position } else { position };
                    let value = if i % 2 == 0 { 0.5 } else { -0.5 };
                    if inclusive {
                        sum += value;
                    }
                    expected[i] = sum;
                    if !inclusive {
                        sum += value;
                    }
                }
                assert_eq!(
                    rt.read_f32(&rt.scan_low_f32(&input, 0, options)?)?,
                    expected
                );
                let expected_bits = rt.read_low_bits(
                    &rt.cast_to_low(&rt.upload_f32(shape(&[n]), &expected)?, dtype)?,
                )?;
                assert_eq!(
                    rt.read_low_bits(&rt.scan_low(&input, 0, options)?)?,
                    expected_bits
                );
            }
        }
        assert_eq!(rt.read_low_bits(&backing)?, bits);

        // Raw movement includes every payload, nonzero offsets, transposed
        // strides and changing mask selectivity across repeated launches.
        let rows = 43_691;
        let raw: Vec<u16> = (0..3 * (rows + 2)).map(|i| i as u16).collect();
        let storage = rt.upload_low(dtype, shape(&[3, rows + 2]), &raw)?;
        let sliced = rt.narrow_low(&storage, 1, 1, rows)?;
        let source = rt.permute_low(&sliced, &[1, 0])?;
        let mut mask = rt.upload_u32(shape(&[rows, 1]), &vec![0; rows])?;
        for mode in [2, 1, 0] {
            let flags: Vec<u32> = (0..rows)
                .map(|i| {
                    if mode == 2 || (mode == 1 && i % 3 == 0) {
                        u32::MAX
                    } else {
                        0
                    }
                })
                .collect();
            rt.write_u32(&mut mask, &flags)?;
            let compacted = rt.compact_low(&source, &mask)?;
            let mut expected = Vec::new();
            for (row, &flag) in flags.iter().enumerate() {
                if flag != 0 {
                    expected.extend((0..3).map(|col| raw[col * (rows + 2) + row + 1]));
                }
            }
            assert_eq!(rt.read_u32(&compacted.count)?, [expected.len() as u32]);
            expected.resize(rows * 3, 0);
            assert_eq!(rt.read_low_bits(&compacted.values)?, expected);
        }
        let index_bits: Vec<u32> = (0..1003)
            .map(|i| if i % 2 == 0 { 1 } else { u32::MAX })
            .collect();
        let indices = rt.upload_u32(shape(&[index_bits.len()]), &index_bits)?;
        let gathered = rt.gather_low(&source, &indices, 0)?;
        assert_eq!(rt.read_u32(&gathered.invalid_count)?, [501]);
        let expected: Vec<u16> = index_bits
            .iter()
            .flat_map(|&index| {
                let raw = &raw;
                (0..3).map(move |col| {
                    if index == 1 {
                        raw[col * (rows + 2) + 2]
                    } else {
                        0
                    }
                })
            })
            .collect();
        assert_eq!(rt.read_low_bits(&gathered.values)?, expected);

        let scalar = rt.upload_low(dtype, shape(&[]), &[positive])?;
        let scalar_mask = rt.upload_u32(shape(&[]), &[7])?;
        let foreign = other.upload_low(dtype, shape(&[]), &[positive])?;
        let foreign_mask = other.upload_u32(shape(&[]), &[0])?;
        assert!(matches!(
            rt.compare_low(CompareOp::Equal, &scalar, &foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.select_low(&scalar_mask, &foreign, &scalar),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.select_low(&foreign_mask, &scalar, &scalar),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.gather_low(&source, &foreign_mask, 0),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            other.gather_low(&source, &indices, 0),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.compact_low(&source, &foreign_mask),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            other.scan_low_f32(&input, 0, ScanOptions::default()),
            Err(CudaError::ForeignRuntime)
        ));
        let opposite_dtype = if dtype == LowDtype::F16 {
            LowDtype::Bf16
        } else {
            LowDtype::F16
        };
        let opposite = rt.upload_low(opposite_dtype, shape(&[]), &[positive])?;
        assert!(matches!(
            rt.compare_low(CompareOp::Equal, &scalar, &opposite),
            Err(CudaError::Contract(
                tensor_core::TensorError::LowDtypeMismatch { .. }
            ))
        ));
        assert!(matches!(
            rt.select_low(&scalar_mask, &scalar, &opposite),
            Err(CudaError::Contract(
                tensor_core::TensorError::LowDtypeMismatch { .. }
            ))
        ));
    }
    Ok(())
}

fn check_low_ops_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 131_077;
        let mut bits = vec![1u16; n + 2];
        bits[258] = 0x8001;
        bits[n] = 2;
        let backing = rt.upload_low(dtype, shape(&[n + 2]), &bits)?;
        let input = rt.narrow_low(&backing, 0, 1, n)?;
        let min = rt.reduce_low_f32(ReduceOp::Min, &input, &[0], false)?;
        let max = rt.reduce_low_f32(ReduceOp::Max, &input, &[0], true)?;
        let absolute = rt.unary_low(UnaryOp::Abs, &input)?;
        let resident_min = rt.reduce_low(ReduceOp::Min, &absolute, &[0], false)?;
        let (min_bits, max_bits) = match dtype {
            LowDtype::F16 => (0xb380_0000, 0x3400_0000),
            LowDtype::Bf16 => (0x8001_0000, 0x0002_0000),
        };
        assert_eq!(rt.read_f32(&min)?[0].to_bits(), min_bits);
        assert_eq!(rt.read_f32(&max)?[0].to_bits(), max_bits);
        assert_eq!(rt.read_low_bits(&resident_min)?, [1]);
        assert_eq!(rt.read_low_bits(&backing)?, bits);
        let foreign = other.upload_low(dtype, shape(&[]), &[1])?;
        assert!(matches!(
            rt.unary_low(UnaryOp::Abs, &foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.binary_low(BinaryOp::Add, &input, &foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.reduce_low_f32(ReduceOp::Min, &foreign, &[], false),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.mean_low_f32(&foreign, &[], false),
            Err(CudaError::ForeignRuntime)
        ));
    }
    let left = rt.upload_low(LowDtype::F16, shape(&[0]), &[])?;
    let right = rt.upload_low(LowDtype::Bf16, shape(&[0]), &[])?;
    assert!(matches!(
        rt.binary_low(BinaryOp::Add, &left, &right),
        Err(CudaError::Contract(
            tensor_core::TensorError::LowDtypeMismatch { .. }
        ))
    ));
    Ok(())
}

fn check_low_attention_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let opposite = if dtype == LowDtype::F16 {
            LowDtype::Bf16
        } else {
            LowDtype::F16
        };
        // Nonzero offsets and transposed strides for every low operand, a
        // partial feature warp, three key tiles and more V columns than lanes.
        let q_back = rt.upload_low(dtype, shape(&[35, 7]), &[0; 35 * 7])?;
        let k_back = rt.upload_low(dtype, shape(&[35, 69]), &[0; 35 * 69])?;
        let q = rt.permute_low(&rt.narrow_low(&q_back, 1, 1, 5)?, &[1, 0])?;
        let k = rt.permute_low(&rt.narrow_low(&k_back, 1, 1, 67)?, &[1, 0])?;
        let source: Vec<f32> = (0..263 * 69).map(|i| (i % 19) as f32 - 9.).collect();
        let v_back = rt.cast_to_low(&rt.upload_f32(shape(&[263, 69]), &source)?, dtype)?;
        let before = rt.read_low_bits(&v_back)?;
        let v = rt.permute_low(&rt.narrow_low(&v_back, 1, 1, 67)?, &[1, 0])?;
        let visible = |row: usize, key: usize| row > 0 && key >= 33 && key % 3 != 1;
        let mask: Vec<u32> = (0..5 * 69)
            .map(|i| u32::from(i % 69 > 0 && visible(i / 69, i % 69 - 1)))
            .collect();
        let keep = rt.narrow(&rt.upload_u32(shape(&[5, 69]), &mask)?, 1, 1, 67)?;
        let additive_values: Vec<f32> = mask
            .iter()
            .map(|&m| if m == 0 { f32::NEG_INFINITY } else { 0. })
            .collect();
        let additive = rt.narrow(&rt.upload_f32(shape(&[5, 69]), &additive_values)?, 1, 1, 67)?;
        let options = AttentionOptions {
            scale: Some(-0.5),
            causal: Some(40),
        };
        let expected: Vec<f64> = (0..5 * 263)
            .map(|i| {
                let row = i / 263;
                let keys: Vec<_> = (0..67)
                    .filter(|&j| visible(row, j) && j <= row + 40)
                    .collect();
                if keys.is_empty() {
                    0.
                } else {
                    keys.iter()
                        .map(|&j| f64::from(source[(i % 263) * 69 + j + 1]))
                        .sum::<f64>()
                        / keys.len() as f64
                }
            })
            .collect();
        for _ in 0..2 {
            for mask in [
                AttentionMask::Keep(&keep),
                AttentionMask::Additive(&additive),
            ] {
                let result = rt.attention_low_f32(&q, &k, &v, mask, options)?;
                close(&rt.read_f32(&result)?, &expected, 1e-6, 2e-6);
            }
        }
        assert_eq!(rt.read_low_bits(&v_back)?, before);
        assert_eq!(rt.read_low_bits(&q_back)?, vec![0; 35 * 7]);
        assert_eq!(rt.read_low_bits(&k_back)?, vec![0; 35 * 69]);

        // A tiny native operand times a large partner produces normal logits
        // in both Q/K orders. Wider loader arithmetic must retain the product.
        let last = if dtype == LowDtype::F16 {
            0x7bff
        } else {
            0x7f7f
        };
        let one = if dtype == LowDtype::F16 {
            0x3c00
        } else {
            0x3f80
        };
        let tiny_value = if dtype == LowDtype::F16 {
            2f64.powi(-24)
        } else {
            2f64.powi(-133)
        };
        let max_value = if dtype == LowDtype::F16 {
            65504.
        } else {
            f64::from(f32::from_bits(0x7f7f0000))
        };
        let logits = tiny_value * max_value;
        let probability = logits.exp() / (1. + logits.exp());
        let values = rt.upload_low(dtype, shape(&[2, 1]), &[0, one])?;
        for (qb, kb) in [(1, last), (last, 1)] {
            let query = rt.upload_low(dtype, shape(&[1, 1]), &[qb])?;
            let key = rt.upload_low(dtype, shape(&[2, 1]), &[0, kb])?;
            let result = rt.attention_low_f32(
                &query,
                &key,
                &values,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(1.),
                    causal: None,
                },
            )?;
            close(&rt.read_f32(&result)?, &[probability], 1e-7, 1e-7);
        }
        let constant = rt.upload_low(dtype, shape(&[67, 1]), &[last; 67])?;
        let result = rt.attention_low_f32(
            &q,
            &k,
            &constant,
            AttentionMask::None,
            AttentionOptions::default(),
        )?;
        assert_eq!(rt.read_f32(&result)?, vec![max_value as f32; 5]);
        assert_eq!(
            rt.read_low_bits(&rt.attention_low(
                &q,
                &k,
                &constant,
                AttentionMask::None,
                AttentionOptions::default()
            )?)?,
            vec![last; 5]
        );

        let rows = rt.capabilities().multiprocessors as usize * 8 + 17;
        let many = rt.upload_low(dtype, shape(&[rows, 1]), &vec![one; rows])?;
        let single = rt.upload_low(dtype, shape(&[1, 1]), &[one])?;
        let result = rt.attention_low_f32(
            &many,
            &single,
            &single,
            AttentionMask::None,
            AttentionOptions::default(),
        )?;
        assert_eq!(rt.read_f32(&result)?, vec![1.; rows]);
        let foreign = other.upload_low(dtype, shape(&[1, 1]), &[one])?;
        for (q, k, v) in [
            (&foreign, &single, &single),
            (&single, &foreign, &single),
            (&single, &single, &foreign),
        ] {
            assert!(matches!(
                rt.attention_low_f32(q, k, v, AttentionMask::None, AttentionOptions::default()),
                Err(CudaError::ForeignRuntime)
            ));
        }
        let foreign_keep = other.upload_u32(shape(&[]), &[1])?;
        let foreign_additive = other.upload_f32(shape(&[]), &[0.])?;
        for mask in [
            AttentionMask::Keep(&foreign_keep),
            AttentionMask::Additive(&foreign_additive),
        ] {
            assert!(matches!(
                rt.attention_low_f32(&single, &single, &single, mask, AttentionOptions::default()),
                Err(CudaError::ForeignRuntime)
            ));
        }
        let empty = rt.upload_low(dtype, shape(&[0, 1]), &[])?;
        let different = rt.upload_low(opposite, shape(&[0, 1]), &[])?;
        for (q, k, v) in [
            (&different, &empty, &empty),
            (&empty, &different, &empty),
            (&empty, &empty, &different),
        ] {
            assert!(matches!(
                rt.attention_low_f32(q, k, v, AttentionMask::None, AttentionOptions::default()),
                Err(CudaError::Contract(
                    tensor_core::TensorError::LowDtypeMismatch { .. }
                ))
            ));
            assert!(
                rt.attention_low(q, k, v, AttentionMask::None, AttentionOptions::default())
                    .is_err()
            );
        }
    }
    Ok(())
}

fn check_attention_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    // Three key tiles, a partial warp depth, and more V components than lanes.
    let query = rt.upload_f32(shape(&[5, 35]), &[0.; 175])?;
    let key = rt.upload_f32(shape(&[67, 35]), &vec![0.; 67 * 35])?;
    let values: Vec<f32> = (0..67 * 263).map(|i| (i % 19) as f32 - 9.).collect();
    let mut value = rt.upload_f32(shape(&[67, 263]), &values)?;
    let expected: Vec<f64> = (0..5 * 263)
        .map(|i| {
            (0..67)
                .map(|k| f64::from(values[k * 263 + i % 263]))
                .sum::<f64>()
                / 67.
        })
        .collect();
    let first = rt.attention(
        &query,
        &key,
        &value,
        AttentionMask::None,
        AttentionOptions::default(),
    )?;
    close(&rt.read_f32(&first)?, &expected, 1e-6, 1e-6);
    rt.write_f32(&mut value, &vec![3.; values.len()])?;
    let causal = rt.attention(
        &query,
        &key,
        &value,
        AttentionMask::None,
        AttentionOptions {
            scale: Some(-0.5),
            causal: Some(-2),
        },
    )?;
    let expected: Vec<f64> = (0..5 * 263)
        .map(|i| if i / 263 < 2 { 0. } else { 3. })
        .collect();
    close(&rt.read_f32(&causal)?, &expected, 1e-6, 1e-6);
    assert_eq!(rt.read_f32(&query)?, [0.; 175]);

    // Exercise query-row grid reuse; each one-key row has exactly the same V.
    let rows = rt.capabilities().multiprocessors as usize * 8 + 17;
    let many = rt.upload_f32(shape(&[rows, 1]), &vec![1.; rows])?;
    let single = rt.upload_f32(shape(&[1, 1]), &[4.])?;
    let repeated = rt.attention(
        &many,
        &single,
        &single,
        AttentionMask::None,
        AttentionOptions::default(),
    )?;
    assert_eq!(rt.read_f32(&repeated)?, vec![4.; rows]);

    let foreign = other.upload_f32(shape(&[1, 1]), &[1.])?;
    for (q, k, v) in [
        (&foreign, &single, &single),
        (&single, &foreign, &single),
        (&single, &single, &foreign),
    ] {
        assert!(matches!(
            rt.attention(q, k, v, AttentionMask::None, AttentionOptions::default()),
            Err(CudaError::ForeignRuntime)
        ));
    }
    let foreign_keep = other.upload_u32(shape(&[]), &[1])?;
    assert!(matches!(
        rt.attention(
            &single,
            &single,
            &single,
            AttentionMask::Keep(&foreign_keep),
            AttentionOptions::default()
        ),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        rt.attention(
            &single,
            &single,
            &single,
            AttentionMask::Additive(&foreign),
            AttentionOptions::default()
        ),
        Err(CudaError::ForeignRuntime)
    ));
    Ok(())
}

fn check_statistics_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    // More rows than a typical bounded grid, with an offset, transpose and a
    // large common offset. Every row has mean=base and population variance1536.
    let rows = 2051;
    let values: Vec<f32> = (0..rows * 19)
        .map(|i| {
            let row = i / 19;
            let column = i % 19;
            if column == 0 || column == 18 {
                99.
            } else {
                1e8 + (row % 5) as f32 * 64. + (column as f32 - 9.) * 8.
            }
        })
        .collect();
    let backing = rt.upload_f32(shape(&[rows, 19]), &values)?;
    let view = rt.permute(&rt.narrow(&backing, 1, 1, 17)?, &[1, 0])?;
    let moments = rt.moments(&view, &[0], false)?;
    let normalized = rt.layer_norm(&view, &[0], 0.25)?;
    let probabilities = rt.softmax(&normalized, &[0])?;
    let totals = rt.sum_axes(&probabilities, &[0], false)?;
    // All statistics and this composition are enqueued before the first read.
    close(
        &rt.read_f32(&moments.mean)?,
        &(0..rows)
            .map(|r| 1e8 + (r % 5) as f64 * 64.)
            .collect::<Vec<_>>(),
        0.,
        0.,
    );
    close(&rt.read_f32(&moments.variance)?, &vec![1536.; rows], 0., 0.);
    close(&rt.read_f32(&totals)?, &vec![1.; rows], 2e-6, 0.);
    let expected: Vec<f64> = (0..17)
        .flat_map(|j| std::iter::repeat_n((j as f64 - 8.) * 8. / 1536.25f64.sqrt(), rows))
        .collect();
    close(&rt.read_f32(&normalized)?, &expected, 2e-7, 2e-7);
    assert_eq!(rt.read_f32(&backing)?, values);
    assert!(matches!(
        other.softmax(&view, &[0]),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.log_softmax(&view, &[0]),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.logsumexp(&view, &[0], false),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.moments(&view, &[0], false),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.layer_norm(&view, &[0], 0.25),
        Err(CudaError::ForeignRuntime)
    ));
    Ok(())
}

fn check_low_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let support = rt.low_precision_support(dtype);
        eprintln!("native CUDA {dtype:?}: {support:?}");
        assert_eq!(support.storage, LowStorage::Native16);
        for n in [0, 1, 3, 65_537] {
            let input = rt.upload_low(dtype, shape(&[n]), &vec![0; n])?;
            assert_eq!(input.storage_bytes(), n.max(1) * 2);
        }
        // Odd grid-stride tail and signed values, all exactly representable in
        // both low formats. Conversion is entirely on the selected device.
        let n = 1_048_583;
        let values: Vec<f32> = (0..n).map(|i| ((i % 31) as f32 - 15.) * 0.25).collect();
        let input = rt.upload_f32(shape(&[n]), &values)?;
        let low = rt.cast_to_low(&input, dtype)?;
        assert_eq!(low.storage_bytes(), 2 * n);
        assert_eq!(rt.read_f32(&rt.cast_to_f32(&low)?)?, values);

        // Two-byte offsets are preserved when vectors become GEMM matrices.
        // Strided materialization and raw reads must retain the original bits.
        let source =
            rt.cast_to_low(&rt.upload_f32(shape(&[5]), &[99., 1., 2., 3., 99.])?, dtype)?;
        let vector = rt.narrow_low(&source, 0, 1, 3)?;
        assert_eq!(vector.layout().offset(), 1);
        assert_eq!(vector.storage_bytes(), source.storage_bytes());
        assert_eq!(rt.read_f32(&rt.cast_to_f32(&vector)?)?, [1., 2., 3.]);
        let other_vector = rt.cast_to_low(&rt.upload_f32(shape(&[3]), &[4., 5., 6.])?, dtype)?;
        if support.matmul_f32 {
            assert_eq!(
                rt.read_f32(&rt.matmul_low_f32(&vector, &other_vector)?)?,
                [32.]
            );
            let low_output = rt.matmul_low(&vector, &other_vector)?;
            assert_eq!(low_output.storage_bytes(), 2);
            assert_eq!(rt.read_f32(&rt.cast_to_f32(&low_output)?)?, [32.]);
        }
        let foreign = other.upload_low(dtype, shape(&[3]), &[0; 3])?;
        assert!(matches!(
            rt.read_low_bits(&foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.materialize_low(&foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.cast_to_f32(&foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            other.cast_to_low(&input, dtype),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.matmul_low_f32(&vector, &foreign),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            rt.matmul_low(&foreign, &vector),
            Err(CudaError::ForeignRuntime)
        ));
        // The low view fits its two-byte limit. Widening it must reject the
        // f32 byte-count overflow before allocating or launching anything.
        let scalar = rt.upload_low(dtype, shape(&[]), &[0])?;
        let huge = rt.broadcast_low(&scalar, shape(&[usize::MAX / 2]))?;
        assert!(matches!(
            rt.cast_to_f32(&huge),
            Err(CudaError::InvalidInput(_))
        ));
        assert!(rt.broadcast_low(&scalar, shape(&[usize::MAX])).is_err());
    }
    Ok(())
}

fn check_scatter_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    // Input and updates can be the same allocation; scatter must return fresh
    // storage, including when the base was already contiguous.
    let base = rt.upload_f32(shape(&[2]), &[2., 3.])?;
    let indices = rt.upload_u32(shape(&[2]), &[1, 0])?;
    let result = rt.scatter_f32(ScatterOp::Add, &base, &indices, &base, 0)?;
    assert_eq!(rt.read_f32(&result.values)?, [5., 5.]);
    assert_eq!(rt.read_f32(&base)?, [2., 3.]);
    let ints = rt.upload_u32(shape(&[2]), &[u32::MAX, 3])?;
    let result = rt.scatter_u32(ScatterOp::Multiply, &ints, &indices, &ints, 0)?;
    assert_eq!(rt.read_u32(&result.values)?, [u32::MAX - 2; 2]);
    assert_eq!(rt.read_u32(&ints)?, [u32::MAX, 3]);

    // Global native float atomicAdd would flush these subnormals. The CAS
    // fold uses precise f32 add with --ftz=false instead. Exact bit assertions
    // are CUDA-specific; this fixture requires execution on NVIDIA hardware.
    let tiny = f32::from_bits(1);
    let base_tiny = rt.upload_f32(shape(&[1]), &[tiny])?;
    let updates = rt.upload_f32(shape(&[]), &[tiny])?;
    let repeated = rt.upload_u32(shape(&[513]), &vec![0; 513])?;
    let result = rt.scatter_f32(ScatterOp::Add, &base_tiny, &repeated, &updates, 0)?;
    assert_eq!(rt.read_f32(&result.values)?[0].to_bits(), 514);

    // Multi-block contention for every fold, with exact representable results.
    // Invalid entries are counted once despite two prefix replicas.
    let n = 65_539;
    let index_values: Vec<u32> = (0..n)
        .map(|i| if i % 5 == 0 { u32::MAX } else { 1 })
        .collect();
    let indices_many = rt.upload_u32(shape(&[n]), &index_values)?;
    let base = rt.upload_f32(shape(&[2, 3]), &[8., 2., -8., 16., -2., -16.])?;
    for op in [
        ScatterOp::Replace,
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        let values: Vec<f32> = (0..n)
            .map(|i| {
                if op == ScatterOp::Multiply {
                    if i % 2 == 0 { 1. } else { -1. }
                } else {
                    ((i % 7) as f32 - 3.) * 0.25
                }
            })
            .collect();
        let mut expected = [8., 2., -8., 16., -2., -16.];
        for (j, &idx) in index_values.iter().enumerate() {
            if idx < 3 {
                for row in 0..2 {
                    let cell = &mut expected[row * 3 + idx as usize];
                    *cell = match op {
                        ScatterOp::Replace => f64::from(values[j]),
                        ScatterOp::Add => *cell + f64::from(values[j]),
                        ScatterOp::Multiply => *cell * f64::from(values[j]),
                        ScatterOp::Min => cell.min(f64::from(values[j])),
                        ScatterOp::Max => cell.max(f64::from(values[j])),
                    };
                }
            }
        }
        let updates = rt.upload_f32(shape(&[n]), &values)?;
        for _ in 0..3 {
            let result = rt.scatter_f32(op, &base, &indices_many, &updates, 1)?;
            close(&rt.read_f32(&result.values)?, &expected, 0., 0.);
            assert_eq!(rt.read_u32(&result.invalid_count)?, [n.div_ceil(5) as u32]);
        }
    }
    assert_eq!(rt.read_f32(&base)?, [8., 2., -8., 16., -2., -16.]);

    let foreign = other.upload_u32(shape(&[2]), &[0, 1])?;
    assert!(matches!(
        rt.scatter_u32(ScatterOp::Add, &ints, &foreign, &ints, 0),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        rt.scatter_u32(ScatterOp::Add, &ints, &indices, &foreign, 0),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        rt.scatter_u32(ScatterOp::Add, &foreign, &indices, &ints, 0),
        Err(CudaError::ForeignRuntime)
    ));
    if usize::BITS > 32 {
        let scalar = rt.upload_u32(shape(&[]), &[0])?;
        let huge = rt.broadcast_u32(&scalar, shape(&[u32::MAX as usize + 1]))?;
        assert!(matches!(
            rt.scatter_u32(ScatterOp::Add, &ints, &huge, &scalar, 0),
            Err(CudaError::Contract(
                tensor_core::TensorError::IndexCountOverflow { .. }
            ))
        ));
    }
    Ok(())
}

fn check_indexing_edges(rt: &CudaRuntime, other: &CudaRuntime) -> Result<(), CudaError> {
    // Deep f32 hierarchy, both traversal directions, exact binary fractions.
    let values: Vec<f32> = (0..262_147)
        .map(|i| ((i % 17) as f32 - 8.) * 0.25)
        .collect();
    let input = rt.upload_f32(shape(&[values.len()]), &values)?;
    for reverse in [false, true] {
        for inclusive in [false, true] {
            let mut expected = vec![0.; values.len()];
            let mut sum = 0f64;
            for step in 0..values.len() {
                let i = if reverse {
                    values.len() - 1 - step
                } else {
                    step
                };
                if inclusive {
                    sum += f64::from(values[i]);
                }
                expected[i] = sum;
                if !inclusive {
                    sum += f64::from(values[i]);
                }
            }
            close(
                &rt.read_f32(&rt.scan_f32(&input, 0, ScanOptions { inclusive, reverse })?)?,
                &expected,
                0.,
                0.,
            );
        }
    }

    // Large strided compaction with broadcast nonbinary masks, repeated writes,
    // both extremes of selectivity, exact u32 values and checked zero tails.
    let rows = 87_383;
    let source_values: Vec<u32> = (0..3 * rows).map(|i| u32::MAX - i as u32).collect();
    let source = rt.permute_u32(&rt.upload_u32(shape(&[3, rows]), &source_values)?, &[1, 0])?;
    let mut mask = rt.upload_u32(shape(&[rows, 1]), &vec![0; rows])?;
    for mode in [2, 1, 0] {
        let flags: Vec<u32> = (0..rows)
            .map(|i| {
                if mode == 2 || (mode == 1 && i % 2 == 0) {
                    u32::MAX
                } else {
                    0
                }
            })
            .collect();
        rt.write_u32(&mut mask, &flags)?;
        let compacted = rt.compact_u32(&source, &mask)?;
        let mut expected = Vec::new();
        for (row, &flag) in flags.iter().enumerate() {
            if flag != 0 {
                expected.extend((0..3).map(|col| source_values[col * rows + row]));
            }
        }
        assert_eq!(rt.read_u32(&compacted.count)?, [expected.len() as u32]);
        expected.resize(source_values.len(), 0);
        assert_eq!(rt.read_u32(&compacted.values)?, expected);
    }
    let alias = mask.clone();
    assert!(matches!(
        rt.write_u32(&mut mask, &vec![0; rows]),
        Err(CudaError::SharedOutput)
    ));
    drop(alias);
    assert!(matches!(
        other.read_u32(&mask),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.scan_u32(&mask, 0, ScanOptions::default()),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        other.compact_u32(&source, &mask),
        Err(CudaError::ForeignRuntime)
    ));

    // Invalid counts are computed once per index, even with repeated output
    // copies, and exceed a workgroup's local 256-lane count.
    let index_values: Vec<u32> = (0..1_000_003)
        .map(|i| if i % 2 == 0 { 1 } else { u32::MAX })
        .collect();
    let indices = rt.upload_u32(shape(&[index_values.len()]), &index_values)?;
    let source = rt.upload_u32(shape(&[2, 3]), &[10, 20, 30, 40, 50, 60])?;
    let gathered = rt.gather_u32(&source, &indices, 1)?;
    assert_eq!(rt.read_u32(&gathered.invalid_count)?, [500_001]);
    let values = rt.read_u32(&gathered.values)?;
    for (i, &value) in values.iter().enumerate() {
        let expected = if (i % index_values.len()).is_multiple_of(2) {
            [20, 50][i / index_values.len()]
        } else {
            0
        };
        assert_eq!(value, expected);
    }
    assert!(matches!(
        other.gather_u32(&source, &indices, 1),
        Err(CudaError::ForeignRuntime)
    ));

    // A huge stride-zero view needs no allocation but must reject a count that
    // cannot be represented by the indexing contract's scalar u32 result.
    if usize::BITS > 32 {
        let scalar = rt.upload_u32(shape(&[]), &[1])?;
        let huge = rt.broadcast_u32(&scalar, shape(&[u32::MAX as usize + 1]))?;
        assert!(matches!(
            rt.compact_u32(&huge, &scalar),
            Err(CudaError::Contract(
                tensor_core::TensorError::IndexCountOverflow { .. }
            ))
        ));
        assert!(matches!(
            rt.gather_u32(&source, &huge, 1),
            Err(CudaError::Contract(
                tensor_core::TensorError::IndexCountOverflow { .. }
            ))
        ));
    }
    Ok(())
}
