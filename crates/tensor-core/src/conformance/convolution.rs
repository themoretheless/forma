//! Independent logical convolution oracle; no ConvPlan or Layout reuse.
use super::{
    attention::{Host, data},
    shape,
};
use crate::{ConvOptions, HasShape, TensorConvBackend};

mod low;
pub use low::check_low_conv_backend;

fn coordinates(mut flat: usize, dims: &[usize]) -> Vec<usize> {
    let mut result = vec![0; dims.len()];
    for axis in (0..dims.len()).rev() {
        result[axis] = flat % dims[axis];
        flat /= dims[axis];
    }
    result
}
fn address(dims: &[usize], coords: &[usize]) -> usize {
    dims.iter().zip(coords).fold(0, |offset, (&dim, &coord)| {
        offset * dim + if dim == 1 { 0 } else { coord }
    })
}
struct Input {
    host: Host<f32>,
    broadcast: Option<Vec<usize>>,
}
impl Input {
    fn new(dims: &[usize], seed: usize) -> Self {
        Self {
            host: data(dims, seed),
            broadcast: None,
        }
    }
    fn logical(&self, values: &[f32]) -> (Vec<usize>, Vec<f32>) {
        let physical = self.host.shape.dims();
        let axes = self
            .host
            .permutation
            .clone()
            .unwrap_or_else(|| (0..physical.len()).collect());
        let permuted: Vec<_> = axes.iter().map(|&a| physical[a]).collect();
        let dims = self.broadcast.clone().unwrap_or(permuted.clone());
        let output = (0..dims.iter().product())
            .map(|flat| {
                let logical = coordinates(flat, &dims);
                let mut coords = vec![0; physical.len()];
                for (axis, &original) in axes.iter().enumerate() {
                    coords[original] = if permuted[axis] == 1 {
                        0
                    } else {
                        logical[axis]
                    };
                }
                values[address(physical, &coords)]
            })
            .collect();
        (dims, output)
    }
    fn upload<B: TensorConvBackend>(&self, b: &B) -> Result<B::Tensor, B::Error> {
        let mut tensor = b.upload_f32(self.host.shape.clone(), &self.host.values)?;
        if let Some(axes) = &self.host.permutation {
            tensor = b.permute(&tensor, axes)?;
        }
        if let Some(dims) = &self.broadcast {
            tensor = b.broadcast_to(&tensor, shape(dims))?;
        }
        Ok(tensor)
    }
}

struct Case {
    input: Input,
    weight: Input,
    options: ConvOptions,
}
fn cases() -> Vec<Case> {
    let mut result = vec![
        Case {
            input: Input::new(&[2, 2, 9], 1),
            weight: Input::new(&[3, 2, 3], 2),
            options: ConvOptions {
                strides: vec![2],
                dilations: vec![2],
                padding_before: vec![1],
                padding_after: vec![2],
                groups: 1,
            },
        },
        Case {
            input: Input::new(&[1, 3, 6, 5], 3),
            weight: Input::new(&[6, 1, 2, 3], 4),
            options: ConvOptions {
                strides: vec![1, 2],
                dilations: vec![2, 1],
                padding_before: vec![0, 1],
                padding_after: vec![1, 0],
                groups: 3,
            },
        },
        Case {
            input: Input::new(&[1, 4, 4, 5, 6], 5),
            weight: Input::new(&[6, 2, 2, 2, 3], 6),
            options: ConvOptions {
                strides: vec![1, 2, 2],
                dilations: vec![2, 1, 1],
                padding_before: vec![1, 0, 2],
                padding_after: vec![0, 1, 0],
                groups: 2,
            },
        },
        Case {
            input: Input::new(&[1, 2, 265], 7),
            weight: Input::new(&[3, 2, 257], 8),
            options: ConvOptions::new(1),
        },
    ];
    for kernel in [255, 256, 257, 513] {
        result.push(Case {
            input: Input::new(&[1, 1, kernel + 4], kernel),
            weight: Input::new(&[3, 1, kernel], kernel + 1),
            options: ConvOptions::new(1),
        });
    }
    // Both operands are real strided views, not transposed oracle metadata.
    let mut input = Input::new(&[2, 7, 4, 5], 9);
    input.host.permutation = Some(vec![0, 2, 3, 1]);
    let mut weight = Input::new(&[2, 3, 6, 2], 10);
    weight.host.permutation = Some(vec![2, 0, 1, 3]);
    let options = ConvOptions {
        strides: vec![2, 1],
        dilations: vec![1, 2],
        padding_before: vec![1, 2],
        padding_after: vec![0, 1],
        groups: 2,
    };
    result.push(Case {
        input,
        weight,
        options,
    });
    let mut input = Input::new(&[1, 1, 5, 1], 11);
    input.broadcast = Some(vec![2, 4, 5, 7]);
    let mut weight = Input::new(&[1, 1, 3, 2], 12);
    weight.broadcast = Some(vec![6, 2, 3, 2]);
    let mut options = ConvOptions::new(2);
    options.groups = 2;
    result.push(Case {
        input,
        weight,
        options,
    });
    for (input, weight) in [
        (vec![0, 2, 7], vec![4, 2, 3]),
        (vec![2, 2, 7], vec![0, 2, 3]),
        (vec![2, 0, 7], vec![4, 0, 3]),
        (vec![2, 2, 1], vec![3, 2, 3]),
    ] {
        result.push(Case {
            input: Input::new(&input, 13),
            weight: Input::new(&weight, 14),
            options: ConvOptions::new(1),
        });
    }
    // No input value exists, but padding creates nonempty output windows.
    result.push(Case {
        input: Input::new(&[2, 2, 0, 3], 15),
        weight: Input::new(&[4, 2, 2, 2], 16),
        options: ConvOptions {
            strides: vec![1, 1],
            dilations: vec![1, 1],
            padding_before: vec![2, 1],
            padding_after: vec![1, 0],
            groups: 1,
        },
    });
    result
}

