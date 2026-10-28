use super::{GpuTensor, Result, checked_index, kind};
use crate::{ComputeProgram, GpuElement, Kernel, wgpu};
use tensor_core::{ScanOptions, Shape};
impl<'a> ComputeProgram<'a> {
    /// Hierarchical scan along one axis. u32 addition wraps; f32 follows parallel
    /// addition order. Reverse changes traversal direction while preserving shape.
    pub fn tensor_scan<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        axis: usize,
        options: ScanOptions,
    ) -> Result<GpuTensor<T>> {
        self.index_check(input)?;
        input.shape().validate_axes(&[axis])?;
        let output = self.index_new(input.shape().clone())?;
        self.tensor_scan_into(input, axis, options, &output)?;
        Ok(output)
    }
    pub fn tensor_scan_into<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        axis: usize,
        options: ScanOptions,
        output: &GpuTensor<T>,
    ) -> Result<()> {
        self.index_check(input)?;
        input.shape().validate_axes(&[axis])?;
        self.index_output(output, input.shape(), &[input.values().buffer()])?;
        if input.shape().numel() == 0 {
            return Ok(());
        }
        let mut prepared = self.runtime.program();
        let mut permutation: Vec<usize> =
            (0..input.shape().rank()).filter(|&x| x != axis).collect();
        permutation.push(axis);
        let mut inverse = vec![0; permutation.len()];
        for (i, &axis) in permutation.iter().enumerate() {
            inverse[axis] = i;
        }
        let view = input.permute(&permutation)?;
        let dense = prepared.dense_typed(&view)?;
        let target = output.permute(&permutation)?;
        let direct = target.layout().is_contiguous() && target.layout().offset() == 0;
        let scanned = if direct {
            GpuTensor::from_array(
                target.values().prefix(target.shape().numel())?,
                target.shape().clone(),
            )?
        } else {
            prepared.index_new(dense.shape().clone())?
        };
        let length = checked_index(input.shape().dims()[axis])?;
        let rows = checked_index(input.shape().numel() / length as usize)?;
        prepared.scan_rows(&dense, &scanned, rows, length, options)?;
        if !direct {
            prepared.copy_typed_into(&scanned.permute(&inverse)?, output)?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
    /// Uses the existing four-values-per-lane flat primitive where its contract
    /// applies, while other axis/traversal combinations use the tensor hierarchy.
    pub fn tensor_scan_u32(
        &mut self,
        input: &GpuTensor<u32>,
        axis: usize,
        options: ScanOptions,
    ) -> Result<GpuTensor<u32>> {
        self.index_check(input)?;
        input.shape().validate_axes(&[axis])?;
        if !options.inclusive
            && !options.reverse
            && input.layout().is_contiguous()
            && input.layout().offset() == 0
            && input.shape().numel() == input.shape().dims()[axis]
        {
            let values = self.exclusive_scan(&input.values().prefix(input.shape().numel())?)?;
            return GpuTensor::from_array(values, input.shape().clone());
        }
        self.tensor_scan(input, axis, options)
    }
    pub(super) fn scan_rows<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        output: &GpuTensor<T>,
        rows: u32,
        length: u32,
        options: ScanOptions,
    ) -> Result<()> {
        let kernel = &self.runtime.tensor_index_kernels()?.scan_blocks[kind::<T>()];
        self.scan_rows_loaded(
            input.values().buffer(),
            output,
            rows,
            length,
            options,
            kernel,
            &[],
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::tensor) fn scan_rows_loaded<T: GpuElement>(
        &mut self,
        input: &wgpu::Buffer,
        output: &GpuTensor<T>,
        rows: u32,
        length: u32,
        options: ScanOptions,
        kernel: &'a Kernel,
        load_metadata: &[u32],
    ) -> Result<()> {
        let blocks = length.div_ceil(256);
        let total_count = rows * blocks;
        let totals = self.index_new::<T>(Shape::new(vec![rows as usize, blocks as usize])?)?;
        let groups = self.index_groups(total_count);
        let mut meta = vec![
            rows,
            length,
            blocks,
            groups,
            u32::from(options.inclusive),
            u32::from(options.reverse),
        ];
        meta.extend_from_slice(load_metadata);
        self.index_dispatch(
            kernel,
            &meta,
            &[input, output.values().buffer(), totals.values().buffer()],
            groups,
        )?;
        if blocks > 1 {
            let offsets = self.index_new(totals.shape().clone())?;
            self.scan_rows(
                &totals,
                &offsets,
                rows,
                blocks,
                ScanOptions {
                    inclusive: false,
                    reverse: false,
                },
            )?;
            let count = rows * length;
            let groups = self.index_groups(count.div_ceil(256));
            let add = &self.runtime.tensor_index_kernels()?.scan_add[kind::<T>()];
            self.index_dispatch(
                add,
                &[count, length, blocks, groups, u32::from(options.reverse)],
                &[offsets.values().buffer(), output.values().buffer()],
                groups,
            )?;
        }
        Ok(())
    }
}
