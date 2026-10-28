#![cfg(not(target_arch = "wasm32"))]
use compute_core::{ComputeError, ComputeRuntime, TensorComputeError, gpu_compute::GpuContext};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tensor_core::{
    CompareOp, ScanOptions, Shape, TensorBackend, TensorEvalBackend, TensorIndexBackend, UnaryOp,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn evaluate_fences_producers_and_validates_every_owner() {
    let Some(context) = GpuContext::new() else {
        assert!(
            std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
            "GPU required"
        );
        eprintln!("SKIP evaluation: no GPU adapter");
        return;
    };
    let backend = ComputeRuntime::new(&context).unwrap();
    let input = backend
        .upload_f32(shape(&[2, 3]), &[1., 2., 3., 4., 5., 6.])
        .unwrap();
    let square = backend.unary(UnaryOp::Square, &input).unwrap();
    let threshold = backend.upload_f32(shape(&[]), &[9.]).unwrap();
    let mask = backend
        .compare(CompareOp::Greater, &square, &threshold)
        .unwrap();
    let prefix = backend.scan_u32(&mask, 1, ScanOptions::default()).unwrap();
    let floats = backend.permute(&square, &[1, 0]).unwrap();
    let integers = backend.permute_u32(&prefix, &[1, 0]).unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let observed = completed.clone();
    backend
        .queue()
        .on_submitted_work_done(move || observed.store(true, Ordering::SeqCst));
    backend.evaluate(&[&floats, &floats], &[&integers]).unwrap();
    assert!(
        completed.load(Ordering::SeqCst),
        "evaluate returned before submitted producer completed"
    );
    drop((input, square, threshold, mask, prefix));
    assert_eq!(
        backend.read_f32(&floats).unwrap(),
        [1., 16., 4., 25., 9., 36.]
    );
    assert_eq!(backend.read_u32(&integers).unwrap(), [0, 1, 0, 2, 0, 3]);

    let empty = backend.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let empty_u32 = backend.upload_u32(shape(&[0, 3]), &[]).unwrap();
    let foreign_backend = ComputeRuntime::new(&context).unwrap();
    let foreign = foreign_backend.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let foreign_u32 = foreign_backend.upload_u32(shape(&[0, 3]), &[]).unwrap();
    for result in [
        backend.evaluate(&[&floats, &foreign], &[&integers]),
        backend.evaluate(&[&floats], &[&integers, &foreign_u32]),
    ] {
        assert!(matches!(
            result,
            Err(TensorComputeError::Compute(ComputeError::ForeignArray))
        ));
    }
    backend.evaluate(&[&empty], &[&empty_u32]).unwrap();
    backend.evaluate(&[], &[]).unwrap();
    assert_eq!(
        backend.read_f32(&floats).unwrap(),
        [1., 16., 4., 25., 9., 36.]
    );
}
