//! Checked elementwise launch descriptors shared by eager and prepared calls.
use crate::{
    CudaError, CudaRuntime,
    runtime::{layout_metadata, rank},
};
use gpu_compute::cuda::{CudaFunction, CudaSlice, PushKernelArg, cudarc::driver::DeviceRepr};
use tensor_core::{BinaryOp, Layout, Shape};

#[derive(Clone, Debug)]
pub(crate) struct UnaryDispatch<T = f32> {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    count: u64,
    rank: u32,
    offset: u64,
    op: u32,
    scale: T,
    bias: T,
}
impl<T: DeviceRepr> UnaryDispatch<T> {
    pub fn new(input: &Layout, op: u32, scale: T, bias: T) -> Result<Self, CudaError> {
        if op > 11 {
            return Err(CudaError::InvalidInput("invalid unary kernel operation"));
        }
        crate::runtime::validate_logical_size::<T>(input.shape())?;
        Ok(Self {
            metadata: layout_metadata(input),
            shape: input.shape().clone(),
            count: input.shape().numel() as u64,
            rank: rank(input.shape())?,
            offset: input.offset() as u64,
            op,
            scale,
            bias,
        })
    }
    pub fn launch_loaded(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        input: &CudaSlice<T>,
        output: &mut CudaSlice<T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        if self.count == 0 {
            return Ok(());
        }
        // The caller validates layouts/storage and distinct output ownership;
        // the same argument ABI serves eager and prepared execution. cudarc
        // retains read/write access events through each argument guard.
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
                .arg(&self.op)
                .arg(&self.scale)
                .arg(&self.bias)
                .launch(rt.config(self.shape.numel()))?;
        }
        Ok(())
    }
}

impl UnaryDispatch<f32> {
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        input: &CudaSlice<f32>,
        output: &mut CudaSlice<f32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.launch_loaded(rt, &rt.unary, input, output, metadata)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BinaryDispatch {
    pub metadata: Vec<u64>,
    pub shape: Shape,
    pub count: u64,
    pub rank: u32,
    pub left_offset: u64,
    pub right_offset: u64,
    pub op: u32,
}
impl BinaryDispatch {
    pub fn new(left: &Layout, right: &Layout, op: BinaryOp) -> Result<Self, CudaError> {
        Self::new_typed::<f32>(left, right, op)
    }
    pub fn new_typed<T: DeviceRepr>(
        left: &Layout,
        right: &Layout,
        op: BinaryOp,
    ) -> Result<Self, CudaError> {
        let (shape, metadata) = binary_metadata::<T>(left, right)?;
        Ok(Self {
            metadata,
            count: shape.numel() as u64,
            rank: rank(&shape)?,
            shape,
            left_offset: left.offset() as u64,
            right_offset: right.offset() as u64,
            op: op as u32,
        })
    }
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        left: &CudaSlice<f32>,
        right: &CudaSlice<f32>,
        output: &mut CudaSlice<f32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.launch_loaded(rt, &rt.binary, (left, right), output, metadata)
    }
    pub fn launch_u32(
        &self,
        rt: &CudaRuntime,
        left: &CudaSlice<u32>,
        right: &CudaSlice<u32>,
        output: &mut CudaSlice<u32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        if self.op == BinaryOp::Divide as u32 {
            return Err(CudaError::InvalidInput("u32 division is not supported"));
        }
        self.launch_loaded(rt, &rt.binary_u32, (left, right), output, metadata)
    }
    pub(crate) fn launch_loaded<T: DeviceRepr>(
        &self,
        rt: &CudaRuntime,
        kernel: &CudaFunction,
        inputs: (&CudaSlice<T>, &CudaSlice<T>),
        output: &mut CudaSlice<T>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        if self.count == 0 {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(inputs.0)
                .arg(inputs.1)
                .arg(output)
                .arg(metadata)
                .arg(&self.count)
                .arg(&self.rank)
                .arg(&self.left_offset)
                .arg(&self.right_offset)
                .arg(&self.op)
                .launch(rt.config(self.shape.numel()))?;
        }
        Ok(())
    }
}

/// Shared broadcast traversal for arithmetic and comparisons. The type denotes
/// output storage width; callers validate each input's storage separately.
pub(crate) fn binary_metadata<T>(
    left: &Layout,
    right: &Layout,
) -> Result<(Shape, Vec<u64>), CudaError> {
    let shape = left.shape().broadcast(right.shape())?;
    crate::runtime::validate_logical_size::<T>(&shape)?;
    let left = left.broadcast_to(shape.clone())?;
    let right = right.broadcast_to(shape.clone())?;
    let mut metadata = Vec::with_capacity(shape.rank() * 3);
    metadata.extend(left.shape().dims().iter().map(|&v| v as u64));
    metadata.extend(left.strides().iter().map(|&v| v as u64));
    metadata.extend(right.strides().iter().map(|&v| v as u64));
    Ok((shape, metadata))
}

pub(crate) fn fill(
    rt: &CudaRuntime,
    output: &mut CudaSlice<f32>,
    count: usize,
    value: f32,
) -> Result<(), CudaError> {
    fill_typed(rt, &rt.reduction.fill[0], output, count as u64, value)
}

pub(crate) fn fill_typed<T: DeviceRepr>(
    rt: &CudaRuntime,
    kernel: &CudaFunction,
    output: &mut CudaSlice<T>,
    count: u64,
    value: T,
) -> Result<(), CudaError> {
    if count > output.len() as u64 {
        return Err(CudaError::InvalidInput("fill output is too small"));
    }
    if count == 0 {
        return Ok(());
    }
    unsafe {
        rt.device
            .stream
            .launch_builder(kernel)
            .arg(output)
            .arg(&count)
            .arg(&value)
            .launch(rt.config(count as usize))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary64_descriptors_validate_eight_byte_storage() {
        let shape = Shape::new(vec![usize::MAX / 8 + 1]).unwrap();
        let broadcast = Layout::new(shape, vec![0], 0).unwrap();
        assert!(UnaryDispatch::new(&broadcast, 10, 1f32, 0f32).is_ok());
        assert!(UnaryDispatch::new(&broadcast, 10, 1f64, 0f64).is_err());
        assert!(BinaryDispatch::new_typed::<f64>(&broadcast, &broadcast, BinaryOp::Add).is_err());
    }
    #[test]
    fn shared_metadata_preserves_offsets_and_broadcast_strides() {
        let a = Layout::new(Shape::new(vec![2, 1]).unwrap(), vec![7, 1], 3).unwrap();
        let b = Layout::new(Shape::new(vec![3]).unwrap(), vec![2], 5).unwrap();
        let pass = BinaryDispatch::new(&a, &b, BinaryOp::Multiply).unwrap();
        assert_eq!(pass.metadata, [2, 3, 7, 0, 0, 2]);
        assert_eq!((pass.left_offset, pass.right_offset, pass.count), (3, 5, 6));
        let scalar = UnaryDispatch::new(
            &Layout::new(Shape::new(vec![]).unwrap(), vec![], 9).unwrap(),
            10,
            1.,
            0.,
        )
        .unwrap();
        assert_eq!((scalar.rank, scalar.offset, scalar.count), (0, 9, 1));
        assert!(scalar.metadata.is_empty());
    }
}
