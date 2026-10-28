//! Stable compaction uses normalized flags and their exclusive u32 prefix.
//! The caller zeros values and count before every execution, including empty
//! input. The write pass only touches selected slots and the final count.
use super::dispatch::CopyDispatch;
use crate::{
    CudaError, CudaRuntime,
    runtime::{layout_metadata, rank, validate_logical_size},
};
use gpu_compute::cuda::{CudaFunction, CudaSlice, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{Layout, Shape, compact_shape, validate_index_count};

#[derive(Clone, Debug)]
pub(crate) struct NormalizeMaskDispatch {
    pub geometry: CopyDispatch,
    pub flags_shape: Shape,
}
impl NormalizeMaskDispatch {
    pub fn new(input_shape: &Shape, mask: &Layout) -> Result<Self, CudaError> {
        let flags_shape = compact_shape(input_shape, mask.shape())?;
        let mask = mask.broadcast_to(input_shape.clone())?;
        Ok(Self {
            geometry: CopyDispatch::new::<u32>(&mask)?,
            flags_shape,
        })
    }
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        mask: &CudaSlice<u32>,
        flags: &mut CudaSlice<u32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        // Normalize has the exact copy ABI; only its kernel maps nonzero to one.
        self.geometry
            .launch(rt, &rt.indexing.normalize_mask, mask, flags, metadata)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CompactDispatch {
    pub metadata: Vec<u64>,
    pub values_shape: Shape,
    pub count_shape: Shape,
    count: u64,
    rank: u32,
    offset: u64,
    input_required: usize,
}
pub(crate) struct CompactInputs<'a, T> {
    pub values: &'a CudaSlice<T>,
    pub flags: &'a CudaSlice<u32>,
    pub prefix: &'a CudaSlice<u32>,
}
pub(crate) struct CompactOutputs<'a, T> {
    pub values: &'a mut CudaSlice<T>,
    pub count: &'a mut CudaSlice<u32>,
}
impl CompactDispatch {
    pub fn new<T: DeviceRepr>(input: &Layout) -> Result<Self, CudaError> {
        validate_index_count(input.shape())?;
        validate_logical_size::<T>(input.shape())?;
        Ok(Self {
            metadata: layout_metadata(input),
            values_shape: Shape::new(vec![input.shape().numel()])?,
            count_shape: Shape::new(vec![])?,
            count: input.shape().numel() as u64,
            rank: rank(input.shape())?,
            offset: input.offset() as u64,
            input_required: input.required_storage_len()?,
        })
    }
    fn validate_storage(
        &self,
        inputs: [usize; 3],
        outputs: [usize; 2],
        metadata: usize,
    ) -> Result<(), CudaError> {
        let count = self.count as usize;
        if inputs[0] < self.input_required
            || inputs[1] < count
            || inputs[2] < count
            || outputs[0] < count
            || outputs[1] < 1
            || metadata < self.metadata.len()
        {
            return Err(CudaError::InvalidInput(
                "compact dispatch storage is too small",
            ));
        }
        Ok(())
    }
    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        inputs: CompactInputs<'_, T>,
        output: CompactOutputs<'_, T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.validate_storage(
            [inputs.values.len(), inputs.flags.len(), inputs.prefix.len()],
            [output.values.len(), output.count.len()],
            metadata.len(),
        )?;
        if self.count == 0 {
            return Ok(());
        }
        // The normalized flags and their exclusive prefix make each selected
        // destination unique and bounded. Movement preserves the raw T bits.
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(inputs.values)
                .arg(inputs.flags)
                .arg(inputs.prefix)
                .arg(output.values)
                .arg(output.count)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.rank)
                .arg(&self.offset)
                .launch(rt.config(self.count as usize))?;
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
    #[test]
    fn broadcast_masks_and_strided_values_preserve_logical_order() {
        let input = Layout::new(shape(&[2, 3]), vec![1, 5], 7).unwrap();
        let mask = Layout::new(shape(&[3]), vec![2], 1).unwrap();
        let normalized = NormalizeMaskDispatch::new(input.shape(), &mask).unwrap();
        assert_eq!(normalized.flags_shape.dims(), [6]);
        assert_eq!(normalized.geometry.metadata, [2, 3, 0, 2]);
        assert_eq!(normalized.geometry.offset, 1);
        let compact = CompactDispatch::new::<u16>(&input).unwrap();
        assert_eq!(compact.metadata, [2, 3, 1, 5]);
        assert_eq!(compact.values_shape.dims(), [6]);
        assert_eq!(compact.count_shape.dims(), []);
        assert_eq!((compact.offset, compact.input_required), (7, 19));
        // Independent row-major fixture, including a non-binary true mask.
        let physical_mask = [0, 0, 0, u32::MAX, 0, 1];
        let expected_addresses = [12, 17, 13, 18];
        let mut addresses = Vec::new();
        for row in 0..2 {
            for column in 0..3 {
                if physical_mask[1 + column * 2] != 0 {
                    addresses.push(7 + row + column * 5);
                }
            }
        }
        assert_eq!(addresses, expected_addresses);
        assert!(compact.validate_storage([19, 6, 6], [6, 1], 4).is_ok());
        for (inputs, outputs, meta) in [
            ([18, 6, 6], [6, 1], 4),
            ([19, 5, 6], [6, 1], 4),
            ([19, 6, 5], [6, 1], 4),
            ([19, 6, 6], [5, 1], 4),
            ([19, 6, 6], [6, 0], 4),
            ([19, 6, 6], [6, 1], 3),
        ] {
            assert!(compact.validate_storage(inputs, outputs, meta).is_err());
        }
    }
    #[test]
    fn scalar_empty_and_u32_capacity_bound_have_checked_shapes() {
        let scalar = Layout::new(shape(&[]), vec![], 3).unwrap();
        let pass = CompactDispatch::new::<f32>(&scalar).unwrap();
        assert_eq!(pass.values_shape.dims(), [1]);
        assert!(pass.metadata.is_empty());
        assert_eq!((pass.count, pass.rank, pass.offset), (1, 0, 3));
        let empty = Layout::contiguous(shape(&[2, 0, 3])).unwrap();
        let pass = CompactDispatch::new::<u16>(&empty).unwrap();
        assert_eq!(pass.values_shape.dims(), [0]);
        assert_eq!(pass.count_shape.numel(), 1);
        assert!(pass.validate_storage([0, 0, 0], [0, 1], 6).is_ok());
        assert!(pass.validate_storage([0, 0, 0], [0, 0], 6).is_err());
        assert!(
            NormalizeMaskDispatch::new(empty.shape(), &Layout::contiguous(shape(&[4])).unwrap())
                .is_err()
        );
        let too_many = Layout::new(shape(&[u32::MAX as usize + 1]), vec![0], 0).unwrap();
        assert!(CompactDispatch::new::<u16>(&too_many).is_err());
        assert!(
            NormalizeMaskDispatch::new(too_many.shape(), &Layout::contiguous(shape(&[])).unwrap())
                .is_err()
        );
    }
}
