#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{CompareOp, ScanOptions, Shape, TensorEvalBackend, TensorIndexBackend, UnaryOp};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu() {
        Ok(backend) => Some(backend),
        Err(error @ MlxError::Unavailable(_)) => {
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
            eprintln!("SKIP native MLX evaluation: {error}");
            None
        }
        Err(error) => panic!("MLX initialization failed: {error}"),
    }
}

#[test]
fn evaluate_lazy_producers_keeps_resident_results_and_views() {
    let Some(backend) = backend() else { return };
    let (floats, integers) = {
        let input = backend
            .upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])
            .unwrap();
        let square = backend.unary(UnaryOp::Square, &input).unwrap();
        let threshold = backend.upload_f32(shape(&[]), &[9.]).unwrap();
        let mask = backend
            .compare(CompareOp::Greater, &square, &threshold)
            .unwrap();
        let prefix = backend.scan_u32(&mask, 1, ScanOptions::default()).unwrap();
        (
            backend.permute(&square, &[1, 0]).unwrap(),
            backend.permute_u32(&prefix, &[1, 0]).unwrap(),
        )
    };
    // The original producer handles have already gone away. Evaluation must
    // follow the retained lazy graph, including its integer-output dependency.
    backend.evaluate(&[&floats, &floats], &[&integers]).unwrap();
    assert_eq!(
        backend.read_f32(&floats).unwrap(),
        [1., 16., 4., 25., 9., 36.]
    );
    assert_eq!(backend.read_u32(&integers).unwrap(), [0, 1, 0, 2, 0, 3]);
    backend.evaluate(&[], &[]).unwrap();
    backend.evaluate(&[], &[&integers]).unwrap();
}

#[test]
fn evaluate_rejects_late_foreign_or_wrong_dtype_even_when_empty() {
    let Some(backend) = backend() else { return };
    let empty = backend.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let empty_u32 = backend.upload_u32(shape(&[0, 3]), &[]).unwrap();
    let local = backend.upload_f32(shape(&[]), &[3.]).unwrap();
    let local = backend.unary(UnaryOp::Square, &local).unwrap();
    let foreign_backend = MlxBackend::new_gpu().unwrap();
    let foreign = foreign_backend.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let foreign_u32 = foreign_backend.upload_u32(shape(&[0, 3]), &[]).unwrap();
    assert!(matches!(
        backend.evaluate(&[&local, &foreign], &[]),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        backend.evaluate(&[&local], &[&empty_u32, &foreign_u32]),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        backend.evaluate(&[&local, &empty_u32], &[]),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        backend.evaluate(&[&local], &[&empty_u32, &empty]),
        Err(MlxError::Dtype)
    ));
    backend.evaluate(&[&empty, &local], &[&empty_u32]).unwrap();
    assert_eq!(backend.read_f32(&local).unwrap(), [9.]);
}
