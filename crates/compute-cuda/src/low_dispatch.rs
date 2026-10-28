//! Native u16 storage launch descriptors shared by eager and prepared calls.
//! Layout traversal comes from the ordinary copy/binary geometry. Kernels keep
//! the existing integer codecs and direct f32 arithmetic without expanded inputs.
use crate::{CudaError, CudaRuntime, dispatch::BinaryDispatch, indexing::dispatch::CopyDispatch};
use gpu_compute::cuda::{CudaSlice, PushKernelArg};
use tensor_core::{BinaryOp, Layout, LowDtype, UnaryOp};

#[derive(Clone, Debug)]
pub(crate) struct LowCastDispatch {
    pub geometry: CopyDispatch,
    dtype: u32,
}
impl LowCastDispatch {
    pub fn new(input: &Layout, dtype: LowDtype) -> Result<Self, CudaError> {
        // One side is f32, so both directions need the wider allocation bound.
        Ok(Self {
            geometry: CopyDispatch::new::<f32>(input)?,
            dtype: dtype as u32,
        })
    }
    pub fn launch_encode(
        &self,
        rt: &CudaRuntime,
        input: &CudaSlice<f32>,
        output: &mut CudaSlice<u16>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.launch(rt, &rt.low.encode, input, output, metadata)
    }
    pub fn launch_decode(
        &self,
        rt: &CudaRuntime,
        input: &CudaSlice<u16>,
        output: &mut CudaSlice<f32>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        self.launch(rt, &rt.low.decode, input, output, metadata)
    }
    fn launch<
        I: gpu_compute::cuda::cudarc::driver::DeviceRepr,
        O: gpu_compute::cuda::cudarc::driver::DeviceRepr,
    >(
        &self,
        rt: &CudaRuntime,
        kernel: &gpu_compute::cuda::CudaFunction,
        input: &CudaSlice<I>,
        output: &mut CudaSlice<O>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let geometry = &self.geometry;
        if geometry.count == 0 {
            return Ok(());
        }
        // The caller validates storage/ownership and prepares metadata once.
        // cudarc retains access guards for the current allocations on replay.
        unsafe {
            rt.device
                .stream
                .launch_builder(kernel)
                .arg(input)
                .arg(output)
                .arg(metadata)
                .arg(&geometry.count)
                .arg(&geometry.rank)
                .arg(&geometry.offset)
                .arg(&self.dtype)
                .launch(rt.config(geometry.shape.numel()))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LowUnaryDispatch {
    pub geometry: CopyDispatch,
    op: u32,
    dtype: u32,
}
impl LowUnaryDispatch {
    pub fn new(input: &Layout, op: UnaryOp, dtype: LowDtype) -> Result<Self, CudaError> {
        Self::new_code(input, op as u32, dtype)
    }
    pub fn new_code(input: &Layout, op: u32, dtype: LowDtype) -> Result<Self, CudaError> {
        if op > UnaryOp::Cos as u32 {
            return Err(CudaError::InvalidInput(
                "invalid low unary kernel operation",
            ));
        }
        Ok(Self {
            geometry: CopyDispatch::new::<u16>(input)?,
            op,
            dtype: dtype as u32,
        })
    }
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        input: &CudaSlice<u16>,
        output: &mut CudaSlice<u16>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let geometry = &self.geometry;
        if geometry.count == 0 {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(&rt.low.unary)
                .arg(input)
                .arg(output)
                .arg(metadata)
                .arg(&geometry.count)
                .arg(&geometry.rank)
                .arg(&geometry.offset)
                .arg(&self.op)
                .arg(&self.dtype)
                .launch(rt.config(geometry.shape.numel()))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LowBinaryDispatch {
    pub geometry: BinaryDispatch,
    dtype: u32,
}
impl LowBinaryDispatch {
    /// Both operands have this dtype; callers reject a mixed pair before shape
    /// validation, including empty shapes, using the shared tensor contract.
    pub fn new(
        left: &Layout,
        right: &Layout,
        op: BinaryOp,
        dtype: LowDtype,
    ) -> Result<Self, CudaError> {
        Ok(Self {
            geometry: BinaryDispatch::new_typed::<u16>(left, right, op)?,
            dtype: dtype as u32,
        })
    }
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        left: &CudaSlice<u16>,
        right: &CudaSlice<u16>,
        output: &mut CudaSlice<u16>,
        metadata: &CudaSlice<u64>,
    ) -> Result<(), CudaError> {
        let geometry = &self.geometry;
        if geometry.count == 0 {
            return Ok(());
        }
        unsafe {
            rt.device
                .stream
                .launch_builder(&rt.low.binary)
                .arg(left)
                .arg(right)
                .arg(output)
                .arg(metadata)
                .arg(&geometry.count)
                .arg(&geometry.rank)
                .arg(&geometry.left_offset)
                .arg(&geometry.right_offset)
                .arg(&geometry.op)
                .arg(&self.dtype)
                .launch(rt.config(geometry.shape.numel()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tensor_core::Shape;
    fn layout(dims: &[usize], strides: &[usize], offset: usize) -> Layout {
        Layout::new(Shape::new(dims.to_vec()).unwrap(), strides.to_vec(), offset).unwrap()
    }
    #[test]
    fn scalar_cast_and_strided_unary_keep_native_abi_metadata() {
        let scalar = LowCastDispatch::new(&layout(&[], &[], 9), LowDtype::Bf16).unwrap();
        assert!(scalar.geometry.metadata.is_empty());
        assert_eq!(
            (
                scalar.geometry.count,
                scalar.geometry.rank,
                scalar.geometry.offset
            ),
            (1, 0, 9)
        );
        assert_eq!(scalar.dtype, 1);
        let unary =
            LowUnaryDispatch::new(&layout(&[2, 3], &[1, 7], 5), UnaryOp::Abs, LowDtype::F16)
                .unwrap();
        assert_eq!(unary.geometry.metadata, [2, 3, 1, 7]);
        assert_eq!(
            (
                unary.geometry.count,
                unary.geometry.offset,
                unary.op,
                unary.dtype
            ),
            (6, 5, 1, 0)
        );
    }
    #[test]
    fn binary_broadcast_preserves_strides_offsets_and_unsigned_dtype_tag() {
        let pass = LowBinaryDispatch::new(
            &layout(&[2, 1], &[7, 1], 3),
            &layout(&[3], &[2], 5),
            BinaryOp::Multiply,
            LowDtype::Bf16,
        )
        .unwrap();
        assert_eq!(pass.geometry.metadata, [2, 3, 7, 0, 0, 2]);
        assert_eq!(
            (
                pass.geometry.left_offset,
                pass.geometry.right_offset,
                pass.geometry.count
            ),
            (3, 5, 6)
        );
        assert_eq!((pass.geometry.op, pass.dtype), (2, 1));
    }
    #[test]
    fn empty_shapes_skip_work_and_casts_check_both_storage_widths() {
        let empty = LowUnaryDispatch::new(
            &layout(&[0, 3], &[usize::MAX, 1], 0),
            UnaryOp::Negate,
            LowDtype::F16,
        )
        .unwrap();
        assert_eq!(empty.geometry.count, 0);
        assert!(LowUnaryDispatch::new_code(&layout(&[], &[], 0), 9, LowDtype::F16).is_err());
        assert!(LowUnaryDispatch::new_code(&layout(&[], &[], 0), 10, LowDtype::F16).is_err());
        assert!(LowUnaryDispatch::new_code(&layout(&[], &[], 0), u32::MAX, LowDtype::F16).is_err());
        let large = layout(&[usize::MAX / 4 + 1], &[0], 0);
        assert!(LowUnaryDispatch::new(&large, UnaryOp::Abs, LowDtype::Bf16).is_ok());
        assert!(LowCastDispatch::new(&large, LowDtype::Bf16).is_err());
        assert!(
            LowBinaryDispatch::new(&large, &layout(&[], &[], 0), BinaryOp::Add, LowDtype::Bf16)
                .is_ok()
        );
        let overflow = layout(&[usize::MAX / 2 + 1], &[0], 0);
        assert!(LowUnaryDispatch::new(&overflow, UnaryOp::Abs, LowDtype::Bf16).is_err());
    }
}
