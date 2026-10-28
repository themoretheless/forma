use super::plan::{CudaDtype, TensorSpec};
use crate::CudaError;
use tensor_core::Layout;

pub(super) struct Binding<'a> {
    pub layout: &'a Layout,
    pub dtype: CudaDtype,
    pub storage_len: usize,
    pub allocation: usize,
    pub unique: bool,
}
// Runtime ownership is checked by CudaRuntime::check before these layout and
// alias checks. This pure boundary also supports real pre-enqueue unit tests.
pub(super) fn inputs(expected: &[TensorSpec], actual: &[Binding<'_>]) -> Result<(), CudaError> {
    if actual.len() != expected.len() {
        return Err(CudaError::InvalidInput(
            "prepared CUDA input count mismatch",
        ));
    }
    for (expected, actual) in expected.iter().zip(actual) {
        if actual.dtype != expected.dtype {
            return Err(CudaError::Dtype);
        }
        actual.layout.validate_storage_len(actual.storage_len)?;
        if actual.layout != &expected.layout {
            return Err(CudaError::InvalidInput(
                "prepared CUDA input layout mismatch",
            ));
        }
    }
    Ok(())
}
pub(super) fn outputs(
    expected: &[TensorSpec],
    inputs: &[Binding<'_>],
    actual: &[Binding<'_>],
) -> Result<(), CudaError> {
    if actual.len() != expected.len() {
        return Err(CudaError::InvalidInput(
            "prepared CUDA output count mismatch",
        ));
    }
    for (i, (expected, binding)) in expected.iter().zip(actual).enumerate() {
        if binding.dtype != expected.dtype {
            return Err(CudaError::Dtype);
        }
        binding.layout.validate_storage_len(binding.storage_len)?;
        if binding.layout.shape() != expected.shape() {
            return Err(CudaError::InvalidInput(
                "prepared CUDA output shape mismatch",
            ));
        }
        if !binding.unique || !binding.layout.is_contiguous() || binding.layout.offset() != 0 {
            return Err(CudaError::SharedOutput);
        }
        if inputs
            .iter()
            .any(|input| input.allocation == binding.allocation)
            || actual[..i]
                .iter()
                .any(|previous| previous.allocation == binding.allocation)
        {
            return Err(CudaError::InvalidInput(
                "prepared CUDA outputs alias input or output storage",
            ));
        }
    }
    Ok(())
}

pub(super) fn precisions<T: Copy>(
    requests: &[T],
    mut validate: impl FnMut(T) -> Result<(), CudaError>,
) -> Result<(), CudaError> {
    for &precision in requests {
        validate(precision)?;
    }
    Ok(())
}

pub(super) fn legacy_f32(inputs: &[TensorSpec], outputs: &[TensorSpec]) -> Result<(), CudaError> {
    if inputs
        .iter()
        .chain(outputs)
        .any(|s| s.dtype != CudaDtype::F32)
    {
        Err(CudaError::Dtype)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::Shape;
    fn shape(dims: &[usize]) -> Shape {
        Shape::new(dims.to_vec()).unwrap()
    }
    fn dense(dims: &[usize]) -> Layout {
        Layout::contiguous(shape(dims)).unwrap()
    }
    fn spec(layout: &Layout, dtype: CudaDtype) -> TensorSpec {
        TensorSpec {
            layout: layout.clone(),
            dtype,
        }
    }
    fn binding(layout: &Layout, dtype: CudaDtype, allocation: usize) -> Binding<'_> {
        Binding {
            layout,
            dtype,
            storage_len: layout.required_storage_len().unwrap().max(1),
            allocation,
            unique: true,
        }
    }
    #[test]
    fn fixed_inputs_accept_new_storage_and_readonly_aliases_but_reject_changed_views() {
        let layout = Layout::new(shape(&[2, 3]), vec![1, 2], 7).unwrap();
        let expected = spec(&layout, CudaDtype::U32);
        assert!(
            inputs(
                &[expected.clone(), expected.clone()],
                &[
                    binding(&layout, CudaDtype::U32, 1),
                    binding(&layout, CudaDtype::U32, 1)
                ]
            )
            .is_ok()
        );
        assert!(
            inputs(
                std::slice::from_ref(&expected),
                &[binding(&layout, CudaDtype::U32, 99)]
            )
            .is_ok()
        );
        assert!(inputs(std::slice::from_ref(&expected), &[]).is_err());
        assert!(
            inputs(
                std::slice::from_ref(&expected),
                &[binding(&dense(&[2, 3]), CudaDtype::U32, 1)]
            )
            .is_err()
        );
        let mut short = binding(&layout, CudaDtype::U32, 1);
        short.storage_len = 12;
        assert!(inputs(&[expected], &[short]).is_err());
    }
    #[test]
    fn all_outputs_are_validated_before_late_alias_or_layout_failure() {
        let layout = dense(&[2, 3]);
        let expected = vec![spec(&layout, CudaDtype::F16); 2];
        let input = [binding(&layout, CudaDtype::F16, 1)];
        assert!(
            outputs(
                &expected,
                &input,
                &[
                    binding(&layout, CudaDtype::F16, 2),
                    binding(&layout, CudaDtype::F16, 3)
                ]
            )
            .is_ok()
        );
        for identity in [1, 2] {
            assert!(
                outputs(
                    &expected,
                    &input,
                    &[
                        binding(&layout, CudaDtype::F16, 2),
                        binding(&layout, CudaDtype::F16, identity)
                    ]
                )
                .is_err()
            );
        }
        for invalid in [
            Layout::new(shape(&[2, 3]), vec![3, 1], 1).unwrap(),
            Layout::new(shape(&[2, 3]), vec![1, 2], 0).unwrap(),
        ] {
            assert!(
                outputs(
                    &expected,
                    &input,
                    &[
                        binding(&layout, CudaDtype::F16, 2),
                        binding(&invalid, CudaDtype::F16, 3)
                    ]
                )
                .is_err()
            );
        }
        let mut shared = binding(&layout, CudaDtype::F16, 3);
        shared.unique = false;
        assert!(matches!(
            outputs(
                &expected,
                &input,
                &[binding(&layout, CudaDtype::F16, 2), shared]
            ),
            Err(CudaError::SharedOutput)
        ));
    }
    #[test]
    fn scalar_and_empty_dense_outputs_still_need_unique_storage_and_matching_shape() {
        for dims in [vec![], vec![2, 0, 3]] {
            let layout = dense(&dims);
            let expected = spec(&layout, CudaDtype::Bf16);
            assert!(
                outputs(
                    std::slice::from_ref(&expected),
                    &[],
                    &[binding(&layout, CudaDtype::Bf16, 1)]
                )
                .is_ok()
            );
            let mut shared = binding(&layout, CudaDtype::Bf16, 1);
            shared.unique = false;
            assert!(outputs(&[expected], &[], &[shared]).is_err());
        }
        assert!(
            outputs(
                &[spec(&dense(&[]), CudaDtype::F32)],
                &[],
                &[binding(&dense(&[1]), CudaDtype::F32, 1)]
            )
            .is_err()
        );
    }
    #[test]
    fn every_dtype_mismatch_is_rejected_before_empty_layout_shortcuts() {
        for dims in [vec![], vec![0]] {
            let layout = dense(&dims);
            for expected in [
                CudaDtype::F32,
                CudaDtype::U32,
                CudaDtype::F16,
                CudaDtype::Bf16,
            ] {
                for actual in [
                    CudaDtype::F32,
                    CudaDtype::U32,
                    CudaDtype::F16,
                    CudaDtype::Bf16,
                ] {
                    let wanted = [spec(&layout, expected)];
                    let value = [binding(&layout, actual, 1)];
                    assert_eq!(inputs(&wanted, &value).is_ok(), expected == actual);
                    assert_eq!(outputs(&wanted, &[], &value).is_ok(), expected == actual);
                }
            }
        }
    }
}