/// Direct dense f64 cross-correlation with signed mathematical coordinates.
/// Output geometry is derived independently of the production planner.
fn reference(
    input: (&[usize], &[f32]),
    weight: (&[usize], &[f32]),
    options: &ConvOptions,
) -> (Vec<usize>, Vec<f64>) {
    let spatial = input.0.len() - 2;
    let mut dims = vec![input.0[0], weight.0[0]];
    for axis in 0..spatial {
        let length = input.0[axis + 2] as i128
            + options.padding_before[axis] as i128
            + options.padding_after[axis] as i128;
        let window = (weight.0[axis + 2] as i128 - 1) * options.dilations[axis] as i128 + 1;
        dims.push(if length < window {
            0
        } else {
            ((length - window) / options.strides[axis] as i128 + 1) as usize
        });
    }
    let mut values = Vec::new();
    for flat in 0..dims.iter().product() {
        let out = coordinates(flat, &dims);
        let group = out[1] / (weight.0[0] / options.groups);
        let mut sum = 0f64;
        for channel in 0..weight.0[1] {
            for kernel in 0..weight.0[2..].iter().product() {
                let k = coordinates(kernel, &weight.0[2..]);
                let mut source = vec![out[0], group * weight.0[1] + channel];
                let mut inside = true;
                for axis in 0..spatial {
                    let x = out[axis + 2] as i128 * options.strides[axis] as i128
                        + k[axis] as i128 * options.dilations[axis] as i128
                        - options.padding_before[axis] as i128;
                    if x < 0 || x >= input.0[axis + 2] as i128 {
                        inside = false;
                        break;
                    }
                    source.push(x as usize);
                }
                if inside {
                    let mut filter = vec![out[1], channel];
                    filter.extend(k);
                    sum += f64::from(input.1[address(input.0, &source)])
                        * f64::from(weight.1[address(weight.0, &filter)]);
                }
            }
        }
        values.push(sum);
    }
    (dims, values)
}

fn exact(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.is_finite() && f64::from(a) == e,
            "conv[{i}]: {a:?} != exact {e:?}"
        );
    }
}

