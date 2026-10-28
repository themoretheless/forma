use super::{
    CudaScalar,
    gather_dispatch::{GatherDispatch, InvalidIndicesDispatch},
    output,
};
use crate::{CudaError, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{
    CudaFunction,
    cudarc::driver::{DeviceRepr, ValidAsZeroBits},
};
use tensor_core::{Gathered, Shape};

impl CudaRuntime {
    pub(super) fn invalid_index_count(
        &self,
        indices: &CudaTensor<u32>,
        length: usize,
    ) -> Result<CudaTensor<u32>, CudaError> {
        self.check(indices)?;
        let pass = InvalidIndicesDispatch::new(&indices.layout, length)?;
        // Fresh eager storage supplies the mandatory zero before the atomic
        // count. Prepared replay explicitly clears its reused scalar instead.
        let mut result = self.zeros_typed(Shape::new(vec![])?)?;
        if pass.count != 0 {
            let metadata = self.metadata(&pass.metadata)?;
            pass.launch(
                self,
                indices.storage.as_ref(),
                output(&mut result),
                &metadata,
            )?;
        }
        Ok(result)
    }

    pub(crate) fn gather_typed<T: CudaScalar>(
        &self,
        input: &CudaTensor<T>,
        indices: &CudaTensor<u32>,
        axis: usize,
    ) -> Result<Gathered<CudaTensor<T>, CudaTensor<u32>>, CudaError> {
        self.gather_loaded(input, indices, axis, &self.indexing.gather[T::KIND])
    }

    pub(crate) fn gather_loaded<T: DeviceRepr + ValidAsZeroBits>(
        &self,
        input: &CudaTensor<T>,
        indices: &CudaTensor<u32>,
        axis: usize,
        kernel: &CudaFunction,
    ) -> Result<Gathered<CudaTensor<T>, CudaTensor<u32>>, CudaError> {
        self.check(input)?;
        self.check(indices)?;
        let pass = GatherDispatch::new::<T>(&input.layout, &indices.layout, axis)?;
        let mut values = self.zeros_typed(pass.shape.clone())?;
        // This counts original indices, including for an empty output shape.
        let invalid_count = self.invalid_index_count(indices, input.shape().dims()[axis])?;
        if !pass.shape.is_empty() {
            let metadata = self.metadata(&pass.metadata)?;
            pass.launch(
                self,
                kernel,
                (input.storage.as_ref(), indices.storage.as_ref()),
                output(&mut values),
                &metadata,
            )?;
        }
        Ok(Gathered {
            values,
            invalid_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::{Layout, gather_shape};

    #[test]
    fn gather_flat_addressing_preserves_prefix_indices_and_suffix_axes() {
        let input = Layout::contiguous(Shape::new(vec![2, 3, 5]).unwrap())
            .unwrap()
            .permute(&[1, 0, 2])
            .unwrap();
        let indices = [1usize, 0, 1, 1];
        let result_shape =
            gather_shape(input.shape(), &Shape::new(vec![2, 2]).unwrap(), 1).unwrap();
        assert_eq!(result_shape.dims(), &[3, 2, 2, 5]);
        for i in 0..result_shape.numel() {
            let prefix = i / (indices.len() * 5);
            let chosen = indices[(i / 5) % indices.len()];
            let source_linear = (prefix * 2 + chosen) * 5 + i % 5;
            let expected_address = chosen * 15 + prefix * 5 + i % 5;
            assert_eq!(
                input.element_offset(source_linear).unwrap(),
                expected_address
            );
        }
    }
}
