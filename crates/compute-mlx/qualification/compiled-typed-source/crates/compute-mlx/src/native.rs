use crate::ffi::{self, Api, Handle};
use std::{
    ffi::{CStr, c_void},
    rc::Rc,
    sync::Arc,
};
use tensor_core::{BinaryOp, CompareOp, HasShape, MatmulPrecision, Shape, TensorError, UnaryOp};

mod attention;
mod compiled;
mod custom_metal;
mod index;
mod low_attention;
mod low_index;
mod low_kernels;
mod low_matmul;
mod low_ops;
mod low_precision;
mod low_scatter;
mod low_statistics;
mod lowering;
mod reduction;
mod scatter;
mod statistics;
pub use compiled::{
    MlxCompiledProgram, MlxProgramBuilder, MlxProgramInput, MlxProgramOutput, MlxValue,
};
pub use low_precision::MlxLowTensor;

#[derive(Debug)]
pub enum MlxError {
    Unavailable(String),
    Native {
        operation: &'static str,
        message: String,
    },
    Contract(TensorError),
    ForeignContext,
    Dtype,
    UnsupportedPrecision(MatmulPrecision),
    UnsupportedLowPrecision {
        dtype: tensor_core::LowDtype,
        operation: &'static str,
    },
    InvalidIndices,
    TooLarge,
    CompileUnavailable,
    ForeignValue,
    ProgramInputCount {
        expected: usize,
        actual: usize,
    },
    ProgramInputShape {
        index: usize,
        expected: Shape,
        actual: Shape,
    },
}
impl std::fmt::Display for MlxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(e) => write!(f, "MLX backend unavailable: {e}"),
            Self::Native { operation, message } => write!(f, "MLX {operation}: {message}"),
            Self::Contract(e) => e.fmt(f),
            Self::ForeignContext => write!(f, "tensor belongs to another MLX backend"),
            Self::Dtype => write!(f, "tensor has an incompatible dtype"),
            Self::UnsupportedPrecision(p) => {
                write!(f, "MLX adapter does not implement precision policy {p:?}")
            }
            Self::UnsupportedLowPrecision { dtype, operation } => {
                write!(f, "MLX adapter does not support {operation} for {dtype:?}")
            }
            Self::InvalidIndices => write!(f, "gather index exceeds axis length"),
            Self::TooLarge => write!(f, "tensor exceeds MLX C dimension limits"),
            Self::CompileUnavailable => write!(f, "MLX runtime lacks the complete compilation API"),
            Self::ForeignValue => write!(f, "value belongs to another MLX program builder"),
            Self::ProgramInputCount { expected, actual } => {
                write!(f, "program received {actual} inputs; expected {expected}")
            }
            Self::ProgramInputShape {
                index,
                expected,
                actual,
            } => write!(
                f,
                "program input {index} has shape {actual:?}; expected {expected:?}"
            ),
        }
    }
}
impl std::error::Error for MlxError {}
impl From<TensorError> for MlxError {
    fn from(e: TensorError) -> Self {
        Self::Contract(e)
    }
}
fn native<T>(operation: &'static str, result: Result<T, String>) -> Result<T, MlxError> {
    result.map_err(|message| MlxError::Native { operation, message })
}

fn unary_function(a: &Api, op: UnaryOp) -> ffi::Unary {
    match op {
        UnaryOp::Negate => a.negative,
        UnaryOp::Abs => a.abs,
        UnaryOp::Square => a.square,
        UnaryOp::Sqrt => a.sqrt,
        UnaryOp::Reciprocal => a.reciprocal,
        UnaryOp::Exp => a.exp,
        UnaryOp::Log => a.log,
        UnaryOp::Sin => a.sin,
        UnaryOp::Cos => a.cos,
    }
}

fn binary_function(a: &Api, op: BinaryOp) -> ffi::Binary {
    match op {
        BinaryOp::Add => a.add,
        BinaryOp::Subtract => a.subtract,
        BinaryOp::Multiply => a.multiply,
        BinaryOp::Divide => a.divide,
        BinaryOp::Min => a.minimum,
        BinaryOp::Max => a.maximum,
    }
}

