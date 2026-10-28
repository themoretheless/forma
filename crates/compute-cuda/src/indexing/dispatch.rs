//! Allocation-free typed copy, comparison and selection launch descriptors.
use crate::{
    CudaError, CudaRuntime,
    dispatch::binary_metadata,
    runtime::{layout_metadata, rank, validate_logical_size},
};
use gpu_compute::cuda::{CudaFunction, CudaSlice, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{CompareOp, Layout, Shape, select_shape};

#[derive(Clone, Debug)]
pub(crate) struct CopyDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    pub count: u64,
    pub rank: u32,
    pub offset: u64,
    input_required: usize,
}
impl CopyDispatch {
    pub fn new<T: DeviceRepr>(input: &Layout) -> Result<Self, CudaError> {
        validate_logical_size::<T>(input.shape())?;
        Ok(Self {
            metadata: layout_metadata(input),
            shape: input.shape().clone(),
            count: input.shape().numel() as u64,
            rank: rank(input.shape())?,
            offset: input.offset() as u64,
            input_required: input.required_storage_len()?,
        })
    }
    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        input: &CudaSlice<T>,
        output: &mut CudaSlice<T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        validate_buffers(
            &[(input.len(), self.input_required)],
            output.len(),
            self.shape.numel(),
            metadata.len(),
            self.metadata.len(),
        )?;
        if self.count == 0 {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(input)
                .arg(output)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.rank)
                .arg(&self.offset)
                .launch(rt.config(self.shape.numel()))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CompareDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    count: u64,
    rank: u32,
    left_offset: u64,
    right_offset: u64,
    required: [usize; 2],
    op: u32,
}
pub(crate) struct LoadedCompare<'a> {
    pub function: &'a CudaFunction,
    pub dtype: Option<u32>,
}
impl CompareDispatch {
    pub fn new(op: CompareOp, left: &Layout, right: &Layout) -> Result<Self, CudaError> {
        let (shape, metadata) = binary_metadata::<u32>(left, right)?;
        Ok(Self {
            metadata,
            count: shape.numel() as u64,
            rank: rank(&shape)?,
            left_offset: left.offset() as u64,
            right_offset: right.offset() as u64,
            required: [left.required_storage_len()?, right.required_storage_len()?],
            shape,
            op: op as u32,
        })
    }
    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: LoadedCompare<'_>,
        inputs: (&CudaSlice<T>, &CudaSlice<T>),
        output: &mut CudaSlice<u32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        validate_buffers(
            &[
                (inputs.0.len(), self.required[0]),
                (inputs.1.len(), self.required[1]),
            ],
            output.len(),
            self.shape.numel(),
            metadata.len(),
            self.metadata.len(),
        )?;
        if self.count == 0 {
            return Ok(());
        }
        unsafe {
            let mut launch = rt.device.stream.launch_builder(kernel.function);
            launch
                .arg(inputs.0)
                .arg(inputs.1)
                .arg(output)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.rank)
                .arg(&self.left_offset)
                .arg(&self.right_offset)
                .arg(&self.op);
            if let Some(dtype) = kernel.dtype.as_ref() {
                launch.arg(dtype);
            }
            launch.launch(rt.config(self.shape.numel()))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SelectDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    count: u64,
    rank: u32,
    offsets: [u64; 3],
    required: [usize; 3],
}
pub(crate) struct SelectInputs<'a, T> {
    pub mask: &'a CudaSlice<u32>,
    pub yes: &'a CudaSlice<T>,
    pub no: &'a CudaSlice<T>,
}
impl SelectDispatch {
    pub fn new<T: DeviceRepr>(mask: &Layout, yes: &Layout, no: &Layout) -> Result<Self, CudaError> {
        let shape = select_shape(mask.shape(), yes.shape(), no.shape())?;
        validate_logical_size::<T>(&shape)?;
        let mask = mask.broadcast_to(shape.clone())?;
        let yes = yes.broadcast_to(shape.clone())?;
        let no = no.broadcast_to(shape.clone())?;
        let mut metadata = Vec::with_capacity(shape.rank() * 4);
        metadata.extend(mask.shape().dims().iter().map(|&v| v as u64));
        metadata.extend(mask.strides().iter().map(|&v| v as u64));
        metadata.extend(yes.strides().iter().chain(no.strides()).map(|&v| v as u64));
        Ok(Self {
            metadata,
            count: shape.numel() as u64,
            rank: rank(&shape)?,
            offsets: [
                mask.offset() as u64,
                yes.offset() as u64,
                no.offset() as u64,
            ],
            required: [
                mask.required_storage_len()?,
                yes.required_storage_len()?,
                no.required_storage_len()?,
            ],
            shape,
        })
    }
    pub fn launch<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        inputs: SelectInputs<'_, T>,
        output: &mut CudaSlice<T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        validate_buffers(
            &[
                (inputs.mask.len(), self.required[0]),
                (inputs.yes.len(), self.required[1]),
                (inputs.no.len(), self.required[2]),
            ],
            output.len(),
            self.shape.numel(),
            metadata.len(),
            self.metadata.len(),
        )?;
        if self.count == 0 {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(inputs.mask)
                .arg(inputs.yes)
                .arg(inputs.no)
                .arg(output)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.rank)
                .arg(&self.offsets[0])
                .arg(&self.offsets[1])
                .arg(&self.offsets[2])
                .launch(rt.config(self.shape.numel()))?;
        }
        Ok(())
    }
}

