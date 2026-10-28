#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use half::{bf16, f16};
use tensor_core::{
    CompareOp, HasShape, LowDtype, ScanOptions, Shape, TensorLowBackend, TensorLowIndexBackend,
};
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("MLX unavailable: {e}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
            None
        }
    }
}
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}

#[test]
fn common_low_index_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_low_index_backend(&b).unwrap();
}

#[test]
fn raw_native_gather_and_unique_compaction_keep_every_payload() {
    let Some(b) = backend() else { return };
    let all: Vec<u16> = (0..=u16::MAX).collect();
    let indices: Vec<u32> = (0..256).rev().collect();
    let indices = b.upload_u32(shape(&[256]), &indices).unwrap();
    let full = b.upload_u32(shape(&[]), &[u32::MAX]).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let input = b.upload_low(dtype, shape(&[256, 256]), &all).unwrap();
        let input = b.permute_low(&input, &[1, 0]).unwrap();
        let gathered = b.gather_low(&input, &indices, 0).unwrap();
        let expected: Vec<u16> = (0..256)
            .rev()
            .flat_map(|col| (0..256).map(move |row| (row * 256 + col) as u16))
            .collect();
        assert_eq!(b.read_low_bits(&gathered.values).unwrap(), expected);
        assert_eq!(b.read_u32(&gathered.invalid_count).unwrap(), [0]);
        let compact = b.compact_low(&input, &full).unwrap();
        let expected: Vec<u16> = (0..256)
            .flat_map(|col| (0..256).map(move |row| (row * 256 + col) as u16))
            .collect();
        drop(input);
        assert_eq!(b.read_low_bits(&compact.values).unwrap(), expected);
        assert_eq!(b.read_u32(&compact.count).unwrap(), [65536]);
    }
}

#[test]
fn chunk_scan_boundaries_broadcast_grid_stride_and_lifetimes() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for n in [1, 255, 256, 257, 511, 512, 513, 131077] {
            let values: Vec<f32> = (0..n)
                .map(|i| if i % 3 == 0 { 0.5 } else { -0.125 })
                .collect();
            let input = b
                .upload_low(
                    dtype,
                    shape(&[n]),
                    &values.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
                )
                .unwrap();
            for inclusive in [false, true] {
                for reverse in [false, true] {
                    let options = ScanOptions { inclusive, reverse };
                    let prefix = b.scan_low_f32(&input, 0, options).unwrap();
                    let low = b.scan_low(&input, 0, options).unwrap();
                    let mut expected = vec![0.; n];
                    let mut sum = 0.;
                    for p in 0..n {
                        let i = if reverse { n - 1 - p } else { p };
                        if inclusive {
                            sum += values[i];
                            expected[i] = sum;
                        } else {
                            expected[i] = sum;
                            sum += values[i];
                        }
                    }
                    assert_eq!(
                        b.read_f32(&prefix).unwrap(),
                        expected,
                        "{dtype:?} n={n} {options:?}"
                    );
                    assert_eq!(
                        b.read_low_bits(&low).unwrap(),
                        expected
                            .into_iter()
                            .map(|x| encode(dtype, x))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
        // Both scan dispatches use capped grids; this exercises workgroup reuse
        // on independent rows with shared memory rather than just long vectors.
        let scalar = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.5)])
            .unwrap();
        let view = b.broadcast_low(&scalar, shape(&[65537, 2])).unwrap();
        let prefix = b.scan_low_f32(&view, 1, ScanOptions::default()).unwrap();
        drop(view);
        drop(scalar);
        assert_eq!(b.read_f32(&prefix).unwrap(), [0.5, 1.].repeat(65537));
    }
}

#[test]
fn low_index_ownership_dtype_scalar_and_empty_rules() {
    let Some(b) = backend() else { return };
    let half = b.upload_low(LowDtype::F16, shape(&[]), &[0xfe01]).unwrap();
    let bf = b.upload_low(LowDtype::Bf16, shape(&[]), &[0x8001]).unwrap();
    let yes = b.upload_u32(shape(&[]), &[77]).unwrap();
    assert!(b.compare_low(CompareOp::Equal, &half, &bf).is_err());
    assert!(b.select_low(&yes, &half, &bf).is_err());
    assert!(b.scan_low_f32(&half, 0, ScanOptions::default()).is_err());
    let out = b.select_low(&yes, &half, &half).unwrap();
    assert_eq!(out.shape(), &shape(&[]));
    assert_eq!(b.read_low_bits(&out).unwrap(), [0xfe01]);
    let bad = b.upload_f32(shape(&[]), &[1.]).unwrap();
    assert!(matches!(
        b.select_low(&bad, &half, &half),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(b.gather_low(&half, &bad, 0), Err(MlxError::Dtype)));
    assert!(matches!(b.compact_low(&half, &bad), Err(MlxError::Dtype)));
    let foreign = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        foreign.compare_low(CompareOp::Equal, &half, &half),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        foreign.select_low(&yes, &half, &half),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        foreign.scan_low_f32(&half, 0, ScanOptions::default()),
        Err(MlxError::ForeignContext)
    ));
    let input = b.upload_low(LowDtype::Bf16, shape(&[0, 3]), &[]).unwrap();
    let indices = b.upload_u32(shape(&[3]), &[0, 5, u32::MAX]).unwrap();
    let gathered = b.gather_low(&input, &indices, 0).unwrap();
    assert_eq!(b.read_low_bits(&gathered.values).unwrap(), vec![0; 9]);
    assert_eq!(b.read_u32(&gathered.invalid_count).unwrap(), [3]);
    let compact = b.compact_low(&input, &yes).unwrap();
    assert!(b.read_low_bits(&compact.values).unwrap().is_empty());
    assert_eq!(b.read_u32(&compact.count).unwrap(), [0]);
    assert!(
        b.read_f32(&b.scan_low_f32(&input, 0, ScanOptions::default()).unwrap())
            .unwrap()
            .is_empty()
    );
    assert!(b.scan_low_f32(&input, 2, ScanOptions::default()).is_err());
}