fn compare_function(a: &Api, op: CompareOp) -> ffi::Binary {
    match op {
        CompareOp::Equal => a.equal,
        CompareOp::NotEqual => a.not_equal,
        CompareOp::Less => a.less,
        CompareOp::LessEqual => a.less_equal,
        CompareOp::Greater => a.greater,
        CompareOp::GreaterEqual => a.greater_equal,
    }
}
fn reduce_function(a: &Api, op: tensor_core::ReduceOp) -> ffi::Reduce {
    match op {
        tensor_core::ReduceOp::Sum => a.sum_axes,
        tensor_core::ReduceOp::Product => a.prod_axes,
        tensor_core::ReduceOp::Min => a.min_axes,
        tensor_core::ReduceOp::Max => a.max_axes,
    }
}

// Called only while Api::call holds the native lock. The caller owns the raw
// handle and releases it on failure; this helper neither locks nor transfers it.
fn validate_array(api: &Api, raw: Handle, shape: &Shape, dtype: MlxDtype) -> Result<(), String> {
    if raw.ctx.is_null() {
        return Err("MLX returned a null array".into());
    }
    unsafe {
        let rank = (api.array_ndim)(raw);
        let dimensions = (api.array_shape)(raw);
        let same = rank == shape.rank()
            && (rank == 0
                || (!dimensions.is_null()
                    && std::slice::from_raw_parts(dimensions, rank)
                        .iter()
                        .zip(shape.dims())
                        .all(|(&actual, &expected)| actual >= 0 && actual as usize == expected)));
        if !same || (api.array_dtype)(raw) != dtype.raw() || (api.array_size)(raw) != shape.numel()
        {
            return Err("MLX result shape or dtype differs from validated contract".into());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MlxDtype {
    F32,
    U32,
    F16,
    Bf16,
}
impl MlxDtype {
    fn raw(self) -> i32 {
        match self {
            Self::F32 => ffi::F32,
            Self::U32 => ffi::U32,
            Self::F16 => ffi::F16,
            Self::Bf16 => ffi::BF16,
        }
    }
}

struct Context {
    api: Arc<Api>,
    stream: Handle,
    version: String,
    kernels: std::cell::RefCell<
        std::collections::HashMap<custom_metal::KernelKey, Rc<custom_metal::Kernel>>,
    >,
}
impl Drop for Context {
    fn drop(&mut self) {
        let _ = self.api.call(|a| unsafe {
            let completion = Api::check((a.synchronize)(self.stream));
            let released = Api::check((a.stream_free)(self.stream));
            completion.and(released)
        });
    }
}
struct Array {
    raw: Handle,
    context: Rc<Context>,
}
impl Drop for Array {
    fn drop(&mut self) {
        let _ = self
            .context
            .api
            .call(|a| unsafe { Api::check((a.array_free)(self.raw)) });
    }
}
/// An immutable lazy tensor. Clones share a native MLX array; subsequent
/// operations produce new arrays. Handles are deliberately not Send or Sync.
#[derive(Clone)]
pub struct MlxTensor {
    array: Rc<Array>,
    shape: Shape,
    dtype: MlxDtype,
}
impl HasShape for MlxTensor {
    fn shape(&self) -> &Shape {
        &self.shape
    }
}
impl MlxTensor {
    pub fn dtype(&self) -> MlxDtype {
        self.dtype
    }
}

/// Explicit MLX GPU execution. Each backend owns a stream and validates tensor
/// ownership. The process-wide C error callback is installed once; callers that
/// also use MLX-C directly must not replace it. Native calls are serialized.
#[derive(Clone)]
pub struct MlxBackend {
    context: Rc<Context>,
}
impl MlxBackend {
    pub fn new_gpu() -> Result<Self, MlxError> {
        let api = Api::global().map_err(MlxError::Unavailable)?;
        let (stream, version) = native(
            "initialize",
            api.call(|a| unsafe {
                let mut available = false;
                Api::check((a.metal_is_available)(&mut available))?;
                if !available {
                    return Err("Metal GPU is unavailable".into());
                }
                let device = (a.device_new_type)(ffi::GPU, 0);
                if device.ctx.is_null() {
                    return Err("GPU device creation failed".into());
                }
                let mut available = false;
                let checked = Api::check((a.device_is_available)(&mut available, device));
                if checked.is_err() || !available {
                    (a.device_free)(device);
                    checked?;
                    return Err("GPU device unavailable".into());
                }
                let stream = (a.stream_new_device)(device);
                (a.device_free)(device);
                if stream.ctx.is_null() {
                    return Err("GPU stream creation failed".into());
                }
                let mut version = (a.string_new)();
                if let Err(e) = Api::check((a.version)(&mut version)) {
                    (a.string_free)(version);
                    (a.stream_free)(stream);
                    return Err(e);
                }
                let ptr = (a.string_data)(version);
                let text = if ptr.is_null() {
                    "unknown".to_owned()
                } else {
                    CStr::from_ptr(ptr).to_string_lossy().into_owned()
                };
                (a.string_free)(version);
                Ok((stream, text))
            }),
        )?;
        Ok(Self {
            context: Rc::new(Context {
                api,
                stream,
                version,
                kernels: Default::default(),
            }),
        })
    }
    pub fn version(&self) -> &str {
        &self.context.version
    }
    fn check(&self, t: &MlxTensor, dtype: Option<MlxDtype>) -> Result<(), MlxError> {
        if !Rc::ptr_eq(&self.context, &t.array.context) {
            return Err(MlxError::ForeignContext);
        }
        if dtype.is_some_and(|d| d != t.dtype) {
            return Err(MlxError::Dtype);
        }
        Ok(())
    }
    fn dimensions(shape: &Shape) -> Result<Vec<i32>, MlxError> {
        if shape.numel() > isize::MAX as usize / 4 || shape.rank() > i32::MAX as usize {
            return Err(MlxError::TooLarge);
        }
        shape
            .dims()
            .iter()
            .map(|&n| i32::try_from(n).map_err(|_| MlxError::TooLarge))
            .collect()
    }
    fn output(
        &self,
        operation: &'static str,
        shape: Shape,
        dtype: MlxDtype,
        record: impl FnOnce(&Api, *mut Handle) -> i32,
    ) -> Result<MlxTensor, MlxError> {
        Self::dimensions(&shape)?;
        let raw = native(
            operation,
            self.context.api.call(|a| unsafe {
                let mut raw = (a.array_new)();
                if let Err(e) = Api::check(record(a, &mut raw)) {
                    (a.array_free)(raw);
                    return Err(e);
                }
                if let Err(error) = validate_array(a, raw, &shape, dtype) {
                    (a.array_free)(raw);
                    return Err(error);
                }
                Ok(raw)
            }),
        )?;
        Ok(MlxTensor {
            array: Rc::new(Array {
                raw,
                context: self.context.clone(),
            }),
            shape,
            dtype,
        })
    }
    fn upload(
        &self,
        shape: Shape,
        ptr: *const c_void,
        len: usize,
        dtype: MlxDtype,
    ) -> Result<MlxTensor, MlxError> {
        if shape.numel() != len {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: len,
            }
            .into());
        }
        let dims = Self::dimensions(&shape)?;
        let rank = i32::try_from(dims.len()).map_err(|_| MlxError::TooLarge)?;
        self.output("upload", shape, dtype, |a, out| unsafe {
            (a.array_set_data)(out, ptr, dims.as_ptr(), rank, dtype.raw())
        })
    }
    pub fn upload_f32(&self, shape: Shape, values: &[f32]) -> Result<MlxTensor, MlxError> {
        self.upload(shape, values.as_ptr().cast(), values.len(), MlxDtype::F32)
    }
    pub fn upload_u32(&self, shape: Shape, values: &[u32]) -> Result<MlxTensor, MlxError> {
        self.upload(shape, values.as_ptr().cast(), values.len(), MlxDtype::U32)
    }
    pub fn eval(&self, t: &MlxTensor) -> Result<(), MlxError> {
        self.check(t, None)?;
        native(
            "eval",
            self.context
                .api
                .call(|a| unsafe { Api::check((a.array_eval)(t.array.raw)) }),
        )
    }
    pub fn synchronize(&self) -> Result<(), MlxError> {
        native(
            "synchronize",
            self.context
                .api
                .call(|a| unsafe { Api::check((a.synchronize)(self.context.stream)) }),
        )
    }
    pub fn materialize(&self, t: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.check(t, None)?;
        self.output("contiguous", t.shape.clone(), t.dtype, |a, out| unsafe {
            (a.contiguous)(out, t.array.raw, false, self.context.stream)
        })
    }
    pub fn read_f32(&self, t: &MlxTensor) -> Result<Vec<f32>, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        let t = self.materialize(t)?;
        native(
            "read_f32",
            self.context.api.call(|a| unsafe {
                Api::check((a.array_eval)(t.array.raw))?;
                if t.shape.numel() == 0 {
                    return Ok(Vec::new());
                }
                let ptr = (a.array_data_float32)(t.array.raw);
                if ptr.is_null() {
                    return Err("evaluated f32 array has no data".into());
                }
                Ok(std::slice::from_raw_parts(ptr, t.shape.numel()).to_vec())
            }),
        )
    }
    pub fn read_u32(&self, t: &MlxTensor) -> Result<Vec<u32>, MlxError> {
        self.check(t, Some(MlxDtype::U32))?;
        let t = self.materialize(t)?;
        native(
            "read_u32",
            self.context.api.call(|a| unsafe {
                Api::check((a.array_eval)(t.array.raw))?;
                if t.shape.numel() == 0 {
                    return Ok(Vec::new());
                }
                let ptr = (a.array_data_uint32)(t.array.raw);
                if ptr.is_null() {
                    return Err("evaluated u32 array has no data".into());
                }
                Ok(std::slice::from_raw_parts(ptr, t.shape.numel()).to_vec())
            }),
        )
    }
}