fn validate_buffers(
    inputs: &[(usize, usize)],
    output: usize,
    count: usize,
    metadata: usize,
    metadata_count: usize,
) -> Result<(), CudaError> {
    if inputs.iter().any(|&(actual, required)| actual < required)
        || output < count
        || metadata < metadata_count
    {
        return Err(CudaError::InvalidInput(
            "typed dispatch storage is too small",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shape(dims: &[usize]) -> Shape {
        Shape::new(dims.to_vec()).unwrap()
    }
    #[test]
    fn copy_preserves_offset_and_raw_low_width() {
        let layout = Layout::new(shape(&[2, 3]), vec![7, 2], 5).unwrap();
        let pass = CopyDispatch::new::<u16>(&layout).unwrap();
        assert_eq!(pass.metadata, [2, 3, 7, 2]);
        assert_eq!((pass.offset, pass.input_required, pass.count), (5, 17, 6));
        let huge = Layout::contiguous(shape(&[usize::MAX / 4 + 1])).unwrap();
        assert!(CopyDispatch::new::<u16>(&huge).is_ok());
        assert!(CopyDispatch::new::<f32>(&huge).is_err());
    }
    #[test]
    fn select_broadcasts_three_distinct_offsets() {
        let mask = Layout::new(shape(&[2, 1]), vec![4, 1], 3).unwrap();
        let yes = Layout::new(shape(&[3]), vec![2], 5).unwrap();
        let no = Layout::new(shape(&[]), vec![], 9).unwrap();
        let pass = SelectDispatch::new::<u16>(&mask, &yes, &no).unwrap();
        assert_eq!(pass.shape, shape(&[2, 3]));
        assert_eq!(pass.offsets, [3, 5, 9]);
        assert_eq!(pass.metadata, [2, 3, 4, 0, 0, 2, 0, 0]);
        assert_eq!(pass.required, [8, 10, 10]);
        let compare = CompareDispatch::new(CompareOp::Less, &mask, &yes).unwrap();
        assert_eq!(compare.metadata, [2, 3, 4, 0, 0, 2]);
        assert_eq!(compare.required, [8, 10]);
    }
    #[test]
    fn dispatch_bounds_prevent_short_storage() {
        assert!(validate_buffers(&[(6, 7)], 4, 4, 8, 8).is_err());
        assert!(validate_buffers(&[(7, 7)], 3, 4, 8, 8).is_err());
        assert!(validate_buffers(&[(7, 7)], 4, 4, 7, 8).is_err());
        assert!(validate_buffers(&[(7, 7)], 4, 4, 8, 8).is_ok());
    }
}
