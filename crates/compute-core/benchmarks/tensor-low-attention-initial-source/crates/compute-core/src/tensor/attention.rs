//! Streaming scaled dot-product attention without a score matrix allocation.
use super::{GpuTensor, TensorComputeError, checked_index};
use crate::{Binding, ComputeProgram, ComputeRuntime, Kernel, KernelError};
use tensor_core::{AttentionMask, AttentionOptions, AttentionPlan, Layout, Shape};

type Result<T> = std::result::Result<T, TensorComputeError>;
pub(crate) struct AttentionKernels {
    stream: Kernel,
    merge: Kernel,
}

impl ComputeRuntime {
    fn tensor_attention_kernel(&self) -> std::result::Result<&AttentionKernels, KernelError> {
        if let Some(kernel) = self.tensor_attention.get() {
            return Ok(kernel);
        }
        let kernels = AttentionKernels::new(self.device(), crate::shaders::TENSOR_ATTENTION_WGSL)?;
        let _ = self.tensor_attention.set(kernels);
        Ok(self
            .tensor_attention
            .get()
            .expect("attention kernel initialized"))
    }
}

impl AttentionKernels {
    pub(in crate::tensor) fn new(
        device: &crate::wgpu::Device,
        stream_source: &str,
    ) -> std::result::Result<Self, KernelError> {
        let stream = Kernel::new(
            device,
            "streaming tensor attention",
            stream_source,
            "main",
            &[
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageReadWrite,
                Binding::StorageReadWrite,
            ],
        )?;
        let merge = Kernel::new(
            device,
            "split-key attention merge",
            crate::shaders::TENSOR_ATTENTION_MERGE_WGSL,
            "main",
            &[
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageRead,
                Binding::StorageReadWrite,
            ],
        )?;
        Ok(Self { stream, merge })
    }
}

impl<'a> ComputeProgram<'a> {
    /// Fused scaled QK, masked online softmax and PV. Key tiles stay in
    /// workgroup memory; no score/probability tensor or expanded operand is
    /// allocated. Each 64-channel value tile recomputes QK for its query.
    /// Few-query long-key problems split keys across workgroups and merge
    /// normalized partials, capped at 4 MiB of partial value storage.
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
        self.attention_plan_loaded(
            plan,
            [query.layout(), key.layout(), value.layout()],
            [
                query.values().buffer(),
                key.values().buffer(),
                value.values().buffer(),
            ],
            mask,
            output,
            self.runtime.tensor_attention_kernel()?,
            &[],
        )
    }

    /// Shared traversal; callers check operand runtime ownership first.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::tensor) fn attention_plan_loaded(
        &mut self,
        plan: AttentionPlan,
        layouts: [&Layout; 3],
        buffers: [&crate::wgpu::Buffer; 3],
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        output: &GpuTensor,
        kernels: &'a AttentionKernels,
        metadata_tail: &[u32],
    ) -> Result<()> {
        let mut inputs = vec![buffers[0], buffers[1], buffers[2]];
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
        let q = operand_layout(layouts[0], plan.query.clone(), &plan.batch)?;
        let k = operand_layout(layouts[1], plan.key.clone(), &plan.batch)?;
        let v = operand_layout(layouts[2], plan.value.clone(), &plan.batch)?;
        let batch_rank = plan.batch.rank();
        let tiles_v = plan.value_depth.div_ceil(64);
        let rows = plan.output.numel() / plan.value_depth;
        let limits = self.runtime.device().limits();
        let value_budget = (limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size)
            / 4)
        .min(1_048_576) as usize;
        let parts = key_parts(rows, plan.keys, plan.output.numel(), value_budget);
        let work_count = checked_index(
            rows.checked_mul(tiles_v)
                .and_then(|x| x.checked_mul(parts))
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
            if parts > 1 {
                0
            } else {
                checked_index(output.layout().offset())?
            },
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
            checked_index(parts)?,
            checked_index(plan.keys.div_ceil(parts))?,
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
        meta.extend(metadata_tail);
        let unused_mask = self.runtime.zeros::<u32>(1)?;
        let mask_buffer = match mask {
            AttentionMask::None => unused_mask.buffer(),
            AttentionMask::Keep(tensor) => tensor.values().buffer(),
            AttentionMask::Additive(tensor) => tensor.values().buffer(),
        };
        let partial_values = if parts > 1 {
            Some(self.runtime.zeros::<f32>(plan.output.numel() * parts)?)
        } else {
            None
        };
        let partial_state =
            self.runtime
                .zeros::<f32>(if parts > 1 { rows * parts * 2 } else { 2 })?;
        let mut prepared = self.runtime.program();
        prepared.index_dispatch(
            &kernels.stream,
            &meta,
            &[
                buffers[0],
                buffers[1],
                buffers[2],
                mask_buffer,
                partial_values.as_ref().unwrap_or(output.values()).buffer(),
                partial_state.buffer(),
            ],
            groups,
        )?;
        if let Some(partials) = partial_values {
            let merge_work = checked_index(rows * tiles_v)?;
            let merge_groups = self.index_groups(merge_work);
            prepared.index_dispatch(
                &kernels.merge,
                &[
                    checked_index(rows)?,
                    checked_index(parts)?,
                    checked_index(plan.value_depth)?,
                    checked_index(tiles_v)?,
                    merge_work,
                    merge_groups,
                    checked_index(output.layout().offset())?,
                ],
                &[
                    partial_state.buffer(),
                    partials.buffer(),
                    output.values().buffer(),
                ],
                merge_groups,
            )?;
        }
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

fn key_parts(rows: usize, keys: usize, output_elements: usize, value_budget: usize) -> usize {
    if rows <= 64 && keys >= 512 && output_elements > 0 {
        keys.div_ceil(256)
            .min(64)
            .min(value_budget / output_elements)
            .max(1)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::key_parts;
    #[test]
    fn split_key_schedule_bounds_workspace_and_respects_cutoffs() {
        let budget = 1_048_576;
        assert_eq!(key_parts(1, 511, 64, budget), 1);
        assert_eq!(key_parts(1, 512, 64, budget), 2);
        assert_eq!(key_parts(1, 513, 64, budget), 3);
        assert_eq!(key_parts(4, 4097, 4 * 64, budget), 17);
        assert_eq!(key_parts(65, 4097, 65 * 64, budget), 1);
        assert_eq!(key_parts(1, 131077, 129, budget), 64);
        assert_eq!(key_parts(1, 4097, 400_000, budget), 2);
        assert_eq!(key_parts(1, 4097, 600_000, budget), 1);
        for rows in [1, 4, 64, 65] {
            for width in [1, 64, 129, 65_537] {
                for keys in [0, 511, 512, 513, 4097, u32::MAX as usize] {
                    let output_elements = rows * width;
                    let parts = key_parts(rows, keys, output_elements, budget);
                    if parts > 1 {
                        assert!(parts <= 64 && rows <= 64);
                        assert!(output_elements * parts <= budget);
                        assert!(rows * parts * 2 * 4 <= 32_768);
                        assert!((parts - 1) * keys.div_ceil(parts) < keys);
                    }
                }
            }
        }
    }
}
