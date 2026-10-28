use super::{GpuTensor, TensorComputeError, checked_index};
use crate::{BinaryOp, ComputeError, ComputeProgram, UnaryOp};
use tensor_core::{Layout, Shape, matmul_shape};

type Result<T> = std::result::Result<T, TensorComputeError>;
impl ComputeProgram<'_> {
    fn tensor_output(&self, shape: Shape) -> Result<GpuTensor> {
        GpuTensor::from_array(self.runtime.zeros(shape.numel())?, shape)
    }
    fn check_tensor(&self, tensor: &GpuTensor) -> Result<()> {
        Ok(self.runtime.check(tensor.values())?)
    }
    fn check_tensor_output(
        &self,
        output: &GpuTensor,
        shape: &Shape,
        inputs: &[&GpuTensor],
    ) -> Result<()> {
        self.check_tensor(output)?;
        if output.shape() != shape {
            return Err(TensorComputeError::ShapeMismatch {
                expected: shape.dims().to_vec(),
                actual: output.shape().dims().to_vec(),
            });
        }
        if !output.layout().is_contiguous() {
            return Err(TensorComputeError::OutputNotContiguous);
        }
        for input in inputs {
            self.check_tensor(input)?;
            if input.values().aliases(output.values()) {
                return Err(ComputeError::AliasedOutput.into());
            }
        }
        Ok(())
    }
    fn tensor_groups(&self, logical_groups: u32) -> u32 {
        logical_groups.min(65535).min(
            self.runtime
                .device()
                .limits()
                .max_compute_workgroups_per_dimension,
        )
    }
    /// Copies a logical strided view into row-major GPU storage.
    pub fn tensor_materialize(&mut self, input: &GpuTensor) -> Result<GpuTensor> {
        self.check_tensor(input)?;
        let output = self.tensor_output(input.shape().clone())?;
        self.tensor_materialize_into(input, &output)?;
        Ok(output)
    }
    pub fn tensor_materialize_into(&mut self, input: &GpuTensor, output: &GpuTensor) -> Result<()> {
        self.tensor_elementwise(0, input, input, input.layout(), input.layout(), output)
    }
    /// Elementwise f32 math over arbitrary read strides. Function domains and
    /// finite-intermediate requirements are the same as array unary operations.
    pub fn tensor_unary(&mut self, op: UnaryOp, input: &GpuTensor) -> Result<GpuTensor> {
        self.check_tensor(input)?;
        let output = self.tensor_output(input.shape().clone())?;
        self.tensor_unary_into(op, input, &output)?;
        Ok(output)
    }
    pub fn tensor_unary_into(
        &mut self,
        op: UnaryOp,
        input: &GpuTensor,
        output: &GpuTensor,
    ) -> Result<()> {
        self.tensor_elementwise(
            1 + op as u32,
            input,
            input,
            input.layout(),
            input.layout(),
            output,
        )
    }
    /// Uses standard trailing-axis broadcasting, including scalar tensors.
    pub fn tensor_binary(
        &mut self,
        op: BinaryOp,
        a: &GpuTensor,
        b: &GpuTensor,
    ) -> Result<GpuTensor> {
        self.check_tensor(a)?;
        self.check_tensor(b)?;
        let shape = a.shape().broadcast(b.shape())?;
        let output = self.tensor_output(shape)?;
        self.tensor_binary_into(op, a, b, &output)?;
        Ok(output)
    }
    pub fn tensor_binary_into(
        &mut self,
        op: BinaryOp,
        a: &GpuTensor,
        b: &GpuTensor,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = a.shape().broadcast(b.shape())?;
        let av = a.layout().broadcast_to(shape.clone())?;
        let bv = b.layout().broadcast_to(shape)?;
        self.tensor_elementwise(32 + op as u32, a, b, &av, &bv, output)
    }
    fn tensor_elementwise(
        &mut self,
        operation: u32,
        a: &GpuTensor,
        b: &GpuTensor,
        av: &Layout,
        bv: &Layout,
        output: &GpuTensor,
    ) -> Result<()> {
        self.check_tensor_output(output, av.shape(), &[a, b])?;
        let count = checked_index(av.shape().numel())?;
        if count == 0 {
            return Ok(());
        }
        let groups = self.tensor_groups(count.div_ceil(256));
        let mut meta = vec![
            count,
            checked_index(av.shape().rank())?,
            operation,
            groups,
            checked_index(av.offset())?,
            checked_index(bv.offset())?,
            checked_index(output.layout().offset())?,
            0,
        ];
        for axis in 0..av.shape().rank() {
            meta.extend([
                checked_index(av.shape().dims()[axis])?,
                checked_index(av.strides()[axis])?,
                checked_index(bv.strides()[axis])?,
            ]);
        }
        let metadata = self.runtime.upload(&meta)?;
        let kernel = &self.runtime.tensor_kernels()?.elementwise;
        let bindings = kernel.create_bind_group(
            self.runtime.device(),
            &[
                metadata.buffer(),
                a.values().buffer(),
                b.values().buffer(),
                output.values().buffer(),
            ],
        );
        self.batch.push(kernel, &bindings, groups);
        Ok(())
    }
    /// Sum over arbitrary axes; delegates to the shared typed reduction planner.
    pub fn tensor_sum(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor> {
        self.tensor_reduce(tensor_core::ReduceOp::Sum, input, axes, keep_dims)
    }
    pub fn tensor_sum_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
    ) -> Result<()> {
        self.tensor_reduce_into(tensor_core::ReduceOp::Sum, input, axes, keep_dims, output)
    }
    /// Tiled f32 matrix product with broadcast batch dimensions. Inputs may be
    /// transposed, sliced or broadcast views. Vectors are promoted to matrices
    /// for multiplication and the corresponding singleton axes are removed.
    /// Finite arithmetic is required; compare results with a tolerance.
    pub fn tensor_matmul(&mut self, a: &GpuTensor, b: &GpuTensor) -> Result<GpuTensor> {
        self.check_tensor(a)?;
        self.check_tensor(b)?;
        let shape = matmul_shape(a.shape(), b.shape())?;
        let output = self.tensor_output(shape)?;
        self.tensor_matmul_into(a, b, &output)?;
        Ok(output)
    }
    pub fn tensor_matmul_into(
        &mut self,
        a: &GpuTensor,
        b: &GpuTensor,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = matmul_shape(a.shape(), b.shape())?;
        self.check_tensor_output(output, &shape, &[a, b])?;
        let (meta, groups) = super::matmul::matmul_metadata(
            a.layout(),
            b.layout(),
            output.layout(),
            self.runtime
                .device()
                .limits()
                .max_compute_workgroups_per_dimension,
        )?;
        if groups == 0 {
            return Ok(());
        }
        let metadata = self.runtime.upload(&meta)?;
        let kernel = &self.runtime.tensor_kernels()?.matmul;
        let bindings = kernel.create_bind_group(
            self.runtime.device(),
            &[
                metadata.buffer(),
                a.values().buffer(),
                b.values().buffer(),
                output.values().buffer(),
            ],
        );
        self.batch.push(kernel, &bindings, groups);
        Ok(())
    }
}
