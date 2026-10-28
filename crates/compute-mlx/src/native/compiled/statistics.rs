use super::*;
use crate::native::lowering::{casts, statistics as wide, statistics_low as low};
use tensor_core::Moments;

impl MlxProgramBuilder {
    pub fn softmax(&mut self, value: MlxValue, axes: &[usize]) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| wide::softmax(graph, value, axes))
    }
    pub fn log_softmax(&mut self, value: MlxValue, axes: &[usize]) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| wide::log_softmax(graph, value, axes))
    }
    pub fn logsumexp(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| wide::logsumexp(graph, value, axes, keep_dims))
    }
    pub fn moments(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxValue>, MlxError> {
        self.transaction(|graph| wide::moments(graph, value, axes, keep_dims))
    }
    pub fn layer_norm(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| wide::layer_norm(graph, value, axes, epsilon))
    }
    pub fn softmax_low_f32(
        &mut self,
        value: MlxValue,
        axes: &[usize],
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| low::softmax(graph, value, axes, false))
    }
    pub fn log_softmax_low_f32(
        &mut self,
        value: MlxValue,
        axes: &[usize],
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| low::softmax(graph, value, axes, true))
    }
    pub fn logsumexp_low_f32(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| low::logsumexp(graph, value, axes, keep_dims))
    }
    pub fn moments_low_f32(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxValue>, MlxError> {
        self.transaction(|graph| low::moments(graph, value, axes, keep_dims))
    }
    pub fn layer_norm_low_f32(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxValue, MlxError> {
        self.transaction(|graph| low::layer_norm(graph, value, axes, epsilon))
    }
    pub fn softmax_low(&mut self, value: MlxValue, axes: &[usize]) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let result = low::softmax(graph, value, axes, false)?;
            casts::cast_to_low(graph, result, dtype)
        })
    }
    pub fn log_softmax_low(
        &mut self,
        value: MlxValue,
        axes: &[usize],
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let result = low::softmax(graph, value, axes, true)?;
            casts::cast_to_low(graph, result, dtype)
        })
    }
    pub fn logsumexp_low(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let result = low::logsumexp(graph, value, axes, keep_dims)?;
            casts::cast_to_low(graph, result, dtype)
        })
    }
    pub fn moments_low(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Moments<MlxValue>, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let result = low::moments(graph, value, axes, keep_dims)?;
            Ok(Moments {
                mean: casts::cast_to_low(graph, result.mean, dtype)?,
                variance: casts::cast_to_low(graph, result.variance, dtype)?,
            })
        })
    }
    pub fn layer_norm_low(
        &mut self,
        value: MlxValue,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<MlxValue, MlxError> {
        let (_, dtype) = self.low(value)?;
        self.transaction(|graph| {
            let result = low::layer_norm(graph, value, axes, epsilon)?;
            casts::cast_to_low(graph, result, dtype)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistics_pair_and_final_casts_roll_back_together_on_late_failure() {
        let backend = match MlxBackend::new_gpu() {
            Ok(b) => b,
            Err(error) => {
                assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
                return;
            }
        };
        let mut graph = backend.program();
        let input = graph
            .input_low(tensor_core::LowDtype::Bf16, Shape::new(vec![2, 3]).unwrap())
            .unwrap();
        let before = graph.nodes.len();
        let result: Result<Moments<MlxValue>, MlxError> = graph.transaction(|graph| {
            let pair = graph.moments_low(input, &[1], false)?;
            assert!(pair.mean.index >= before && pair.variance.index > pair.mean.index);
            // Model a caller composing another failing step after both output
            // casts were appended, so no half-built pair can escape rollback.
            graph.reshape(pair.variance, Shape::new(vec![3])?)?;
            Ok(pair)
        });
        assert!(result.is_err());
        assert_eq!(graph.nodes.len(), before);
        assert_eq!(graph.inputs.len(), 1);
        let output = graph.cast_to_f32(input).unwrap();
        assert_eq!(output.index, before);
    }
}
