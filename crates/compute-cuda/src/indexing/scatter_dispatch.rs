//! Reusable scatter launches. Base copies, count resets and owner resets are
//! explicit caller work, so replay never relies on prior allocation contents.
use super::gather_dispatch::InvalidIndicesDispatch;
use crate::runtime::validate_storage;
use crate::{
    CudaError, CudaRuntime,
    low_scatter::padded_slots,
    runtime::{rank, validate_logical_size},
};
use gpu_compute::cuda::{CudaFunction, CudaSlice, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{Layout, ScatterOp, Shape, TensorError, scatter_updates_shape};

#[derive(Clone, Debug)]
pub(crate) struct ScatterOwnersDispatch {
    pub geometry: InvalidIndicesDispatch,
    pub shape: Shape,
}
impl ScatterOwnersDispatch {
    pub fn new(indices: &Layout, length: usize) -> Result<Self, CudaError> {
        let geometry = InvalidIndicesDispatch::new(indices, length)?;
        let shape = Shape::new(vec![length])?;
        validate_logical_size::<u32>(&shape)?;
        Ok(Self { geometry, shape })
    }

    /// Elects the largest logical index position plus one. `owners` MUST be
    /// zeroed before each replay. Position zero means no selected update.
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        indices: &CudaSlice<u32>,
        owners: &mut CudaSlice<u32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let g = &self.geometry;
        validate_storage(&[
            (indices.len(), g.required),
            (owners.len(), self.shape.numel()),
            (metadata.len(), g.metadata.len()),
        ])?;
        if g.count == 0 || self.shape.is_empty() {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(&rt.indexing.scatter_owners)
                .arg(indices)
                .arg(owners)
                .arg(metadata)
                .arg(&g.count)
                .arg(&g.rank)
                .arg(&g.offset)
                .arg(&g.length)
                .launch(rt.config(g.count as usize))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScatterDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    pub updates_shape: Shape,
    count: u64,
    index: InvalidIndicesDispatch,
    inner: u64,
    update_rank: u32,
    update_offset: u64,
    update_required: usize,
    output_required: usize,
    operation: ScatterOp,
}
pub(crate) struct LoadedScatter<'a> {
    pub function: &'a CudaFunction,
    pub dtype: Option<u32>,
}
pub(crate) struct ScatterInputs<'a, T> {
    pub indices: &'a CudaSlice<u32>,
    pub updates: &'a CudaSlice<T>,
    pub owners: &'a CudaSlice<u32>,
}
impl ScatterDispatch {
    pub fn new<I: DeviceRepr, O: DeviceRepr>(
        op: ScatterOp,
        input: &Shape,
        indices: &Layout,
        updates: &Layout,
        axis: usize,
    ) -> Result<Self, CudaError> {
        let expected = scatter_updates_shape(input, indices.shape(), updates.shape(), axis)?;
        validate_logical_size::<O>(input)?;
        validate_logical_size::<I>(&expected)?;
        let index = InvalidIndicesDispatch::new(indices, input.dims()[axis])?;
        let updates = updates.broadcast_to(expected.clone())?;
        let mut metadata = Vec::with_capacity(updates.shape().rank() * 2 + index.metadata.len());
        metadata.extend(updates.shape().dims().iter().map(|&v| v as u64));
        metadata.extend(updates.strides().iter().map(|&v| v as u64));
        metadata.extend_from_slice(&index.metadata);
        // Raw low scatter writes aligned u32 CAS words. Even an odd logical
        // result must own the adjacent halfword; terminal copies may be dense.
        let output_required = if size_of::<O>() == size_of::<u16>() {
            padded_slots(input.numel())?
        } else {
            input.numel()
        };
        output_required
            .checked_mul(size_of::<O>())
            .ok_or(TensorError::ShapeOverflow)?;
        let inner = if input.is_empty() {
            0
        } else {
            input.dims()[axis + 1..].iter().product::<usize>() as u64
        };
        Ok(Self {
            metadata,
            shape: input.clone(),
            updates_shape: expected,
            count: updates.shape().numel() as u64,
            index,
            inner,
            update_rank: rank(updates.shape())?,
            update_offset: updates.offset() as u64,
            update_required: updates.required_storage_len()?,
            output_required,
            operation: op,
        })
    }

    pub fn has_work(&self) -> bool {
        !self.shape.is_empty() && self.index.count != 0
    }
    pub fn needs_owners(&self) -> bool {
        self.has_work() && self.operation == ScatterOp::Replace
    }

