//! Reusable index traversal and gather launches. These descriptors own only
//! checked host metadata; callers upload it once and supply device storage.
use crate::{
    CudaError, CudaRuntime,
    runtime::{layout_metadata, rank, validate_logical_size, validate_storage},
};
use gpu_compute::cuda::{CudaFunction, CudaSlice, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{Layout, Shape, gather_shape, validate_index_count};

#[derive(Clone, Debug)]
pub(crate) struct InvalidIndicesDispatch {
    pub metadata: Vec<u64>,
    pub count: u64,
    pub(super) rank: u32,
    pub(super) offset: u64,
    pub(super) length: u64,
    pub(super) required: usize,
}
impl InvalidIndicesDispatch {
    pub fn new(indices: &Layout, length: usize) -> Result<Self, CudaError> {
        validate_index_count(indices.shape())?;
        validate_logical_size::<u32>(indices.shape())?;
        Ok(Self {
            metadata: layout_metadata(indices),
            count: indices.shape().numel() as u64,
            rank: rank(indices.shape())?,
            offset: indices.offset() as u64,
            length: length as u64,
            required: indices.required_storage_len()?,
        })
    }

    /// Adds invalid logical indices to `invalid`. The caller MUST zero this
    /// scalar before every replay, including when `count == 0`. Counting is
    /// independent of whether gather/scatter has any output elements.
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        indices: &CudaSlice<u32>,
        invalid: &mut CudaSlice<u32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        validate_storage(&[
            (indices.len(), self.required),
            (invalid.len(), 1),
            (metadata.len(), self.metadata.len()),
        ])?;
        if self.count == 0 {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(&rt.indexing.invalid_indices)
                .arg(indices)
                .arg(invalid)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.rank)
                .arg(&self.offset)
                .arg(&self.length)
                .launch(rt.config(self.count as usize))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GatherDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    count: u64,
    index_count: u64,
    length: u64,
    inner: u64,
    input_rank: u32,
    index_rank: u32,
    input_offset: u64,
    index_offset: u64,
    required: [usize; 2],
}
impl GatherDispatch {
    pub fn new<T: DeviceRepr>(
        input: &Layout,
        indices: &Layout,
        axis: usize,
    ) -> Result<Self, CudaError> {
        let shape = gather_shape(input.shape(), indices.shape(), axis)?;
        validate_logical_size::<T>(input.shape())?;
        validate_logical_size::<T>(&shape)?;
        let index = InvalidIndicesDispatch::new(indices, input.shape().dims()[axis])?;
        let mut metadata = Vec::with_capacity(input.shape().rank() * 2 + index.metadata.len());
        metadata.extend(input.shape().dims().iter().map(|&v| v as u64));
        metadata.extend(input.strides().iter().map(|&v| v as u64));
        metadata.extend_from_slice(&index.metadata);
        // The suffix occurs in every nonempty output, so its product then fits
        // the validated output count. Empty outputs never divide by `inner`.
        let inner = if shape.is_empty() {
            0
        } else {
            input.shape().dims()[axis + 1..].iter().product::<usize>() as u64
        };
        Ok(Self {
            metadata,
            count: shape.numel() as u64,
            shape,
            index_count: index.count,
            length: index.length,
            inner,
            input_rank: rank(input.shape())?,
            index_rank: index.rank,
            input_offset: input.offset() as u64,
            index_offset: index.offset,
            required: [input.required_storage_len()?, index.required],
        })
    }

    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        inputs: (&CudaSlice<T>, &CudaSlice<u32>),
        output: &mut CudaSlice<T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        validate_storage(&[
            (inputs.0.len(), self.required[0]),
            (inputs.1.len(), self.required[1]),
            (output.len(), self.shape.numel()),
            (metadata.len(), self.metadata.len()),
        ])?;
        if self.count == 0 {
            return Ok(());
        }
        // Every index is checked against the axis length in the kernel. It
        // writes zero for invalid indices, also for a zero-length input axis.
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(inputs.0)
                .arg(inputs.1)
                .arg(output)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.index_count)
                .arg(&self.length)
                .arg(&self.inner)
                .arg(&self.input_rank)
                .arg(&self.index_rank)
                .arg(&self.input_offset)
                .arg(&self.index_offset)
                .launch(rt.config(self.shape.numel()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(dims: &[usize]) -> Layout {
        Layout::contiguous(Shape::new(dims.to_vec()).unwrap()).unwrap()
    }

    #[test]
    fn strided_gather_preserves_offsets_and_index_traversal() {
        let input = layout(&[4, 3, 5])
            .narrow(0, 1, 2)
            .unwrap()
            .permute(&[1, 0, 2])
            .unwrap();
        let indices = layout(&[3, 2])
            .narrow(0, 1, 2)
            .unwrap()
            .permute(&[1, 0])
            .unwrap();
        let pass = GatherDispatch::new::<f32>(&input, &indices, 1).unwrap();
        assert_eq!(pass.shape.dims(), &[3, 2, 2, 5]);
        assert_eq!(pass.metadata, [3, 2, 5, 5, 15, 1, 2, 2, 1, 2]);
        assert_eq!(
            (pass.input_offset, pass.index_offset, pass.inner),
            (15, 2, 5)
        );
        assert_eq!(pass.required, [45, 6]);
        assert_eq!((pass.index_count, pass.length), (4, 2));
    }

    #[test]
    fn scalar_and_empty_indices_keep_distinct_counts() {
        let scalar = layout(&[]);
        let index = InvalidIndicesDispatch::new(&scalar, 0).unwrap();
        assert_eq!(index.count, 1);
        assert!(index.metadata.is_empty());
        let pass = GatherDispatch::new::<u16>(&layout(&[2, 0, 3]), &scalar, 1).unwrap();
        assert_eq!(pass.shape.dims(), &[2, 3]);
        assert_eq!(
            (pass.count, pass.index_count, pass.length, pass.inner),
            (6, 1, 0, 3)
        );
        let empty = layout(&[0, 2]);
        assert_eq!(InvalidIndicesDispatch::new(&empty, 3).unwrap().count, 0);
        let pass = GatherDispatch::new::<u32>(&layout(&[2, 3]), &empty, 1).unwrap();
        assert_eq!(pass.shape.dims(), &[2, 0, 2]);
        assert_eq!((pass.count, pass.inner), (0, 0));
    }

    #[test]
    fn empty_outputs_do_not_suppress_original_index_count() {
        let indices = layout(&[7]);
        let input = layout(&[0, 4, 3]);
        let gather = GatherDispatch::new::<f32>(&input, &indices, 1).unwrap();
        let count = InvalidIndicesDispatch::new(&indices, input.shape().dims()[1]).unwrap();
        assert!(gather.shape.is_empty());
        assert_eq!((count.count, count.length), (7, 4));
        assert!(GatherDispatch::new::<f32>(&input, &indices, 3).is_err());
        let huge = layout(&[u32::MAX as usize + 1]);
        assert!(InvalidIndicesDispatch::new(&huge, 1).is_err());
    }

    #[test]
    fn every_buffer_extent_is_checked() {
        for bad in 0..4 {
            let mut lengths = [(9, 9), (7, 7), (4, 4), (10, 10)];
            lengths[bad].0 -= 1;
            assert!(validate_storage(&lengths).is_err());
        }
        assert!(validate_storage(&[(1, 0), (1, 1), (1, 0)]).is_ok());
    }
}
