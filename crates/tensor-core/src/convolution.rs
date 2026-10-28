use crate::{Shape, TensorBackend, TensorError};

/// Channel-first cross-correlation geometry. Padding is explicit zero padding;
/// vectors follow the input's spatial axis order. There is no implicit SAME or
/// VALID mode, bias, kernel reversal, or channel broadcasting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvOptions {
    pub strides: Vec<usize>,
    pub dilations: Vec<usize>,
    pub padding_before: Vec<usize>,
    pub padding_after: Vec<usize>,
    pub groups: usize,
}
impl ConvOptions {
    /// Unit stride/dilation, no padding and one group. `ConvPlan::new` validates
    /// that the spatial rank is one, two or three and all option vectors match.
    pub fn new(spatial_rank: usize) -> Self {
        Self {
            strides: vec![1; spatial_rank],
            dilations: vec![1; spatial_rank],
            padding_before: vec![0; spatial_rank],
            padding_after: vec![0; spatial_rank],
            groups: 1,
        }
    }
}

/// Validated logical convolution geometry; storage layouts stay backend-owned.
/// Input is `[N,C,*spatial]`, weight `[O,C/groups,*kernel]`, output
/// `[N,O,*output_spatial]`. Every kernel extent is positive. N, C and O may be
/// zero, provided channel/group divisibility and the weight shape still match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvPlan {
    pub input: Shape,
    pub weight: Shape,
    pub output: Shape,
    pub options: ConvOptions,
    pub spatial_rank: usize,
    pub channels_per_group: usize,
    pub outputs_per_group: usize,
}
impl ConvPlan {
    pub fn new(input: &Shape, weight: &Shape, options: &ConvOptions) -> Result<Self, TensorError> {
        let invalid = TensorError::InvalidConvolution;
        if !(3..=5).contains(&input.rank()) || weight.rank() != input.rank() {
            return Err(invalid(
                "input and weight need equal rank, with one to three spatial axes",
            ));
        }
        let spatial_rank = input.rank() - 2;
        if options.strides.len() != spatial_rank
            || options.dilations.len() != spatial_rank
            || options.padding_before.len() != spatial_rank
            || options.padding_after.len() != spatial_rank
        {
            return Err(invalid("every option vector must match the spatial rank"));
        }
        let channels = input.dims()[1];
        let outputs = weight.dims()[0];
        if options.groups == 0
            || !channels.is_multiple_of(options.groups)
            || !outputs.is_multiple_of(options.groups)
        {
            return Err(invalid(
                "groups must be positive and divide input and output channels",
            ));
        }
        let channels_per_group = channels / options.groups;
        let outputs_per_group = outputs / options.groups;
        if weight.dims()[1] != channels_per_group {
            return Err(invalid(
                "weight input channels must equal input channels divided by groups",
            ));
        }
        let mut output = Vec::with_capacity(spatial_rank + 2);
        output.push(input.dims()[0]);
        output.push(outputs);
        for axis in 0..spatial_rank {
            let kernel = weight.dims()[axis + 2];
            let stride = options.strides[axis];
            let dilation = options.dilations[axis];
            if kernel == 0 || stride == 0 || dilation == 0 {
                return Err(invalid(
                    "kernel extents, strides and dilations must be positive",
                ));
            }
            // Validate both expressions even if N/O/C or another axis is zero.
            let padded = input.dims()[axis + 2]
                .checked_add(options.padding_before[axis])
                .and_then(|n| n.checked_add(options.padding_after[axis]))
                .ok_or(TensorError::ShapeOverflow)?;
            let effective = (kernel - 1)
                .checked_mul(dilation)
                .and_then(|n| n.checked_add(1))
                .ok_or(TensorError::ShapeOverflow)?;
            output.push(if padded < effective {
                0
            } else {
                (padded - effective) / stride + 1
            });
        }
        Ok(Self {
            input: input.clone(),
            weight: weight.clone(),
            output: Shape::new(output)?,
            options: options.clone(),
            spatial_rank,
            channels_per_group,
            outputs_per_group,
        })
    }
}

/// Resident forward 1D/2D/3D cross-correlation. For output channel o, group
/// `o/(O/groups)` selects the contiguous input-channel group; spatial samples
/// are `output*stride + kernel*dilation - padding_before`. Out-of-bounds samples
/// contribute zero. Kernel coordinates are not reversed.
///
/// Inputs and intermediate products/sums must be finite f32. Accumulation is
/// f32 or wider; FMA/reduction order and underflow follow backend f32 limits.
/// Empty outputs are valid. Zero input channels produce zeros, as do windows
/// containing only padding (including a zero-length input spatial axis).
/// Ownership, shape and dtype validation precede empty shortcuts. Bias composes
/// through resident binary operations. No CPU fallback or input readback.
pub trait TensorConvBackend: TensorBackend {
    fn conv(
        &self,
        input: &Self::Tensor,
        weight: &Self::Tensor,
        options: &ConvOptions,
    ) -> Result<Self::Tensor, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(dims: &[usize]) -> Shape {
        Shape::new(dims.to_vec()).unwrap()
    }

