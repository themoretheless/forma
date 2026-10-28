use super::{GpuLowTensor, GpuTensor, Result, checked_index};
use crate::ComputeProgram;
use tensor_core::ScanOptions;
impl ComputeProgram<'_> {
    /// Axis prefixes with direct packed loads and f32 accumulation. Intermediate
    /// prefixes must remain finite; parallel addition order and underflow apply.
    pub fn tensor_scan_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<GpuTensor> {
        self.low_check(input)?;
        input.shape().validate_axes(&[axis])?;
        let output = self.index_new(input.shape().clone())?;
        self.tensor_scan_low_f32_into(input, axis, options, &output)?;
        Ok(output)
    }
    pub fn tensor_scan_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axis: usize,
        options: ScanOptions,
        output: &GpuTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        input.shape().validate_axes(&[axis])?;
        self.index_output(output, input.shape(), &[input.packed_words().buffer()])?;
        if input.shape().numel() == 0 {
            return Ok(());
        }
        let mut prepared = self.runtime.program();
        let mut permutation: Vec<_> = (0..input.shape().rank()).filter(|&i| i != axis).collect();
        permutation.push(axis);
        let mut inverse = vec![0; permutation.len()];
        for (i, &axis) in permutation.iter().enumerate() {
            inverse[axis] = i;
        }
        let view = input.permute(&permutation)?;
        let target = output.permute(&permutation)?;
        let direct = target.layout().is_contiguous() && target.layout().offset() == 0;
        let scanned = if direct {
            GpuTensor::from_array(
                target.values().prefix(target.shape().numel())?,
                target.shape().clone(),
            )?
        } else {
            prepared.index_new(view.shape().clone())?
        };
        let mut load_meta = vec![
            checked_index(view.shape().rank())?,
            checked_index(view.layout().offset())?,
            input.dtype() as u32,
        ];
        for (&dim, &stride) in view.shape().dims().iter().zip(view.layout().strides()) {
            load_meta.extend([checked_index(dim)?, checked_index(stride)?]);
        }
        let length = checked_index(input.shape().dims()[axis])?;
        let rows = checked_index(input.shape().numel() / length as usize)?;
        prepared.scan_rows_loaded(
            input.packed_words().buffer(),
            &scanned,
            rows,
            length,
            options,
            &self.runtime.low_kernels()?.scan,
            &load_meta,
        )?;
        if !direct {
            prepared.copy_typed_into(&scanned.permute(&inverse)?, output)?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
    /// Each completed f32 prefix is rounded once to the original low dtype.
    pub fn tensor_scan_low(
        &mut self,
        input: &GpuLowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        input.shape().validate_axes(&[axis])?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), input.shape().clone())?;
        self.tensor_scan_low_into(input, axis, options, &output)?;
        Ok(output)
    }
    pub fn tensor_scan_low_into(
        &mut self,
        input: &GpuLowTensor,
        axis: usize,
        options: ScanOptions,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        input.shape().validate_axes(&[axis])?;
        self.low_output(
            output,
            input.shape(),
            input.dtype(),
            &[input.packed_words().buffer()],
        )?;
        let mut prepared = self.runtime.program();
        let result = prepared.tensor_scan_low_f32(input, axis, options)?;
        prepared.tensor_cast_to_low_into(&result, output)?;
        self.batch.append(prepared.batch);
        Ok(())
    }
}
