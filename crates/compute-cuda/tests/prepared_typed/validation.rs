use super::*;
use compute_cuda::CudaDtype;

pub fn mixed_bindings_and_recovery(rt: &CudaRuntime) -> Result {
    let input = rt.upload_f32(shape(&[2]), &[2., -3.])?;
    let uint = rt.upload_u32(shape(&[2]), &[0, 7])?;
    let low = rt.upload_low(LowDtype::F16, shape(&[2]), &[0x3c00, 0x8000])?;
    let bf16 = rt.upload_low(LowDtype::Bf16, shape(&[2]), &[0x3f80, 0x8000])?;
    let mut graph = rt.program();
    let f = graph.input(input.layout().clone())?;
    let u = graph.input_u32(uint.layout().clone())?;
    let l = graph.input_low(LowDtype::F16, low.layout().clone())?;
    let squared = graph.unary(f, UnaryOp::Square)?;
    let doubled = graph.binary_u32(u, u, BinaryOp::Add)?;
    let selected = graph.select_low(u, l, l)?;
    let mut program =
        graph.prepare(&[squared, doubled, selected], CudaPrepareOptions::default())?;
    assert_eq!(
        program.input_dtypes(),
        [CudaDtype::F32, CudaDtype::U32, CudaDtype::F16]
    );
    assert_eq!(
        program.output_dtypes(),
        [CudaDtype::F32, CudaDtype::U32, CudaDtype::F16]
    );
    let inputs = [(&input).into(), (&uint).into(), (&low).into()];
    let mut fout = rt.upload_f32(shape(&[2]), &[91.; 2])?;
    let mut uout = rt.upload_u32(shape(&[2]), &[92; 2])?;
    let mut lout = rt.upload_low(LowDtype::F16, shape(&[2]), &[93; 2])?;
    let mut wrong = rt.upload_low(LowDtype::Bf16, shape(&[2]), &[94; 2])?;
    assert!(matches!(program.run(&[&input]), Err(CudaError::Dtype)));
    assert!(matches!(
        program.run_typed(&[(&input).into(), (&uint).into(), (&bf16).into()]),
        Err(CudaError::Dtype)
    ));
    assert!(matches!(
        program.run_typed_into(
            &inputs,
            &mut [(&mut fout).into(), (&mut uout).into(), (&mut wrong).into()]
        ),
        Err(CudaError::Dtype)
    ));
    assert_eq!(rt.read_f32(&fout)?, [91.; 2]);
    assert_eq!(rt.read_u32(&uout)?, [92; 2]);
    assert_eq!(rt.read_low_bits(&wrong)?, [94; 2]);
    let alias = lout.clone();
    assert!(matches!(
        program.run_typed_into(
            &inputs,
            &mut [(&mut fout).into(), (&mut uout).into(), (&mut lout).into()]
        ),
        Err(CudaError::SharedOutput)
    ));
    assert_eq!(rt.read_f32(&fout)?, [91.; 2]);
    drop(alias);
    let other = CudaRuntime::new()?;
    let foreign = other.upload_low(LowDtype::F16, shape(&[2]), &[0; 2])?;
    assert!(matches!(
        program.run_typed(&[(&input).into(), (&uint).into(), (&foreign).into()]),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(!program.is_poisoned());
    program.run_typed_into(
        &inputs,
        &mut [(&mut fout).into(), (&mut uout).into(), (&mut lout).into()],
    )?;
    assert_eq!(rt.read_f32(&fout)?, [4., 9.]);
    assert_eq!(rt.read_u32(&uout)?, [0, 14]);
    assert_eq!(rt.read_low_bits(&lout)?, [0x3c00, 0x8000]);
    let owned = program.run_typed(&inputs)?;
    assert!(matches!(owned[0].as_u32(), Err(CudaError::Dtype)));
    assert!(matches!(owned[1].as_low(), Err(CudaError::Dtype)));
    // Internal low nodes with an exclusively f32 signature preserve legacy run.
    let mut graph = rt.program();
    let x = graph.input(input.layout().clone())?;
    let low = graph.cast_to_low(x, LowDtype::Bf16)?;
    let decoded = graph.cast_to_f32(low)?;
    let negative = graph.unary(decoded, UnaryOp::Negate)?;
    let mask = graph.compare(decoded, negative, CompareOp::Greater)?;
    let result = graph.select(mask, decoded, negative)?;
    let mut legacy = graph.prepare(&[result], CudaPrepareOptions::default())?;
    assert_eq!(rt.read_f32(&legacy.run(&[&input])?[0])?, [2., 3.]);
    // All four empty dtypes reserve a valid sentinel and submit no copy work.
    let f = rt.upload_f32(shape(&[0]), &[])?;
    let u = rt.upload_u32(shape(&[0]), &[])?;
    let h = rt.upload_low(LowDtype::F16, shape(&[0]), &[])?;
    let b = rt.upload_low(LowDtype::Bf16, shape(&[0]), &[])?;
    let mut graph = rt.program();
    let fi = graph.input(f.layout().clone())?;
    let ui = graph.input_u32(u.layout().clone())?;
    let hi = graph.input_low(LowDtype::F16, h.layout().clone())?;
    let bi = graph.input_low(LowDtype::Bf16, b.layout().clone())?;
    let mut empty = graph.prepare(&[fi, ui, hi, bi], CudaPrepareOptions::default())?;
    let results = empty.run_typed(&[(&f).into(), (&u).into(), (&h).into(), (&b).into()])?;
    assert_eq!(empty.stats().kernel_launches, 0);
    assert!(rt.read_f32(results[0].as_f32()?)?.is_empty());
    assert!(rt.read_u32(results[1].as_u32()?)?.is_empty());
    assert!(rt.read_low_bits(results[2].as_low()?)?.is_empty());
    assert!(rt.read_low_bits(results[3].as_low()?)?.is_empty());
    Ok(())
}
