use super::*;
use tensor_core::{Compacted, Gathered, ScanOptions, TensorLowIndexBackend};

impl TensorLowIndexBackend for MlxBackend {
    fn compare_low(
        &self,
        op: CompareOp,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(left)?;
        self.check_low(right)?;
        self.compare_values(op, &left.tensor, &right.tensor)
    }

    fn select_low(
        &self,
        mask: &MlxTensor,
        on_true: &MlxLowTensor,
        on_false: &MlxLowTensor,
    ) -> Result<MlxLowTensor, MlxError> {
        self.check(mask, Some(MlxDtype::U32))?;
        self.check_low(on_true)?;
        self.check_low(on_false)?;
        let tensor = self.select_values(mask, &on_true.tensor, &on_false.tensor)?;
        Ok(MlxLowTensor {
            tensor,
            dtype: on_true.dtype,
        })
    }

    fn gather_low(
        &self,
        input: &MlxLowTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxLowTensor, MlxTensor>, MlxError> {
        self.check_low(input)?;
        let result = self.gather_values(&input.tensor, indices, axis)?;
        Ok(Gathered {
            values: MlxLowTensor {
                tensor: result.values,
                dtype: input.dtype,
            },
            invalid_count: result.invalid_count,
        })
    }

    fn compact_low(
        &self,
        input: &MlxLowTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxLowTensor, MlxTensor>, MlxError> {
        self.check_low(input)?;
        let result = self.compact_values(&input.tensor, mask)?;
        Ok(Compacted {
            values: MlxLowTensor {
                tensor: result.values,
                dtype: input.dtype,
            },
            count: result.count,
        })
    }

    fn scan_low_f32(
        &self,
        input: &MlxLowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check_low(input)?;
        lowering::scan::scan_low_f32(
            &mut lowering::NativeLowerer::new(self),
            input.tensor.clone(),
            axis,
            options,
        )
    }
}
