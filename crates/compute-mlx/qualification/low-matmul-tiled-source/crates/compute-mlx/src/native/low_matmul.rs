use super::{low_kernels, *};
use tensor_core::{LowDtype, MatmulPlan};

const TILE: usize = 16;
const SOURCE: &str = include_str!("../metal/low_matmul.metal");

impl MlxBackend {
    pub(super) fn low_matmul_f32(
        &self,
        left: &MlxLowTensor,
        right: &MlxLowTensor,
        plan: MatmulPlan,
    ) -> Result<MlxTensor, MlxError> {
        Self::dimensions(&plan.output)?;
        let rank = plan.matrix_output.rank();
        let batch = &plan.matrix_output.dims()[..rank - 2];
        let m = plan.left.dims()[plan.left.rank() - 2];
        let k = plan.left.dims()[plan.left.rank() - 1];
        let n = plan.right.dims()[plan.right.rank() - 1];
        if plan.output.is_empty() || k == 0 {
            return self.zeros(plan.output, MlxDtype::F32);
        }
        // Broadcast inserts a left vector's leading singleton automatically.
        // A right vector needs a trailing singleton: broadcast to [1,K], then
        // transpose. Both transformations are native views, with no full copy.
        let mut dims = batch.to_vec();
        dims.extend([m, k]);
        let a = self.broadcast_to(&left.tensor, Shape::new(dims)?)?;
        let right = if right.shape().rank() == 1 {
            let row = self.broadcast_to(&right.tensor, Shape::new(vec![1, k])?)?;
            self.permute(&row, &[1, 0])?
        } else {
            right.tensor.clone()
        };
        let mut dims = batch.to_vec();
        dims.extend([k, n]);
        let b = self.broadcast_to(&right, Shape::new(dims)?)?;
        let tiles_m = m.div_ceil(TILE);
        let tiles_n = n.div_ceil(TILE);
        let batches = plan.output.numel() / m / n;
        let work = batches
            .checked_mul(tiles_m)
            .and_then(|x| x.checked_mul(tiles_n))
            .ok_or(MlxError::TooLarge)?;
        let mut words = [m, n, k, tiles_m, tiles_n]
            .into_iter()
            .map(|x| u32::try_from(x).map_err(|_| MlxError::TooLarge))
            .collect::<Result<Vec<_>, _>>()?;
        words.extend([work as u32, (work as u64 >> 32) as u32]);
        let params = self.upload_u32(Shape::new(vec![words.len()])?, &words)?;
        self.custom_metal(
            low_kernels::key(SOURCE.to_owned(), &["left", "right", "params"]),
            &[&a, &b, &params],
            plan.output,
            MlxDtype::F32,
            left.dtype == LowDtype::Bf16,
            (work.min(65535) * TILE * TILE, TILE * TILE),
        )
    }
}
