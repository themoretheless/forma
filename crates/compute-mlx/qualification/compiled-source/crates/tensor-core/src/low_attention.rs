use crate::{
    AttentionMask, AttentionOptions, AttentionPlan, HasLowDtype, Shape, TensorAttentionBackend,
    TensorError, TensorLowBackend, low_precision::check_low_dtypes,
};

/// Masked scaled dot-product attention from resident f16/BF16 Q, K and V.
/// All three operands must have the same dtype, including for empty results.
/// Geometry, broadcasting, grouped query heads, signed causal alignment and
/// empty/fully-masked rows follow `TensorAttentionBackend`. Additive masks stay
/// f32, and keep masks stay u32; mask values do not round to the input dtype.
///
/// Evaluation retains f32 accuracy or wider intermediates. Finite inputs,
/// allowed dot products/logits and normalized results obey the f32 attention
/// range contract. Normal products from BF16 subnormal operands and large
/// finite partners must not disappear through an initial low-input flush.
/// True subnormal arithmetic/results otherwise follow backend f32 limits.
/// Constant finite V must not overflow an internal unnormalized numerator.
///
/// `attention_low_f32` returns the evaluated result without an intermediate
/// low rounding. `attention_low` rounds it once, nearest-even, into Q's dtype.
/// The final low conversion follows `TensorLowBackend`; transcendental and
/// reduction results use numerical tolerances rather than bitwise equality.
///
/// All data and intermediates stay on device. Implementations read low inputs
/// directly, without expanding entire Q/K/V tensors or materializing complete
/// score/probability matrices. Bounded tiles/partials, row statistics and actual
/// output-sized accumulators are allowed.
pub trait TensorLowAttentionBackend: TensorLowBackend + TensorAttentionBackend {
    fn attention_low_f32(
        &self,
        query: &Self::LowTensor,
        key: &Self::LowTensor,
        value: &Self::LowTensor,
        mask: AttentionMask<'_, Self::Tensor, Self::UIntTensor>,
        options: AttentionOptions,
    ) -> Result<Self::Tensor, Self::Error>;

    fn attention_low(
        &self,
        query: &Self::LowTensor,
        key: &Self::LowTensor,
        value: &Self::LowTensor,
        mask: AttentionMask<'_, Self::Tensor, Self::UIntTensor>,
        options: AttentionOptions,
    ) -> Result<Self::LowTensor, Self::Error> {
        let result = self.attention_low_f32(query, key, value, mask, options)?;
        self.cast_to_low(&result, query.low_dtype())
    }
}

/// Validate all storage dtypes before planning geometry or taking an empty
/// shortcut. Runtime ownership remains the executor's responsibility.
pub fn low_attention_plan(
    query: &impl HasLowDtype,
    key: &impl HasLowDtype,
    value: &impl HasLowDtype,
    mask: Option<&Shape>,
    options: AttentionOptions,
) -> Result<AttentionPlan, TensorError> {
    check_low_dtypes(query, key)?;
    check_low_dtypes(query, value)?;
    AttentionPlan::new(query.shape(), key.shape(), value.shape(), mask, options)
}