impl MlxBackend {
    pub fn reshape(&self, t: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(t, None)?;
        if shape.numel() != t.shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: t.shape.numel(),
                actual: shape.numel(),
            }
            .into());
        }
        let dims = Self::dimensions(&shape)?;
        let input = self.materialize(t)?;
        self.output("reshape", shape, t.dtype, |a, out| unsafe {
            (a.reshape)(
                out,
                input.array.raw,
                dims.as_ptr(),
                dims.len(),
                self.context.stream,
            )
        })
    }
    pub fn permute(&self, t: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(t, None)?;
        let shape = t.shape.permute(axes)?;
        let axes: Vec<i32> = axes
            .iter()
            .map(|&x| i32::try_from(x).map_err(|_| MlxError::TooLarge))
            .collect::<Result<_, _>>()?;
        self.output("permute", shape, t.dtype, |a, out| unsafe {
            (a.transpose_axes)(
                out,
                t.array.raw,
                axes.as_ptr(),
                axes.len(),
                self.context.stream,
            )
        })
    }
    pub fn broadcast_to(&self, t: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(t, None)?;
        if t.shape.broadcast(&shape)? != shape {
            return Err(TensorError::IncompatibleBroadcast {
                left: t.shape.dims().to_vec(),
                right: shape.dims().to_vec(),
            }
            .into());
        }
        let dims = Self::dimensions(&shape)?;
        self.output("broadcast", shape, t.dtype, |a, out| unsafe {
            (a.broadcast_to)(
                out,
                t.array.raw,
                dims.as_ptr(),
                dims.len(),
                self.context.stream,
            )
        })
    }
    pub fn unary(&self, op: UnaryOp, t: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        self.output("unary", t.shape.clone(), MlxDtype::F32, |a, out| unsafe {
            let operation = unary_function(a, op);
            operation(out, t.array.raw, self.context.stream)
        })
    }
    pub fn binary(
        &self,
        op: BinaryOp,
        left: &MlxTensor,
        right: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(left, Some(MlxDtype::F32))?;
        self.check(right, Some(MlxDtype::F32))?;
        let shape = left.shape.broadcast(&right.shape)?;
        self.output("binary", shape, MlxDtype::F32, |a, out| unsafe {
            let operation = binary_function(a, op);
            operation(out, left.array.raw, right.array.raw, self.context.stream)
        })
    }
    /// Comparisons produce exact u32 zero/one masks on the GPU.
    pub fn compare(
        &self,
        op: CompareOp,
        left: &MlxTensor,
        right: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(left, Some(MlxDtype::F32))?;
        self.check(right, Some(MlxDtype::F32))?;
        self.compare_values(op, left, right)
    }
    fn compare_values(
        &self,
        op: CompareOp,
        left: &MlxTensor,
        right: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(left, None)?;
        self.check(right, Some(left.dtype))?;
        let shape = left.shape.broadcast(&right.shape)?;
        self.output("compare", shape, MlxDtype::U32, |a, out| unsafe {
            let operation = compare_function(a, op);
            let mut mask = (a.array_new)();
            let code = operation(
                &mut mask,
                left.array.raw,
                right.array.raw,
                self.context.stream,
            );
            let code = if code == 0 {
                (a.astype)(out, mask, ffi::U32, self.context.stream)
            } else {
                code
            };
            (a.array_free)(mask);
            code
        })
    }
    /// Sum over checked axes. An empty axes list preserves the input.
    pub fn sum_axes(
        &self,
        t: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.reduce_values(tensor_core::ReduceOp::Sum, t, axes, keep_dims)
    }
    pub fn matmul(
        &self,
        left: &MlxTensor,
        right: &MlxTensor,
        precision: MatmulPrecision,
    ) -> Result<MlxTensor, MlxError> {
        self.check(left, Some(MlxDtype::F32))?;
        self.check(right, Some(MlxDtype::F32))?;
        let shape = tensor_core::matmul_shape(&left.shape, &right.shape)?;
        if precision != MatmulPrecision::F32 {
            return Err(MlxError::UnsupportedPrecision(precision));
        }
        // No products exist for an empty output. MLX 0.32.1 Metal crashes when
        // evaluating some empty-batch vector matmuls; use a native empty array.
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        self.output("matmul", shape, MlxDtype::F32, |a, out| unsafe {
            (a.matmul)(out, left.array.raw, right.array.raw, self.context.stream)
        })
    }
    /// Inclusive or exclusive prefix sum along one axis, optionally reversed.
    /// u32 sums wrap modulo 2^32; f32 sums follow MLX reduction ordering.
    pub fn scan(
        &self,
        t: &MlxTensor,
        axis: usize,
        inclusive: bool,
        reverse: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(t, None)?;
        t.shape.validate_axes(&[axis])?;
        let axis = i32::try_from(axis).map_err(|_| MlxError::TooLarge)?;
        self.output("scan", t.shape.clone(), t.dtype, |a, out| unsafe {
            (a.cumsum)(
                out,
                t.array.raw,
                axis,
                reverse,
                inclusive,
                self.context.stream,
            )
        })
    }
    /// Gather with host-provided indices. Values and the resulting gather stay
    /// on the GPU; bounds are checked before uploading the index vector.
    pub fn gather_axis(
        &self,
        t: &MlxTensor,
        indices: &[u32],
        axis: usize,
    ) -> Result<MlxTensor, MlxError> {
        self.check(t, None)?;
        t.shape.validate_axes(&[axis])?;
        if indices.iter().any(|&i| i as usize >= t.shape.dims()[axis]) {
            return Err(MlxError::InvalidIndices);
        }
        let mut dims = t.shape.dims().to_vec();
        dims[axis] = indices.len();
        let shape = Shape::new(dims)?;
        let uploaded = self.upload_u32(Shape::new(vec![indices.len()])?, indices)?;
        let axis = i32::try_from(axis).map_err(|_| MlxError::TooLarge)?;
        self.output("gather", shape, t.dtype, |a, out| unsafe {
            (a.take_axis)(
                out,
                t.array.raw,
                uploaded.array.raw,
                axis,
                self.context.stream,
            )
        })
    }
}

