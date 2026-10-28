use super::{
    lowering::{NativeLowerer, ops},
    *,
};
use tensor_core::MatmulPlan;

impl MlxBackend {
    pub(super) fn low_matmul_f32(
        &self,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
        plan: MatmulPlan,
    ) -> Result<MlxTensor, MlxError> {
        ops::matmul_low_f32(
            &mut NativeLowerer::new(self),
            left.tensor.clone(),
            right.tensor.clone(),
            plan,
        )
    }
}
