use super::super::indexing::{compare_metadata, gather_metadata, select_metadata};
use super::{GpuLowTensor, GpuTensor, Result, checked_index};
use crate::{CompareOp, ComputeProgram, scan::ScanPlan};
use tensor_core::{
    Compacted, Gathered, Shape, compact_shape, gather_shape, low_binary_shape, low_select_shape,
};
impl ComputeProgram<'_> {
    /// IEEE comparison over every low-format bit pattern. NaNs are unordered;
    /// either signed zero compares equal. Finite subnormals compare exactly.
    pub fn tensor_compare_low(
        &mut self,
        op: CompareOp,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
    ) -> Result<GpuTensor<u32>> {
        self.low_check(a)?;
        self.low_check(b)?;
        let output = self.index_new(low_binary_shape(a, b)?)?;
        self.tensor_compare_low_into(op, a, b, &output)?;
        Ok(output)
    }
    pub fn tensor_compare_low_into(
        &mut self,
        op: CompareOp,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
        output: &GpuTensor<u32>,
    ) -> Result<()> {
        self.low_check(a)?;
        self.low_check(b)?;
        let shape = low_binary_shape(a, b)?;
        self.index_output(
            output,
            &shape,
            &[a.packed_words().buffer(), b.packed_words().buffer()],
        )?;
        let count = checked_index(shape.numel())?;
        if count == 0 {
            return Ok(());
        }
        let groups = self.index_groups(count.div_ceil(256));
        let av = a.layout().broadcast_to(shape.clone())?;
        let bv = b.layout().broadcast_to(shape)?;
        let mut meta = compare_metadata(&av, &bv, output.layout(), op, groups)?;
        meta.push(a.dtype() as u32);
        self.index_dispatch(
            &self.runtime.low_kernels()?.compare,
            &meta,
            &[
                a.packed_words().buffer(),
                b.packed_words().buffer(),
                output.values().buffer(),
            ],
            groups,
        )
    }
    /// Raw payload selection. Nonzero masks choose `on_true`, preserving NaNs,
    /// infinities, signed zero and all low-format subnormal bits.
    pub fn tensor_select_low(
        &mut self,
        mask: &GpuTensor<u32>,
        on_true: &GpuLowTensor,
        on_false: &GpuLowTensor,
    ) -> Result<GpuLowTensor> {
        self.index_check(mask)?;
        self.low_check(on_true)?;
        self.low_check(on_false)?;
        let output = self.runtime.zeros_low(
            on_true.dtype(),
            low_select_shape(mask.shape(), on_true, on_false)?,
        )?;
        self.tensor_select_low_into(mask, on_true, on_false, &output)?;
        Ok(output)
    }
    pub fn tensor_select_low_into(
        &mut self,
        mask: &GpuTensor<u32>,
        a: &GpuLowTensor,
        b: &GpuLowTensor,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.index_check(mask)?;
        self.low_check(a)?;
        self.low_check(b)?;
        let shape = low_select_shape(mask.shape(), a, b)?;
        self.low_output(
            output,
            &shape,
            a.dtype(),
            &[
                mask.values().buffer(),
                a.packed_words().buffer(),
                b.packed_words().buffer(),
            ],
        )?;
        if shape.numel() == 0 {
            return Ok(());
        }
        let groups = self.low_word_groups(output)?;
        let mv = mask.layout().broadcast_to(shape.clone())?;
        let av = a.layout().broadcast_to(shape.clone())?;
        let bv = b.layout().broadcast_to(shape)?;
        let meta = select_metadata(&mv, &av, &bv, output.layout(), groups)?;
        self.index_dispatch(
            &self.runtime.low_kernels()?.select,
            &meta,
            &[
                mask.values().buffer(),
                a.packed_words().buffer(),
                b.packed_words().buffer(),
                output.packed_words().buffer(),
            ],
            groups,
        )
    }
    fn low_word_groups(&self, output: &GpuLowTensor) -> Result<u32> {
        let words = (output.shape().numel() + (output.layout().offset() & 1)).div_ceil(2);
        Ok(self.index_groups(checked_index(words)?.div_ceil(256)))
    }
    /// Raw gather; invalid indices yield positive zero. The resident scalar
    /// counts each logical index once, including when output slices are empty.
    pub fn tensor_gather_low(
        &mut self,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        axis: usize,
    ) -> Result<Gathered<GpuLowTensor, GpuTensor<u32>>> {
        self.low_check(input)?;
        self.index_check(indices)?;
        let values = self.runtime.zeros_low(
            input.dtype(),
            gather_shape(input.shape(), indices.shape(), axis)?,
        )?;
        let invalid_count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_gather_low_into(input, indices, axis, &values, &invalid_count)?;
        Ok(Gathered {
            values,
            invalid_count,
        })
    }
    pub fn tensor_gather_low_into(
        &mut self,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        axis: usize,
        output: &GpuLowTensor,
        invalid_count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.low_check(input)?;
        self.index_check(indices)?;
        let shape = gather_shape(input.shape(), indices.shape(), axis)?;
        self.low_output(
            output,
            &shape,
            input.dtype(),
            &[
                input.packed_words().buffer(),
                indices.values().buffer(),
                invalid_count.values().buffer(),
            ],
        )?;
        self.index_output(
            invalid_count,
            &Shape::new(vec![])?,
            &[
                input.packed_words().buffer(),
                indices.values().buffer(),
                output.packed_words().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        prepared.index_count_into(
            indices,
            checked_index(input.shape().dims()[axis])?,
            invalid_count,
        )?;
        if shape.numel() > 0 {
            let groups = self.low_word_groups(output)?;
            let meta = gather_metadata(
                input.layout(),
                indices.layout(),
                output.layout(),
                axis,
                groups,
            )?;
            prepared.index_dispatch(
                &self.runtime.low_kernels()?.gather,
                &meta,
                &[
                    input.packed_words().buffer(),
                    indices.values().buffer(),
                    output.packed_words().buffer(),
                ],
                groups,
            )?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
    /// Stable row-major compaction of raw payloads. Capacity equals numel;
    /// count and zeroed unused tail are regenerated on every program execution.
    pub fn tensor_compact_low(
        &mut self,
        input: &GpuLowTensor,
        mask: &GpuTensor<u32>,
    ) -> Result<Compacted<GpuLowTensor, GpuTensor<u32>>> {
        self.low_check(input)?;
        self.index_check(mask)?;
        let values = self
            .runtime
            .zeros_low(input.dtype(), compact_shape(input.shape(), mask.shape())?)?;
        let count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_compact_low_into(input, mask, &values, &count)?;
        Ok(Compacted { values, count })
    }
    pub fn tensor_compact_low_into(
        &mut self,
        input: &GpuLowTensor,
        mask: &GpuTensor<u32>,
        output: &GpuLowTensor,
        count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.low_check(input)?;
        self.index_check(mask)?;
        let shape = compact_shape(input.shape(), mask.shape())?;
        self.low_output(
            output,
            &shape,
            input.dtype(),
            &[
                input.packed_words().buffer(),
                mask.values().buffer(),
                count.values().buffer(),
            ],
        )?;
        self.index_output(
            count,
            &Shape::new(vec![])?,
            &[
                input.packed_words().buffer(),
                mask.values().buffer(),
                output.packed_words().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        let keep = prepared.dense_typed(&mask.broadcast_to(input.shape().clone())?)?;
        let local = self.runtime.zeros::<u32>(input.shape().numel())?;
        let (scan, offsets, block_size) = ScanPlan::local(self.runtime, keep.values(), &local)?;
        let length = checked_index(input.shape().numel())?;
        let groups = prepared.index_groups(length.div_ceil(256).max(1));
        let mut meta = vec![
            length,
            checked_index(input.shape().rank())?,
            groups,
            block_size,
            checked_index(input.layout().offset())?,
            checked_index(output.layout().offset())?,
            checked_index(count.layout().offset())?,
        ];
        for (&dim, &stride) in input.shape().dims().iter().zip(input.layout().strides()) {
            meta.extend([checked_index(dim)?, checked_index(stride)?]);
        }
        let kernel = &self.runtime.low_kernels()?.compact;
        scan.append_to(&mut prepared.batch);
        prepared.index_dispatch(
            kernel,
            &meta,
            &[
                input.packed_words().buffer(),
                keep.values().buffer(),
                local.buffer(),
                offsets.buffer(),
                output.packed_words().buffer(),
                count.values().buffer(),
            ],
            groups,
        )?;
        self.batch.append(prepared.batch);
        Ok(())
    }
}