impl tensor_core::TensorBackend for MlxBackend {
    type Tensor = MlxTensor;
    type Error = MlxError;
    fn kind(&self) -> tensor_core::BackendKind {
        tensor_core::BackendKind::Mlx
    }
    fn upload_f32(&self, shape: Shape, values: &[f32]) -> Result<MlxTensor, MlxError> {
        self.upload_f32(shape, values)
    }
    fn read_f32(&self, t: &MlxTensor) -> Result<Vec<f32>, MlxError> {
        self.read_f32(t)
    }
    fn materialize(&self, t: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        self.materialize(t)
    }
    fn reshape(&self, t: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        self.reshape(t, shape)
    }
    fn permute(&self, t: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        self.permute(t, axes)
    }
    fn broadcast_to(&self, t: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        self.broadcast_to(t, shape)
    }
    fn unary(&self, op: UnaryOp, t: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.unary(op, t)
    }
    fn binary(&self, op: BinaryOp, a: &MlxTensor, b: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.binary(op, a, b)
    }
    fn sum_axes(
        &self,
        t: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(t, Some(MlxDtype::F32))?;
        self.sum_axes(t, axes, keep_dims)
    }
    fn matmul(
        &self,
        a: &MlxTensor,
        b: &MlxTensor,
        precision: MatmulPrecision,
    ) -> Result<MlxTensor, MlxError> {
        self.matmul(a, b, precision)
    }
}
