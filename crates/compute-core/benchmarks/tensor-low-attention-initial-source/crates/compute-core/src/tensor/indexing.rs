mod gather_compact;
mod scan;
pub(super) mod scatter;
use super::{GpuTensor, TensorComputeError, checked_index, index_kernels::kind};
use crate::{CompareOp, ComputeError, ComputeProgram, GpuElement, Kernel, wgpu};
use tensor_core::{Layout, Shape, select_shape};
type Result<T> = std::result::Result<T, TensorComputeError>;

impl<'a> ComputeProgram<'a> {
    pub(super) fn index_check<T: GpuElement>(&self, tensor: &GpuTensor<T>) -> Result<()> {
        Ok(self.runtime.check(tensor.values())?)
    }
    pub(super) fn index_new<T: GpuElement>(&self, shape: Shape) -> Result<GpuTensor<T>> {
        GpuTensor::from_array(self.runtime.zeros(shape.numel())?, shape)
    }
    pub(super) fn index_output<T: GpuElement>(
        &self,
        output: &GpuTensor<T>,
        shape: &Shape,
        inputs: &[&wgpu::Buffer],
    ) -> Result<()> {
        self.index_check(output)?;
        if output.shape() != shape {
            return Err(TensorComputeError::ShapeMismatch {
                expected: shape.dims().to_vec(),
                actual: output.shape().dims().to_vec(),
            });
        }
        if !output.layout().is_contiguous() {
            return Err(TensorComputeError::OutputNotContiguous);
        }
        if inputs.contains(&output.values().buffer()) {
            return Err(ComputeError::AliasedOutput.into());
        }
        Ok(())
    }
    pub(super) fn index_groups(&self, count: u32) -> u32 {
        count.min(65535).min(
            self.runtime
                .device()
                .limits()
                .max_compute_workgroups_per_dimension,
        )
    }
    pub(super) fn index_dispatch(
        &mut self,
        kernel: &'a Kernel,
        metadata: &[u32],
        buffers: &[&wgpu::Buffer],
        groups: u32,
    ) -> Result<()> {
        if groups == 0 {
            return Ok(());
        }
        let params = self.runtime.upload(metadata)?;
        let mut all = vec![params.buffer()];
        all.extend_from_slice(buffers);
        let bindings = kernel.create_bind_group(self.runtime.device(), &all);
        self.batch.push(kernel, &bindings, groups);
        Ok(())
    }
    // Callers validate ownership, scalar shape and aliases before preparing.
    // Count each logical index once even if every corresponding slice is empty.
    pub(super) fn index_count_into(
        &mut self,
        indices: &GpuTensor<u32>,
        axis_length: u32,
        invalid_count: &GpuTensor<u32>,
    ) -> Result<()> {
        let count_offset = checked_index(invalid_count.layout().offset())?;
        let kernels = self.runtime.tensor_index_kernels()?;
        self.index_dispatch(
            &kernels.count_reset,
            &[count_offset],
            &[invalid_count.values().buffer()],
            1,
        )?;
        let index_count = checked_index(indices.shape().numel())?;
        if index_count > 0 {
            let groups = self.index_groups(index_count.div_ceil(256));
            let mut meta = vec![
                index_count,
                checked_index(indices.shape().rank())?,
                groups,
                checked_index(indices.layout().offset())?,
                axis_length,
                count_offset,
            ];
            for (&dim, &stride) in indices
                .shape()
                .dims()
                .iter()
                .zip(indices.layout().strides())
            {
                meta.extend([checked_index(dim)?, checked_index(stride)?]);
            }
            self.index_dispatch(
                &kernels.index_count,
                &meta,
                &[indices.values().buffer(), invalid_count.values().buffer()],
                groups,
            )?;
        }
        Ok(())
    }
    pub(super) fn copy_typed_into<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        output: &GpuTensor<T>,
    ) -> Result<()> {
        self.index_check(input)?;
        self.index_output(output, input.shape(), &[input.values().buffer()])?;
        let count = checked_index(input.shape().numel())?;
        if count == 0 {
            return Ok(());
        }
        let groups = self.index_groups(count.div_ceil(256));
        let mut metadata = vec![
            count,
            checked_index(input.shape().rank())?,
            groups,
            checked_index(input.layout().offset())?,
            checked_index(output.layout().offset())?,
        ];
        for (&dim, &stride) in input.shape().dims().iter().zip(input.layout().strides()) {
            metadata.extend([checked_index(dim)?, checked_index(stride)?]);
        }
        let kernel = &self.runtime.tensor_index_kernels()?.copy[kind::<T>()];
        self.index_dispatch(
            kernel,
            &metadata,
            &[input.values().buffer(), output.values().buffer()],
            groups,
        )
    }
    pub(super) fn dense_typed<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
    ) -> Result<GpuTensor<T>> {
        self.index_check(input)?;
        if input.layout().is_contiguous() && input.layout().offset() == 0 {
            return GpuTensor::from_array(
                input.values().prefix(input.shape().numel())?,
                input.shape().clone(),
            );
        }
        let output = self.index_new(input.shape().clone())?;
        self.copy_typed_into(input, &output)?;
        Ok(output)
    }
    pub fn tensor_materialize_u32(&mut self, input: &GpuTensor<u32>) -> Result<GpuTensor<u32>> {
        self.index_check(input)?;
        let output = self.index_new(input.shape().clone())?;
        self.tensor_materialize_u32_into(input, &output)?;
        Ok(output)
    }
    pub fn tensor_materialize_u32_into(
        &mut self,
        input: &GpuTensor<u32>,
        output: &GpuTensor<u32>,
    ) -> Result<()> {
        self.copy_typed_into(input, output)
    }
    /// Broadcast comparisons produce exact zero/one masks for f32 or u32 input.
    pub fn tensor_compare<T: GpuElement>(
        &mut self,
        op: CompareOp,
        a: &GpuTensor<T>,
        b: &GpuTensor<T>,
    ) -> Result<GpuTensor<u32>> {
        self.index_check(a)?;
        self.index_check(b)?;
        let output = self.index_new(a.shape().broadcast(b.shape())?)?;
        self.tensor_compare_into(op, a, b, &output)?;
        Ok(output)
    }
    pub fn tensor_compare_into<T: GpuElement>(
        &mut self,
        op: CompareOp,
        a: &GpuTensor<T>,
        b: &GpuTensor<T>,
        output: &GpuTensor<u32>,
    ) -> Result<()> {
        self.index_check(a)?;
        self.index_check(b)?;
        let shape = a.shape().broadcast(b.shape())?;
        self.index_output(output, &shape, &[a.values().buffer(), b.values().buffer()])?;
        let av = a.layout().broadcast_to(shape.clone())?;
        let bv = b.layout().broadcast_to(shape.clone())?;
        let count = checked_index(shape.numel())?;
        if count == 0 {
            return Ok(());
        }
        let groups = self.index_groups(count.div_ceil(256));
        let metadata = compare_metadata(&av, &bv, output.layout(), op, groups)?;
        let kernel = &self.runtime.tensor_index_kernels()?.compare[kind::<T>()];
        self.index_dispatch(
            kernel,
            &metadata,
            &[
                a.values().buffer(),
                b.values().buffer(),
                output.values().buffer(),
            ],
            groups,
        )
    }
    /// Broadcast selection: zero mask chooses `on_false`, every other value `on_true`.
    pub fn tensor_select<T: GpuElement>(
        &mut self,
        mask: &GpuTensor<u32>,
        on_true: &GpuTensor<T>,
        on_false: &GpuTensor<T>,
    ) -> Result<GpuTensor<T>> {
        self.index_check(mask)?;
        self.index_check(on_true)?;
        self.index_check(on_false)?;
        let output = self.index_new(select_shape(
            mask.shape(),
            on_true.shape(),
            on_false.shape(),
        )?)?;
        self.tensor_select_into(mask, on_true, on_false, &output)?;
        Ok(output)
    }
    pub fn tensor_select_into<T: GpuElement>(
        &mut self,
        mask: &GpuTensor<u32>,
        a: &GpuTensor<T>,
        b: &GpuTensor<T>,
        output: &GpuTensor<T>,
    ) -> Result<()> {
        self.index_check(mask)?;
        self.index_check(a)?;
        self.index_check(b)?;
        let shape = select_shape(mask.shape(), a.shape(), b.shape())?;
        self.index_output(
            output,
            &shape,
            &[
                mask.values().buffer(),
                a.values().buffer(),
                b.values().buffer(),
            ],
        )?;
        let mv = mask.layout().broadcast_to(shape.clone())?;
        let av = a.layout().broadcast_to(shape.clone())?;
        let bv = b.layout().broadcast_to(shape.clone())?;
        let count = checked_index(shape.numel())?;
        if count == 0 {
            return Ok(());
        }
        let groups = self.index_groups(count.div_ceil(256));
        let metadata = select_metadata(&mv, &av, &bv, output.layout(), groups)?;
        let kernel = &self.runtime.tensor_index_kernels()?.select[kind::<T>()];
        self.index_dispatch(
            kernel,
            &metadata,
            &[
                mask.values().buffer(),
                a.values().buffer(),
                b.values().buffer(),
                output.values().buffer(),
            ],
            groups,
        )
    }
}

