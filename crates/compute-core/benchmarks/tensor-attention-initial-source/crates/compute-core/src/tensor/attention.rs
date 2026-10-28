//! Streaming scaled dot-product attention without a score matrix allocation.
use super::{GpuTensor, TensorComputeError, checked_index};
use crate::{Binding, ComputeProgram, ComputeRuntime, Kernel, KernelError};
use tensor_core::{AttentionMask, AttentionOptions, AttentionPlan, Layout, Shape};

type Result<T> = std::result::Result<T, TensorComputeError>;

impl ComputeRuntime {
    fn tensor_attention_kernel(&self) -> std::result::Result<&Kernel, KernelError> {
        if let Some(kernel) = self.tensor_attention.get() {
            return Ok(kernel);
        }
        let kernel = Kernel::new(
            self.device(),
            "streaming tensor attention",
            crate::shaders::TENSOR_ATTENTION_WGSL,
            "main",
            &[
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageReadWrite,
            ],
        )?;
        let _ = self.tensor_attention.set(kernel);
        Ok(self
            .tensor_attention
            .get()
            .expect("attention kernel initialized"))
    }
}

impl ComputeProgram<'_> {
    /// Fused scaled QK, masked online softmax and PV. Key tiles stay in
    /// workgroup memory; no score/probability tensor or expanded operand is
    /// allocated. Each 64-channel value tile recomputes QK for its query.
    ///
    /// Inputs and allowed score arithmetic must be finite. An additive -Inf
    /// mask excludes a key before arithmetic. Fully masked and zero-key rows
    /// produce zero. Ordinary f32 reduction/underflow limits apply.
    pub fn tensor_attention(
        &mut self,
        query: &GpuTensor,
        key: &GpuTensor,
        value: &GpuTensor,
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
    ) -> Result<GpuTensor> {
        self.index_check(query)?;
        self.index_check(key)?;
        self.index_check(value)?;
        let plan = AttentionPlan::new(
            query.shape(),
            key.shape(),
            value.shape(),
            mask.shape(),
            options,
        )?;
        let output = self.index_new(plan.output)?;
        self.tensor_attention_into(query, key, value, mask, options, &output)?;
        Ok(output)
    }

    /// Records into a distinct contiguous output, preserving storage outside
    /// its logical offset range. Validation and allocation precede recording.
    pub fn tensor_attention_into(
        &mut self,
        query: &GpuTensor,
        key: &GpuTensor,
        value: &GpuTensor,
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
        output: &GpuTensor,
    ) -> Result<()> {
        self.index_check(query)?;
        self.index_check(key)?;
        self.index_check(value)?;
        let plan = AttentionPlan::new(
            query.shape(),
            key.shape(),
            value.shape(),
            mask.shape(),
            options,
        )?;
        let mut inputs = vec![
            query.values().buffer(),
            key.values().buffer(),
            value.values().buffer(),
        ];
        let (mask_layout, mask_mode) = match &mask {
            AttentionMask::None => (None, 0),
            AttentionMask::Keep(tensor) => {
                self.index_check(tensor)?;
                inputs.push(tensor.values().buffer());
                (Some(tensor.layout().broadcast_to(plan.scores.clone())?), 1)
            }
            AttentionMask::Additive(tensor) => {
                self.index_check(tensor)?;
                inputs.push(tensor.values().buffer());
                (Some(tensor.layout().broadcast_to(plan.scores.clone())?), 2)
            }
        };
        self.index_output(output, &plan.output, &inputs)?;
        if plan.output.numel() == 0 {
            return Ok(());
        }
        let q = operand_layout(query.layout(), plan.query.clone(), &plan.batch)?;
        let k = operand_layout(key.layout(), plan.key.clone(), &plan.batch)?;
        let v = operand_layout(value.layout(), plan.value.clone(), &plan.batch)?;
        let batch_rank = plan.batch.rank();
        let tiles_v = plan.value_depth.div_ceil(64);
        let rows = plan.output.numel() / plan.value_depth;
        let work_count = checked_index(
            rows.checked_mul(tiles_v)
                .ok_or(TensorComputeError::IndexTooLarge)?,
        )?;
        let groups = self.index_groups(work_count);
        let mut meta = vec![
            work_count,
            groups,
            checked_index(batch_rank)?,
            checked_index(plan.query_heads)?,
            checked_index(plan.queries)?,
            checked_index(plan.keys)?,
            checked_index(plan.depth)?,
            checked_index(plan.value_depth)?,
            checked_index(plan.group_size)?,
            checked_index(tiles_v)?,
            checked_index(q.offset())?,
            checked_index(k.offset())?,
            checked_index(v.offset())?,
            checked_index(mask_layout.as_ref().map_or(0, Layout::offset))?,
            checked_index(output.layout().offset())?,
        ];
        for layout in [&q, &k, &v] {
            for &stride in &layout.strides()[batch_rank..] {
                meta.push(checked_index(stride)?);
            }
        }
        for axis in batch_rank..batch_rank + 3 {
            meta.push(checked_index(
                mask_layout.as_ref().map_or(0, |m| m.strides()[axis]),
            )?);
        }
        meta.extend([
            mask_mode,
            plan.scale.to_bits(),
            u32::from(plan.causal.is_some()),
            plan.causal.unwrap_or(0) as u32,
        ]);
        for axis in 0..batch_rank {
            meta.extend([
                checked_index(plan.batch.dims()[axis])?,
                checked_index(q.strides()[axis])?,
                checked_index(k.strides()[axis])?,
                checked_index(v.strides()[axis])?,
                checked_index(mask_layout.as_ref().map_or(0, |m| m.strides()[axis]))?,
            ]);
        }
        let unused_mask = self.runtime.zeros::<u32>(1)?;
        let mask_buffer = match mask {
            AttentionMask::None => unused_mask.buffer(),
            AttentionMask::Keep(tensor) => tensor.values().buffer(),
            AttentionMask::Additive(tensor) => tensor.values().buffer(),
        };
        let kernel = self.runtime.tensor_attention_kernel()?;
        let mut prepared = self.runtime.program();
        prepared.index_dispatch(
            kernel,
            &meta,
            &[
                query.values().buffer(),
                key.values().buffer(),
                value.values().buffer(),
                mask_buffer,
                output.values().buffer(),
            ],
            groups,
        )?;
        self.batch.append(prepared.batch);
        Ok(())
    }
}

fn operand_layout(input: &Layout, promoted: Shape, batch: &Shape) -> Result<Layout> {
    let layout = if input.shape().rank() == 2 {
        Layout::new(
            promoted,
            vec![0, input.strides()[0], input.strides()[1]],
            input.offset(),
        )?
    } else {
        input.clone()
    };
    let rank = layout.shape().rank();
    let mut dims = batch.dims().to_vec();
    dims.extend(&layout.shape().dims()[rank - 3..]);
    Ok(layout.broadcast_to(Shape::new(dims)?)?)
}