    /// Updates an independent contiguous base copy in place. All metadata and
    /// source buffers remain resident; this method does not allocate or reset.
    pub fn launch<I: DeviceRepr, O: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: LoadedScatter<'_>,
        inputs: ScatterInputs<'_, I>,
        output: &mut CudaSlice<O>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let owners_required = if self.needs_owners() {
            self.index.length as usize
        } else {
            0
        };
        validate_storage(&[
            (inputs.indices.len(), self.index.required),
            (inputs.updates.len(), self.update_required),
            (inputs.owners.len(), owners_required),
            (output.len(), self.output_required),
            (metadata.len(), self.metadata.len()),
        ])?;
        if !self.has_work() {
            return Ok(());
        }
        let operation = self.operation as u32;
        unsafe {
            let mut launch = rt.device.stream.launch_builder(kernel.function);
            launch
                .arg(inputs.indices)
                .arg(inputs.updates)
                .arg(output)
                .arg(inputs.owners)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.index.count)
                .arg(&self.index.length)
                .arg(&self.inner)
                .arg(&self.update_rank)
                .arg(&self.index.rank)
                .arg(&self.update_offset)
                .arg(&self.index.offset)
                .arg(&operation);
            if let Some(dtype) = kernel.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(rt.config(self.updates_shape.numel()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shape(dims: &[usize]) -> Shape {
        Shape::new(dims.to_vec()).unwrap()
    }
    fn layout(dims: &[usize]) -> Layout {
        Layout::contiguous(shape(dims)).unwrap()
    }

    #[test]
    fn strided_broadcast_updates_and_indices_share_logical_addressing() {
        let indices = layout(&[3, 2])
            .narrow(0, 1, 2)
            .unwrap()
            .permute(&[1, 0])
            .unwrap();
        let updates = layout(&[4, 2])
            .narrow(0, 1, 3)
            .unwrap()
            .permute(&[1, 0])
            .unwrap();
        let pass = ScatterDispatch::new::<f32, f32>(
            ScatterOp::Replace,
            &shape(&[5, 3]),
            &indices,
            &updates,
            0,
        )
        .unwrap();
        assert_eq!(pass.updates_shape.dims(), &[2, 2, 3]);
        assert_eq!(pass.metadata, [2, 2, 3, 0, 1, 2, 2, 2, 1, 2]);
        assert_eq!(
            (pass.update_offset, pass.index.offset, pass.inner),
            (2, 2, 3)
        );
        assert_eq!(
            (pass.count, pass.update_required, pass.output_required),
            (12, 8, 15)
        );
        assert!(pass.needs_owners());
        let owner = ScatterOwnersDispatch::new(&indices, 5).unwrap();
        assert_eq!(owner.geometry.metadata, [2, 2, 1, 2]);
        assert_eq!(owner.shape.dims(), &[5]);
    }

    #[test]
    fn empty_base_and_empty_indices_still_have_independent_count_geometry() {
        for base in [shape(&[0, 3]), shape(&[2, 0])] {
            let pass = ScatterDispatch::new::<u32, u32>(
                ScatterOp::Replace,
                &base,
                &layout(&[7]),
                &layout(&[]),
                1,
            )
            .unwrap();
            assert!(!pass.has_work());
            assert!(!pass.needs_owners());
            assert_eq!(pass.index.count, 7);
            assert_eq!(pass.index.length, base.dims()[1] as u64);
        }
        let pass = ScatterDispatch::new::<u32, u32>(
            ScatterOp::Add,
            &shape(&[5]),
            &layout(&[0]),
            &layout(&[]),
            0,
        )
        .unwrap();
        assert!(!pass.has_work());
        assert_eq!(pass.index.count, 0);
        let scalar = ScatterDispatch::new::<f32, f32>(
            ScatterOp::Add,
            &shape(&[5]),
            &layout(&[]),
            &layout(&[]),
            0,
        )
        .unwrap();
        assert!(scalar.has_work());
        assert!(!scalar.needs_owners());
        assert_eq!((scalar.count, scalar.index.count, scalar.inner), (1, 1, 1));
    }

    #[test]
    fn raw_low_outputs_require_complete_cas_words() {
        for count in [0, 1, 2, 3, 257] {
            let low = ScatterDispatch::new::<u16, u16>(
                ScatterOp::Min,
                &shape(&[count]),
                &layout(&[1]),
                &layout(&[]),
                0,
            )
            .unwrap();
            let float = ScatterDispatch::new::<u16, f32>(
                ScatterOp::Min,
                &shape(&[count]),
                &layout(&[1]),
                &layout(&[]),
                0,
            )
            .unwrap();
            assert_eq!(low.output_required, padded_slots(count).unwrap());
            assert_eq!(float.output_required, count);
            if count % 2 != 0 {
                assert!(validate_storage(&[(count, low.output_required)]).is_err());
            }
        }
    }

    #[test]
    fn invalid_axis_broadcast_and_position_token_overflow_are_rejected() {
        assert!(
            ScatterDispatch::new::<f32, f32>(
                ScatterOp::Add,
                &shape(&[5]),
                &layout(&[2]),
                &layout(&[3]),
                0
            )
            .is_err()
        );
        assert!(
            ScatterDispatch::new::<f32, f32>(
                ScatterOp::Add,
                &shape(&[5]),
                &layout(&[2]),
                &layout(&[]),
                1
            )
            .is_err()
        );
        assert!(ScatterOwnersDispatch::new(&layout(&[u32::MAX as usize + 1]), 3).is_err());
    }
}