/// Shared finite f32 qualification. Dyadic inputs/products and all possible
/// accumulation partials fit f32 exactly in these fixtures, so equality does
/// not depend on FMA or reduction order. The oracle never calls ConvPlan.
pub fn check_conv_backend<B: TensorConvBackend>(b: &B) -> Result<(), B::Error> {
    for case in cases() {
        let input = case.input.upload(b)?;
        let weight = case.weight.upload(b)?;
        let (id, iv) = case.input.logical(&case.input.host.values);
        let (wd, wv) = case.weight.logical(&case.weight.host.values);
        let (out, expected) = reference((&id, &iv), (&wd, &wv), &case.options);
        let result = b.conv(&input, &weight, &case.options)?;
        assert_eq!(result.shape().dims(), out);
        exact(&b.read_f32(&result)?, &expected);
        assert_eq!(b.read_f32(&input)?, iv);
        assert_eq!(b.read_f32(&weight)?, wv);
    }
    // Inexact products exercise normal f32 arithmetic, with an independent
    // absolute-sum error scale instead of relative error near cancellation.
    let input: Vec<_> = (0..65).map(|i| (i as f32 - 30.) / 37.).collect();
    let weight: Vec<_> = (0..65)
        .map(|i| ((i * 17) % 31) as f32 / 23. - 0.7)
        .collect();
    let expected = input
        .iter()
        .zip(&weight)
        .map(|(&a, &b)| f64::from(a) * f64::from(b))
        .sum::<f64>();
    let absolute = input
        .iter()
        .zip(&weight)
        .map(|(&a, &b)| (f64::from(a) * f64::from(b)).abs())
        .sum::<f64>();
    let a = b.upload_f32(shape(&[1, 1, 65]), &input)?;
    let w = b.upload_f32(shape(&[1, 1, 65]), &weight)?;
    let result = b.conv(&a, &w, &ConvOptions::new(1))?;
    let got = b.read_f32(&result)?;
    assert_eq!(got.len(), 1);
    assert!(got[0].is_finite() && (f64::from(got[0]) - expected).abs() <= 2e-5 * absolute);
    invalid(b)?;
    Ok(())
}

fn invalid<B: TensorConvBackend>(b: &B) -> Result<(), B::Error> {
    let a = b.upload_f32(shape(&[0, 2, 5]), &[])?;
    let w = b.upload_f32(shape(&[4, 2, 3]), &[1.; 24])?;
    for option in [
        ConvOptions::new(0),
        ConvOptions::new(2),
        ConvOptions {
            groups: 0,
            ..ConvOptions::new(1)
        },
        ConvOptions {
            strides: vec![0],
            ..ConvOptions::new(1)
        },
        ConvOptions {
            dilations: vec![0],
            ..ConvOptions::new(1)
        },
        ConvOptions {
            groups: 3,
            ..ConvOptions::new(1)
        },
    ] {
        assert!(b.conv(&a, &w, &option).is_err());
    }
    let malformed = b.upload_f32(shape(&[0, 2]), &[])?;
    assert!(b.conv(&malformed, &w, &ConvOptions::new(1)).is_err());
    let wrong_channels = b.upload_f32(shape(&[4, 1, 3]), &[1.; 12])?;
    assert!(b.conv(&a, &wrong_channels, &ConvOptions::new(1)).is_err());
    let zero_kernel = b.upload_f32(shape(&[4, 2, 0]), &[])?;
    assert!(b.conv(&a, &zero_kernel, &ConvOptions::new(1)).is_err());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_is_cross_correlation_with_signed_padding_and_groups() {
        let (s, v) = reference(
            (&[1, 1, 4], &[1., 2., 3., 4.]),
            (&[1, 1, 2], &[2., -1.]),
            &ConvOptions::new(1),
        );
        assert_eq!(s, [1, 1, 3]);
        assert_eq!(v, [0., 1., 2.]);
        let o = ConvOptions {
            strides: vec![2],
            dilations: vec![2],
            padding_before: vec![1],
            padding_after: vec![2],
            groups: 1,
        };
        assert_eq!(
            reference(
                (&[1, 1, 4], &[1., 2., 3., 4.]),
                (&[1, 1, 2], &[2., -1.]),
                &o
            )
            .1,
            [-2., 0., 8.]
        );
        let mut o = ConvOptions::new(1);
        o.groups = 2;
        assert_eq!(
            reference(
                (&[1, 2, 2], &[1., 2., 10., 20.]),
                (&[2, 1, 1], &[2., 3.]),
                &o
            )
            .1,
            [2., 4., 30., 60.]
        );
    }
}
