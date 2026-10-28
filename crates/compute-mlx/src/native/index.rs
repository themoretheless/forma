use super::*;
use tensor_core::{Compacted, Gathered, ScanOptions, TensorIndexBackend};

impl MlxBackend {
    pub(super) fn zeros(&self, shape: Shape, dtype: MlxDtype) -> Result<MlxTensor, MlxError> {
        let dims = Self::dimensions(&shape)?;
        self.output("zeros", shape, dtype, |a, out| unsafe {
            (a.zeros)(
                out,
                dims.as_ptr(),
                dims.len(),
                dtype.raw(),
                self.context.stream,
            )
        })
    }

    pub(super) fn select_values(
        &self,
        mask: &MlxTensor,
        on_true: &MlxTensor,
        on_false: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        lowering::indexing::select(
            &mut lowering::NativeLowerer::new(self),
            mask.clone(),
            on_true.clone(),
            on_false.clone(),
        )
    }

    pub(super) fn gather_values(
        &self,
        input: &MlxTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxTensor, MlxTensor>, MlxError> {
        lowering::indexing::gather(
            &mut lowering::NativeLowerer::new(self),
            input.clone(),
            indices.clone(),
            axis,
        )
    }

    pub(super) fn compact_values(
        &self,
        input: &MlxTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxTensor, MlxTensor>, MlxError> {
        lowering::compaction::compact(
            &mut lowering::NativeLowerer::new(self),
            input.clone(),
            mask.clone(),
        )
    }
}

impl TensorIndexBackend for MlxBackend {
    type UIntTensor = MlxTensor;

    fn upload_u32(&self, shape: Shape, values: &[u32]) -> Result<MlxTensor, MlxError> {
        self.upload_u32(shape, values)
    }
    fn read_u32(&self, input: &MlxTensor) -> Result<Vec<u32>, MlxError> {
        self.read_u32(input)
    }
    fn materialize_u32(&self, input: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.materialize(input)
    }
    fn reshape_u32(&self, input: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.reshape(input, shape)
    }
    fn permute_u32(&self, input: &MlxTensor, axes: &[usize]) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.permute(input, axes)
    }
    fn broadcast_u32(&self, input: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.broadcast_to(input, shape)
    }
    fn compare(&self, op: CompareOp, a: &MlxTensor, b: &MlxTensor) -> Result<MlxTensor, MlxError> {
        self.compare(op, a, b)
    }
    fn compare_u32(
        &self,
        op: CompareOp,
        a: &MlxTensor,
        b: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(a, Some(MlxDtype::U32))?;
        self.compare_values(op, a, b)
    }
    fn select_f32(
        &self,
        mask: &MlxTensor,
        a: &MlxTensor,
        b: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(a, Some(MlxDtype::F32))?;
        self.select_values(mask, a, b)
    }
    fn select_u32(
        &self,
        mask: &MlxTensor,
        a: &MlxTensor,
        b: &MlxTensor,
    ) -> Result<MlxTensor, MlxError> {
        self.check(a, Some(MlxDtype::U32))?;
        self.select_values(mask, a, b)
    }
    fn scan_f32(
        &self,
        input: &MlxTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.scan(input, axis, options.inclusive, options.reverse)
    }
    fn scan_u32(
        &self,
        input: &MlxTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.scan(input, axis, options.inclusive, options.reverse)
    }
    fn gather_f32(
        &self,
        input: &MlxTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.gather_values(input, indices, axis)
    }
    fn gather_u32(
        &self,
        input: &MlxTensor,
        indices: &MlxTensor,
        axis: usize,
    ) -> Result<Gathered<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.gather_values(input, indices, axis)
    }
    fn compact_f32(
        &self,
        input: &MlxTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.compact_values(input, mask)
    }
    fn compact_u32(
        &self,
        input: &MlxTensor,
        mask: &MlxTensor,
    ) -> Result<Compacted<MlxTensor, MlxTensor>, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.compact_values(input, mask)
    }
}