pub(super) fn compare_metadata(
    a: &Layout,
    b: &Layout,
    output: &Layout,
    op: CompareOp,
    groups: u32,
) -> Result<Vec<u32>> {
    let mut meta = vec![
        checked_index(output.shape().numel())?,
        checked_index(output.shape().rank())?,
        groups,
        op as u32,
        checked_index(a.offset())?,
        checked_index(b.offset())?,
        checked_index(output.offset())?,
    ];
    append_layouts(&mut meta, output.shape(), &[a, b])?;
    Ok(meta)
}
pub(super) fn select_metadata(
    mask: &Layout,
    a: &Layout,
    b: &Layout,
    output: &Layout,
    groups: u32,
) -> Result<Vec<u32>> {
    let mut meta = vec![
        checked_index(output.shape().numel())?,
        checked_index(output.shape().rank())?,
        groups,
        checked_index(mask.offset())?,
        checked_index(a.offset())?,
        checked_index(b.offset())?,
        checked_index(output.offset())?,
    ];
    append_layouts(&mut meta, output.shape(), &[mask, a, b])?;
    Ok(meta)
}
fn append_layouts(meta: &mut Vec<u32>, shape: &Shape, layouts: &[&Layout]) -> Result<()> {
    for (axis, &dim) in shape.dims().iter().enumerate() {
        meta.push(checked_index(dim)?);
        for layout in layouts {
            meta.push(checked_index(layout.strides()[axis])?);
        }
    }
    Ok(())
}
pub(super) fn gather_metadata(
    input: &Layout,
    indices: &Layout,
    output: &Layout,
    axis: usize,
    groups: u32,
) -> Result<Vec<u32>> {
    let mut meta = vec![
        checked_index(output.shape().numel())?,
        checked_index(output.shape().rank())?,
        groups,
        checked_index(input.offset())?,
        checked_index(indices.offset())?,
        checked_index(output.offset())?,
        checked_index(input.shape().dims()[axis])?,
        checked_index(input.strides()[axis])?,
    ];
    for original in 0..axis {
        meta.extend([
            checked_index(input.shape().dims()[original])?,
            checked_index(input.strides()[original])?,
            0,
        ]);
    }
    for (&dim, &stride) in indices.shape().dims().iter().zip(indices.strides()) {
        meta.extend([checked_index(dim)?, 0, checked_index(stride)?]);
    }
    for original in axis + 1..input.shape().rank() {
        meta.extend([
            checked_index(input.shape().dims()[original])?,
            checked_index(input.strides()[original])?,
            0,
        ]);
    }
    Ok(meta)
}
