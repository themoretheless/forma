use super::{low_kernels, *};
use tensor_core::{LowDtype, MatmulPlan};

const TILE: usize = 16;
const SOURCE: &str = include_str!("../metal/low_matmul.metal");
const VECTOR_SOURCE: &str = include_str!("../metal/low_matvec.metal");
const HEADER: &str = concat!(
    "#include <metal_simdgroup_matrix>\n",
    include_str!("../metal/low_common.h")
);

impl MlxBackend {
    /// Entry validation has already checked ownership and input dtype. Keep
    /// existing shape/stride handles when promotion/broadcast changes nothing.
    fn low_matmul_view(&self, input: &MlxTensor, shape: Shape) -> Result<MlxTensor, MlxError> {
        Self::dimensions(&shape)?;
        if input.shape == shape {
            Ok(input.clone())
        } else {
            self.broadcast_to(input, shape)
        }
    }

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
        let a = self.low_matmul_view(&left.tensor, Shape::new(dims)?)?;
        let right = if right.shape().rank() == 1 {
            let row = self.low_matmul_view(&right.tensor, Shape::new(vec![1, k])?)?;
            self.permute(&row, &[1, 0])?
        } else {
            right.tensor.clone()
        };
        let mut dims = batch.to_vec();
        dims.extend([k, n]);
        let b = self.low_matmul_view(&right, Shape::new(dims)?)?;
        let tiles_m = m.div_ceil(TILE);
        let tiles_n = n.div_ceil(TILE);
        let batches = plan.output.numel() / m / n;
        let vector = m == 1 || n == 1;
        let work = if vector {
            plan.output.numel()
        } else {
            batches
                .checked_mul(tiles_m)
                .and_then(|x| x.checked_mul(tiles_n))
                .ok_or(MlxError::TooLarge)?
        };
        let mut words = [m, n, k, tiles_m, tiles_n]
            .into_iter()
            .map(|x| u32::try_from(x).map_err(|_| MlxError::TooLarge))
            .collect::<Result<Vec<_>, _>>()?;
        words.extend([work as u32, (work as u64 >> 32) as u32]);
        let params = self.upload_u32(Shape::new(vec![words.len()])?, &words)?;
        let threads = if vector { 256 } else { 128 };
        let mut kernel = low_kernels::key(
            if vector { VECTOR_SOURCE } else { SOURCE }.to_owned(),
            &["left", "right", "params"],
        );
        if !vector {
            kernel.header = HEADER;
        }
        self.custom_metal(
            kernel,
            &[&a, &b, &params],
            plan.output,
            MlxDtype::F32,
            left.dtype == LowDtype::Bf16,
            (work.min(65535) * threads, threads),
        )
    }
}