    #[test]
    fn grouped_asymmetric_dilated_geometry() {
        let options = ConvOptions {
            strides: vec![2, 1],
            dilations: vec![1, 2],
            padding_before: vec![1, 2],
            padding_after: vec![0, 1],
            groups: 4,
        };
        let plan = ConvPlan::new(&s(&[2, 4, 5, 7]), &s(&[8, 1, 3, 2]), &options).unwrap();
        assert_eq!(plan.output.dims(), [2, 8, 2, 8]);
        assert_eq!(
            (
                plan.spatial_rank,
                plan.channels_per_group,
                plan.outputs_per_group
            ),
            (2, 1, 2)
        );
        assert_eq!(plan.options, options);
        assert_eq!(
            ConvPlan::new(
                &s(&[1, 2, 5, 6, 7]),
                &s(&[4, 2, 2, 3, 4]),
                &ConvOptions::new(3)
            )
            .unwrap()
            .output
            .dims(),
            [1, 4, 4, 4, 4]
        );
    }

    #[test]
    fn zeros_have_geometry_even_without_a_contraction() {
        for (input, weight, output) in [
            (vec![0, 2, 5], vec![4, 2, 3], vec![0, 4, 3]),
            (vec![3, 0, 5], vec![4, 0, 3], vec![3, 4, 3]),
            (vec![3, 2, 5], vec![0, 2, 3], vec![3, 0, 3]),
            (vec![3, 2, 1], vec![4, 2, 3], vec![3, 4, 0]),
        ] {
            assert_eq!(
                ConvPlan::new(&s(&input), &s(&weight), &ConvOptions::new(1))
                    .unwrap()
                    .output
                    .dims(),
                output
            );
        }
        let mut options = ConvOptions::new(1);
        options.padding_before[0] = 2;
        options.padding_after[0] = 3;
        assert_eq!(
            ConvPlan::new(&s(&[2, 3, 0]), &s(&[6, 3, 3]), &options)
                .unwrap()
                .output
                .dims(),
            [2, 6, 3]
        );
        options.groups = 7;
        assert!(ConvPlan::new(&s(&[2, 0, 1]), &s(&[0, 0, 1]), &options).is_ok());
    }

    #[test]
    fn invalid_geometry_is_not_bypassed_by_empty_inputs() {
        let input = s(&[0, 4, 5]);
        let weight = s(&[6, 2, 3]);
        let mut options = ConvOptions::new(1);
        options.groups = 2;
        assert!(ConvPlan::new(&input, &weight, &options).is_ok());
        for rank in [0, 1, 2, 6] {
            assert!(
                ConvPlan::new(
                    &s(&vec![1; rank]),
                    &s(&vec![1; rank]),
                    &ConvOptions::new(rank.saturating_sub(2))
                )
                .is_err()
            );
        }
        assert!(ConvPlan::new(&input, &s(&[0, 2, 3, 1]), &options).is_err());
        for group in [0, 3, 4] {
            let mut bad = options.clone();
            bad.groups = group;
            assert!(ConvPlan::new(&input, &weight, &bad).is_err());
        }
        assert!(ConvPlan::new(&input, &s(&[6, 1, 3]), &options).is_err());
        assert!(ConvPlan::new(&input, &s(&[6, 2, 0]), &options).is_err());
        for field in 0..4 {
            let mut bad = options.clone();
            [
                &mut bad.strides,
                &mut bad.dilations,
                &mut bad.padding_before,
                &mut bad.padding_after,
            ][field]
                .push(0);
            assert!(ConvPlan::new(&input, &weight, &bad).is_err());
        }
        for stride in [true, false] {
            let mut bad = options.clone();
            if stride {
                bad.strides[0] = 0;
            } else {
                bad.dilations[0] = 0;
            }
            assert!(ConvPlan::new(&input, &weight, &bad).is_err());
        }
    }

    #[test]
    fn arithmetic_overflow_is_checked_including_empty_shapes() {
        let mut options = ConvOptions::new(1);
        options.padding_after[0] = 1;
        assert_eq!(
            ConvPlan::new(&s(&[0, 0, usize::MAX]), &s(&[1, 0, 1]), &options),
            Err(TensorError::ShapeOverflow)
        );
        options = ConvOptions::new(1);
        options.dilations[0] = 2;
        assert_eq!(
            ConvPlan::new(&s(&[0, 0, 1]), &s(&[0, 0, usize::MAX]), &options),
            Err(TensorError::ShapeOverflow)
        );
        assert_eq!(
            ConvPlan::new(
                &s(&[usize::MAX, 0, 1]),
                &s(&[2, 0, 1]),
                &ConvOptions::new(1)
            ),
            Err(TensorError::ShapeOverflow)
        );
        // Zero N/O permits a large conceptual spatial product; there is no
        // kernel-volume or output-volume allocation merely to validate it.
        assert!(
            ConvPlan::new(
                &s(&[0, 0, usize::MAX, usize::MAX]),
                &s(&[0, 0, 1, 1]),
                &ConvOptions::new(2)
            )
            .is_ok()
        );
    }
}
