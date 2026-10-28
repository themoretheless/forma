use crate::{Binding, ComputeRuntime, GpuArray, Kernel, TensorComputeError};
use std::time::Duration;
use tensor_core::{
    BinaryOp, HasShape, Layout, MatmulPlan, ReduceOp, Shape, TensorError, TensorEvalBackend,
    TensorF64Backend, UnaryOp, mean_shape, reduction_shape,
};
type Result<T> = std::result::Result<T, TensorComputeError>;

/// Exact binary64 storage in low/high u32 words. Shape and strides count f64
/// elements; no numerical conversion occurs at upload, readback or in views.
#[derive(Clone)]
pub struct GpuF64Tensor {
    words: GpuArray<u32>,
    layout: Layout,
}
impl HasShape for GpuF64Tensor {
    fn shape(&self) -> &Shape {
        self.layout.shape()
    }
}
impl GpuF64Tensor {
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    fn view(&self, layout: Layout) -> Result<Self> {
        layout.validate_storage_len(self.words.len() / 2)?;
        word_count(layout.shape())?;
        for value in layout
            .shape()
            .dims()
            .iter()
            .chain(layout.strides())
            .copied()
            .chain([layout.offset()])
        {
            index(value)?;
        }
        Ok(Self {
            words: self.words.clone(),
            layout,
        })
    }
}
fn index(value: usize) -> Result<u32> {
    u32::try_from(value).map_err(|_| TensorComputeError::IndexTooLarge)
}
fn word_count(shape: &Shape) -> Result<usize> {
    let words = shape
        .numel()
        .checked_mul(2)
        .ok_or(TensorComputeError::IndexTooLarge)?
        .max(2);
    index(words)?;
    Ok(words)
}
impl ComputeRuntime {
    fn zero_f64(&self, shape: Shape) -> Result<GpuF64Tensor> {
        let words = self.zeros(word_count(&shape)?)?;
        Ok(GpuF64Tensor {
            words,
            layout: Layout::contiguous(shape)?,
        })
    }
    fn check_f64(&self, tensor: &GpuF64Tensor) -> Result<()> {
        self.check(&tensor.words)?;
        Ok(())
    }
    fn f64_kernel(&self) -> Result<&Kernel> {
        if let Some(kernel) = self.binary64.get() {
            return Ok(kernel);
        }
        let source = format!(
            "{}\n{}\n{}",
            super::ARITHMETIC_WGSL,
            super::TRANSCENDENTAL_WGSL,
            include_str!("tensor.wgsl")
        );
        let kernel = Kernel::new(
            &self.device,
            "software binary64 tensor",
            &source,
            "main",
            &[
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageReadWrite,
                Binding::StorageRead,
            ],
        )?;
        let _ = self.binary64.set(kernel);
        Ok(self.binary64.get().expect("binary64 initialized"))
    }
    fn dispatch_f64(
        &self,
        mode: u32,
        op: u32,
        inputs: [&GpuF64Tensor; 2],
        shape: Shape,
        contraction: [usize; 3],
    ) -> Result<GpuF64Tensor> {
        self.check_f64(inputs[0])?;
        self.check_f64(inputs[1])?;
        let out = self.zero_f64(shape)?;
        if out.shape().is_empty() {
            return Ok(out);
        }
        let [a, b] = inputs;
        let mut metadata = vec![
            mode,
            op,
            index(out.shape().numel())?,
            index(a.shape().rank())?,
            index(b.shape().rank())?,
            index(a.layout.offset())?,
            index(b.layout.offset())?,
            index(contraction[0])?,
            index(contraction[1])?,
            index(contraction[2])?,
        ];
        for tensor in inputs {
            for &v in tensor.shape().dims().iter().chain(tensor.layout.strides()) {
                metadata.push(index(v)?);
            }
        }
        let meta = self.upload(&metadata)?;
        let kernel = self.f64_kernel()?;
        let groups = if mode == 2 {
            index(out.shape().numel())?
        } else {
            index(out.shape().numel())?.div_ceil(256)
        };
        let groups = groups
            .min(self.device.limits().max_compute_workgroups_per_dimension)
            .clamp(1, 65535);
        kernel.dispatch_groups(
            &self.device,
            &self.queue,
            &[
                a.words.buffer(),
                b.words.buffer(),
                out.words.buffer(),
                meta.buffer(),
            ],
            groups,
        );
        Ok(out)
    }
    fn copy_f64(&self, input: &GpuF64Tensor) -> Result<GpuF64Tensor> {
        self.dispatch_f64(0, 10, [input, input], input.shape().clone(), [0; 3])
    }
}
impl TensorF64Backend for ComputeRuntime {
    type F64Tensor = GpuF64Tensor;
    fn upload_f64(&self, shape: Shape, values: &[f64]) -> Result<GpuF64Tensor> {
        if shape.numel() != values.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        let words = self.upload_generated(word_count(&shape)?, |i| {
            // Empty tensors retain the same two-word zero allocation as before.
            let bits = values.get(i / 2).map_or(0, |value| value.to_bits());
            (bits >> ((i % 2) * 32)) as u32
        })?;
        Ok(GpuF64Tensor {
            words,
            layout: Layout::contiguous(shape)?,
        })
    }
    fn read_f64(&self, input: &GpuF64Tensor) -> Result<Vec<f64>> {
        self.check_f64(input)?;
        if input.shape().is_empty() {
            return Ok(Vec::new());
        }
        let dense = self.materialize_f64(input)?;
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut bytes = gpu_compute::ByteReadback::copy_buffer(
            &self.device,
            &mut encoder,
            dense.words.buffer(),
            0,
            input.shape().numel() as u64 * 8,
        )
        .map_err(crate::ComputeError::from)?;
        bytes.submitted(self.queue.submit([encoder.finish()]));
        Ok(bytes
            .wait_with(Duration::from_secs(30), |bytes| {
                bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|v| f64::from_le_bytes(*v))
                    .collect()
            })
            .map_err(crate::ComputeError::from)?)
    }
    fn materialize_f64(&self, input: &GpuF64Tensor) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        if input.layout.is_contiguous() && input.layout.offset() == 0 {
            Ok(input.clone())
        } else {
            self.copy_f64(input)
        }
    }
    fn reshape_f64(&self, input: &GpuF64Tensor, shape: Shape) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        if input.shape().numel() != shape.numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: input.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        let dense = self.materialize_f64(input)?;
        dense.view(dense.layout.reshape(shape)?)
    }
    fn permute_f64(&self, input: &GpuF64Tensor, axes: &[usize]) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        input.view(input.layout.permute(axes)?)
    }
    fn broadcast_f64(&self, input: &GpuF64Tensor, shape: Shape) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        input.view(input.layout.broadcast_to(shape)?)
    }
    fn narrow_f64(
        &self,
        input: &GpuF64Tensor,
        axis: usize,
        start: usize,
        len: usize,
    ) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        input.view(input.layout.narrow(axis, start, len)?)
    }
    fn unary_f64(&self, op: UnaryOp, input: &GpuF64Tensor) -> Result<GpuF64Tensor> {
        self.dispatch_f64(0, op as u32, [input, input], input.shape().clone(), [0; 3])
    }
    fn binary_f64(
        &self,
        op: BinaryOp,
        left: &GpuF64Tensor,
        right: &GpuF64Tensor,
    ) -> Result<GpuF64Tensor> {
        self.check_f64(left)?;
        self.check_f64(right)?;
        let shape = left.shape().broadcast(right.shape())?;
        let a = self.broadcast_f64(left, shape.clone())?;
        let b = self.broadcast_f64(right, shape.clone())?;
        self.dispatch_f64(1, op as u32, [&a, &b], shape, [0; 3])
    }
    fn reduce_f64(
        &self,
        op: ReduceOp,
        input: &GpuF64Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return self.materialize_f64(input);
        }
        let remaining: Vec<_> = (0..input.shape().rank())
            .filter(|i| !axes.contains(i))
            .collect();
        let part = |selected: &[usize], offset: usize| -> Result<GpuF64Tensor> {
            input.view(Layout::new(
                Shape::new(
                    selected
                        .iter()
                        .map(|&i| input.shape().dims()[i])
                        .collect::<Vec<_>>(),
                )?,
                selected
                    .iter()
                    .map(|&i| input.layout.strides()[i])
                    .collect::<Vec<_>>(),
                offset,
            )?)
        };
        // Empty input can have a nonempty logical remaining-axis layout without
        // any backing values. Kernels with a zero contraction never read it.
        if input.shape().is_empty() {
            let out = self.zero_f64(shape)?;
            if op == ReduceOp::Product && !out.shape().is_empty() {
                let one = self.upload_f64(Shape::new(vec![])?, &[1.])?;
                return self.copy_f64(&self.broadcast_f64(&one, out.shape().clone())?);
            }
            return Ok(out);
        }
        let a = part(&remaining, input.layout.offset())?;
        let b = part(axes, 0)?;
        self.dispatch_f64(2, op as u32, [&a, &b], shape, [b.shape().numel(), 0, 0])
    }
    fn mean_f64(
        &self,
        input: &GpuF64Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuF64Tensor> {
        self.check_f64(input)?;
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return self.materialize_f64(input);
        }
        if shape.is_empty() {
            return self.zero_f64(shape);
        }
        let divisor = self.upload_f64(
            Shape::new(vec![])?,
            &[(input.shape().numel() / shape.numel()) as f64],
        )?;
        let sum = self.reduce_f64(ReduceOp::Sum, input, axes, keep_dims)?;
        self.binary_f64(BinaryOp::Divide, &sum, &divisor)
    }
    fn matmul_f64(&self, left: &GpuF64Tensor, right: &GpuF64Tensor) -> Result<GpuF64Tensor> {
        self.check_f64(left)?;
        self.check_f64(right)?;
        let plan = MatmulPlan::new(left.shape(), right.shape())?;
        let rank = plan.matrix_output.rank();
        let dims = plan.matrix_output.dims();
        let rows = dims[rank - 2];
        let cols = dims[rank - 1];
        let inner = *plan.left.dims().last().unwrap();
        if plan.output.is_empty() || inner == 0 {
            return self.zero_f64(plan.output);
        }
        let a = self.reshape_f64(left, plan.left)?;
        let b = self.reshape_f64(right, plan.right)?;
        let mut ashape = dims[..rank - 2].to_vec();
        ashape.extend([rows, inner]);
        let mut bshape = dims[..rank - 2].to_vec();
        bshape.extend([inner, cols]);
        let a = self.broadcast_f64(&a, Shape::new(ashape)?)?;
        let b = self.broadcast_f64(&b, Shape::new(bshape)?)?;
        let result = self.dispatch_f64(3, 0, [&a, &b], plan.matrix_output, [inner, rows, cols])?;
        result.view(result.layout.reshape(plan.output)?)
    }
    fn evaluate_f64(&self, tensors: &[&GpuF64Tensor]) -> Result<()> {
        for tensor in tensors {
            self.check_f64(tensor)?;
        }
        self.evaluate(&[], &[])
    }
}
