use super::{GpuLowTensor, GpuTensor, Result, TensorComputeError, check_dtype, checked_index};
use crate::{ComputeError, ComputeProgram, wgpu};
use tensor_core::{Layout, LowDtype, Shape, matmul_shape};

impl ComputeProgram<'_> {
    pub(super) fn low_check(&self, input: &GpuLowTensor) -> Result<()> {
        Ok(self.runtime.check(input.packed_words())?)
    }
    pub(super) fn low_output(
        &self,
        output: &GpuLowTensor,
        shape: &Shape,
        dtype: LowDtype,
        inputs: &[&wgpu::Buffer],
    ) -> Result<()> {
        self.low_check(output)?;
        check_dtype(dtype, output.dtype())?;
        if output.shape() != shape {
            return Err(TensorComputeError::ShapeMismatch {
                expected: shape.dims().to_vec(),
                actual: output.shape().dims().to_vec(),
            });
        }
        if !output.layout().is_contiguous() {
            return Err(TensorComputeError::OutputNotContiguous);
        }
        if inputs.contains(&output.packed_words().buffer()) {
            return Err(ComputeError::AliasedOutput.into());
        }
        Ok(())
    }

    /// Bit-preserving GPU copy of an arbitrary low-precision logical view.
    pub fn tensor_materialize_low(&mut self, input: &GpuLowTensor) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), input.shape().clone())?;
        self.tensor_materialize_low_into(input, &output)?;
        Ok(output)
    }
    pub fn tensor_materialize_low_into(
        &mut self,
        input: &GpuLowTensor,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        self.low_output(
            output,
            input.shape(),
            input.dtype(),
            &[input.packed_words().buffer()],
        )?;
        self.low_pack(input.layout(), input.packed_words().buffer(), output, 0)
    }

    /// Rounds f32 IEEE bits on GPU, preserving signed zeros/subnormals and
    /// overflowing to infinity. NaN payloads may be canonicalized.
    pub fn tensor_cast_to_low(
        &mut self,
        input: &GpuTensor,
        dtype: LowDtype,
    ) -> Result<GpuLowTensor> {
        self.index_check(input)?;
        let output = self.runtime.zeros_low(dtype, input.shape().clone())?;
        self.tensor_cast_to_low_into(input, &output)?;
        Ok(output)
    }
    pub fn tensor_cast_to_low_into(
        &mut self,
        input: &GpuTensor,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.index_check(input)?;
        self.low_output(
            output,
            input.shape(),
            output.dtype(),
            &[input.values().buffer()],
        )?;
        self.low_pack(input.layout(), input.values().buffer(), output, 1)
    }
    fn low_pack(
        &mut self,
        input: &Layout,
        source: &wgpu::Buffer,
        output: &GpuLowTensor,
        mode: u32,
    ) -> Result<()> {
        let count = checked_index(input.shape().numel())?;
        if count == 0 {
            return Ok(());
        }
        let word_count =
            checked_index((input.shape().numel() + (output.layout().offset() & 1)).div_ceil(2))?;
        let groups = self.index_groups(word_count.div_ceil(256));
        let mut meta = vec![
            count,
            checked_index(input.shape().rank())?,
            groups,
            checked_index(input.offset())?,
            checked_index(output.layout().offset())?,
            output.dtype() as u32,
            mode,
            word_count,
        ];
        for (&dim, &stride) in input.shape().dims().iter().zip(input.strides()) {
            meta.extend([checked_index(dim)?, checked_index(stride)?]);
        }
        self.index_dispatch(
            &self.runtime.low_kernels()?.pack,
            &meta,
            &[source, output.packed_words().buffer()],
            groups,
        )
    }

    /// Decodes low-precision IEEE bits directly into f32 storage. The codec
    /// performs only integer arithmetic, including for f32 subnormal results.
    pub fn tensor_cast_to_f32(&mut self, input: &GpuLowTensor) -> Result<GpuTensor> {
        self.low_check(input)?;
        let output = self.index_new(input.shape().clone())?;
        self.tensor_cast_to_f32_into(input, &output)?;
        Ok(output)
    }
    pub fn tensor_cast_to_f32_into(
        &mut self,
        input: &GpuLowTensor,
        output: &GpuTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        self.index_output(output, input.shape(), &[input.packed_words().buffer()])?;
        let count = checked_index(input.shape().numel())?;
        if count == 0 {
            return Ok(());
        }
        let groups = self.index_groups(count.div_ceil(256));
        let mut meta = vec![
            count,
            checked_index(input.shape().rank())?,
            groups,
            checked_index(input.layout().offset())?,
            checked_index(output.layout().offset())?,
            input.dtype() as u32,
        ];
        for (&dim, &stride) in input.shape().dims().iter().zip(input.layout().strides()) {
            meta.extend([checked_index(dim)?, checked_index(stride)?]);
        }
        self.index_dispatch(
            &self.runtime.low_kernels()?.unpack,
            &meta,
            &[input.packed_words().buffer(), output.values().buffer()],
            groups,
        )
    }

    /// Tiled matrix/vector/batched multiplication that decodes packed operands
    /// during tile loads and accumulates into f32. Dtypes must match; no whole
    /// operand expansion is allocated. Finite arithmetic and tolerance apply.
    pub fn tensor_matmul_low_f32(
        &mut self,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
    ) -> Result<GpuTensor> {
        self.low_check(a)?;
        self.low_check(b)?;
        check_dtype(a.dtype(), b.dtype())?;
        let output = self.index_new(matmul_shape(a.shape(), b.shape())?)?;
        self.tensor_matmul_low_f32_into(a, b, &output)?;
        Ok(output)
    }
    pub fn tensor_matmul_low_f32_into(
        &mut self,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
        output: &GpuTensor,
    ) -> Result<()> {
        self.low_check(a)?;
        self.low_check(b)?;
        check_dtype(a.dtype(), b.dtype())?;
        self.index_output(
            output,
            &matmul_shape(a.shape(), b.shape())?,
            &[a.packed_words().buffer(), b.packed_words().buffer()],
        )?;
        let (mut meta, groups) = super::super::matmul::matmul_metadata(
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
        meta[15] = a.dtype() as u32;
        self.index_dispatch(
            &self.runtime.low_kernels()?.matmul,
            &meta,
            &[
                a.packed_words().buffer(),
                b.packed_words().buffer(),
                output.values().buffer(),
            ],
            groups,
        )
    }
    /// Low-precision result with one final rounding of the completed f32
    /// accumulation. Only the result uses an f32 intermediate allocation.
    pub fn tensor_matmul_low(
        &mut self,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
    ) -> Result<GpuLowTensor> {
        self.low_check(a)?;
        self.low_check(b)?;
        check_dtype(a.dtype(), b.dtype())?;
        let output = self
            .runtime
            .zeros_low(a.dtype(), matmul_shape(a.shape(), b.shape())?)?;
        self.tensor_matmul_low_into(a, b, &output)?;
        Ok(output)
    }
    pub fn tensor_matmul_low_into(
        &mut self,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(a)?;
        self.low_check(b)?;
        check_dtype(a.dtype(), b.dtype())?;
        self.low_output(
            output,
            &matmul_shape(a.shape(), b.shape())?,
            a.dtype(),
            &[a.packed_words().buffer(), b.packed_words().buffer()],
        )?;
        let mut prepared = self.runtime.program();
        let product = prepared.tensor_matmul_low_f32(a, b)?;
        prepared.tensor_cast_to_low_into(&product, output)?;
        self.batch.append(prepared.batch);
        Ok(())
    }
}
