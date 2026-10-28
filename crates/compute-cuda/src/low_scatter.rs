use crate::{
    CudaError, CudaLowTensor, CudaRuntime, CudaTensor,
    indexing::{dispatch::CopyDispatch, output},
};
use tensor_core::{
    Layout, ScatterOp, Scattered, Shape, TensorError, TensorLowBackend, TensorLowScatterBackend,
    low_scatter_updates_shape,
};

pub(crate) fn padded_slots(count: usize) -> Result<usize, TensorError> {
    count
        .checked_add(count & 1)
        .map(|n| n.max(2))
        .ok_or(TensorError::ShapeOverflow)
}

impl CudaRuntime {
    fn low_scatter_updates(
        &self,
        input: &CudaLowTensor,
        indices: &CudaTensor<u32>,
        updates: &CudaLowTensor,
        axis: usize,
    ) -> Result<CudaTensor<u16>, CudaError> {
        self.check(&input.tensor)?;
        self.check(indices)?;
        self.check(&updates.tensor)?;
        let expected = low_scatter_updates_shape(input, indices.shape(), updates, axis)?;
        self.view(&updates.tensor, updates.layout().broadcast_to(expected)?)
    }

    // Raw scatter atomically updates aligned 32-bit words containing two u16
    // elements. Its independent output therefore pads the last physical word;
    // no CAS may reach past a two-byte odd tail allocation.
    fn copy_low_for_scatter(&self, input: &CudaLowTensor) -> Result<CudaLowTensor, CudaError> {
        let pass = CopyDispatch::new::<u16>(input.layout())?;
        let mut tensor =
            self.zeros_typed::<u16>(Shape::new(vec![padded_slots(input.shape().numel())?])?)?;
        tensor.layout = Layout::contiguous(input.shape().clone())?;
        if !pass.shape.is_empty() {
            let metadata = self.metadata(&pass.metadata)?;
            pass.launch(
                self,
                &self.low.copy,
                input.tensor.storage.as_ref(),
                output(&mut tensor),
                &metadata,
            )?;
        }
        Ok(CudaLowTensor {
            tensor,
            dtype: input.dtype,
        })
    }
}

impl TensorLowScatterBackend for CudaRuntime {
    fn scatter_low(
        &self,
        op: ScatterOp,
        input: &CudaLowTensor,
        indices: &CudaTensor<u32>,
        updates: &CudaLowTensor,
        axis: usize,
    ) -> Result<Scattered<CudaLowTensor, CudaTensor<u32>>, CudaError> {
        if matches!(op, ScatterOp::Add | ScatterOp::Multiply) {
            let result = self.scatter_low_f32(op, input, indices, updates, axis)?;
            return Ok(Scattered {
                values: self.cast_to_low(&result.values, input.dtype)?,
                invalid_count: result.invalid_count,
            });
        }
        let updates = self.low_scatter_updates(input, indices, updates, axis)?;
        let mut values = self.copy_low_for_scatter(input)?;
        let invalid_count = self.scatter_loaded(
            op,
            indices,
            &updates,
            &mut values.tensor,
            axis,
            &self.low.scatter_raw,
            None,
        )?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }

    fn scatter_low_f32(
        &self,
        op: ScatterOp,
        input: &CudaLowTensor,
        indices: &CudaTensor<u32>,
        updates: &CudaLowTensor,
        axis: usize,
    ) -> Result<Scattered<CudaTensor, CudaTensor<u32>>, CudaError> {
        let updates = self.low_scatter_updates(input, indices, updates, axis)?;
        // This is the requested f32 output/accumulator, not an expanded update
        // tensor. Every low update is decoded directly inside the scatter CAS.
        let mut values = self.cast_to_f32(input)?;
        let invalid_count = self.scatter_loaded(
            op,
            indices,
            &updates,
            &mut values,
            axis,
            &self.low.scatter_f32,
            Some(input.dtype as u32),
        )?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_scatter_cas_words_fit_padded_allocations() {
        for count in [0usize, 1, 2, 3, 255, 256, 257, 65535, usize::MAX / 2] {
            let slots = padded_slots(count).unwrap();
            assert_eq!(slots % 2, 0);
            assert!(slots >= count && slots >= 2);
            if count > 0 {
                let last_word_end = ((count - 1) / 2 + 1) * 2;
                assert!(last_word_end <= slots);
            }
        }
        assert_eq!(padded_slots(usize::MAX), Err(TensorError::ShapeOverflow));
    }
}
