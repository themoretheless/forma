use super::shape;
use crate::{AttentionMask, AttentionOptions, HasShape, Layout, Shape, TensorAttentionBackend};

struct Host<T> {
    shape: Shape,
    values: Vec<T>,
    permutation: Option<Vec<usize>>,
}
impl<T: Copy> Host<T> {
    fn logical(&self) -> (Shape, Vec<T>) {
        let mut layout = Layout::contiguous(self.shape.clone()).unwrap();
        if let Some(axes) = &self.permutation {
            layout = layout.permute(axes).unwrap();
        }
        (
            layout.shape().clone(),
            (0..layout.shape().numel())
                .map(|i| self.values[layout.element_offset(i).unwrap()])
                .collect(),
        )
    }
}
fn data(dims: &[usize], seed: usize) -> Host<f32> {
    let shape = shape(dims);
    Host {
        values: (0..shape.numel())
            .map(|i| ((i * 17 + i / 19 + seed * 7) % 43) as f32 / 16.0 - 1.25)
            .collect(),
        shape,
        permutation: None,
    }
}
fn upload<B: TensorAttentionBackend>(b: &B, host: &Host<f32>) -> Result<B::Tensor, B::Error> {
    let input = b.upload_f32(host.shape.clone(), &host.values)?;
    match &host.permutation {
        Some(axes) => b.permute(&input, axes),
        None => Ok(input),
    }
}
enum Mask {
    None,
    Keep(Host<u32>),
    Additive(Host<f32>),
}
fn check<B: TensorAttentionBackend>(
    b: &B,
    q: Host<f32>,
    k: Host<f32>,
    v: Host<f32>,
    mask: Mask,
    options: AttentionOptions,
) -> Result<(), B::Error> {
    let (q_shape, q_values) = q.logical();
    let (k_shape, k_values) = k.logical();
    let (v_shape, v_values) = v.logical();
    let query = upload(b, &q)?;
    let key = upload(b, &k)?;
    let value = upload(b, &v)?;
    let (bias_shape, bias_values, keep, additive) = match &mask {
        Mask::None => (None, Vec::new(), None, None),
        Mask::Additive(host) => {
            let (shape, values) = host.logical();
            (Some(shape), values, None, Some(upload(b, host)?))
        }
        Mask::Keep(host) => {
            let input = b.upload_u32(host.shape.clone(), &host.values)?;
            let input = match &host.permutation {
                Some(axes) => b.permute_u32(&input, axes)?,
                None => input,
            };
            let (shape, values) = host.logical();
            (
                Some(shape),
                values
                    .iter()
                    .map(|&x| if x == 0 { f32::NEG_INFINITY } else { 0.0 })
                    .collect(),
                Some(input),
                None,
            )
        }
    };
    let mask = match (&keep, &additive) {
        (Some(input), _) => AttentionMask::Keep(input),
        (_, Some(input)) => AttentionMask::Additive(input),
        _ => AttentionMask::None,
    };
    let actual = b.attention(&query, &key, &value, mask, options)?;
    let (expected_shape, expected) = reference(
        (&q_shape, &q_values),
        (&k_shape, &k_values),
        (&v_shape, &v_values),
        bias_shape.as_ref().map(|s| (s, bias_values.as_slice())),
        options,
    );
    assert_eq!(actual.shape(), &expected_shape);
    let actual = b.read_f32(&actual)?;
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
        assert!(
            a.is_finite() && (f64::from(a) - e).abs() <= 3e-4 * e.abs().max(1.0),
            "attention Q{:?} K{:?} V{:?} options{options:?} [{i}]: {a} != {e}",
            q_shape.dims(),
            k_shape.dims(),
            v_shape.dims()
        );
    }
    // The public operation must leave its source tensors unchanged.
    assert_eq!(b.read_f32(&query)?, q_values);
    assert_eq!(b.read_f32(&key)?, k_values);
    assert_eq!(b.read_f32(&value)?, v_values);
    Ok(())
}

/// Independent dense f64 attention oracle. This does not use AttentionPlan,
/// online updates, the backend's matmul or the shared statistics fixture.
fn reference(
    query: (&Shape, &[f32]),
    key: (&Shape, &[f32]),
    value: (&Shape, &[f32]),
    mask: Option<(&Shape, &[f32])>,
    options: AttentionOptions,
) -> (Shape, Vec<f64>) {
    fn canonical(shape: &Shape) -> Vec<usize> {
        if shape.rank() == 2 {
            vec![1, shape.dims()[0], shape.dims()[1]]
        } else {
            shape.dims().to_vec()
        }
    }
    fn coords(mut index: usize, dims: &[usize]) -> Vec<usize> {
        let mut out = vec![0; dims.len()];
        for (i, &dim) in dims.iter().enumerate().rev() {
            out[i] = index % dim;
            index /= dim;
        }
        out
    }
    fn address(dims: &[usize], coordinates: &[usize]) -> usize {
        let start = coordinates.len() - dims.len();
        dims.iter().enumerate().fold(0, |index, (i, &dim)| {
            index * dim + if dim == 1 { 0 } else { coordinates[start + i] }
        })
    }
    let q = canonical(query.0);
    let k = canonical(key.0);
    let v = canonical(value.0);
    let n = q[q.len() - 2];
    let keys = k[k.len() - 2];
    let d = q[q.len() - 1];
    let dv = v[v.len() - 1];
    let heads = q[q.len() - 3];
    let group = heads / k[k.len() - 3];
    let batch_rank = q.len().max(k.len()).max(v.len()) - 3;
    let mut batches = vec![1; batch_rank];
    for input in [&q, &k, &v] {
        let prefix = &input[..input.len() - 3];
        for (axis, &dim) in prefix.iter().enumerate() {
            let dest = batch_rank - prefix.len() + axis;
            if dim != 1 {
                batches[dest] = dim;
            }
        }
    }
    let mut out_dims = batches.clone();
    out_dims.extend([heads, n, dv]);
    if query.0.rank() == 2 && key.0.rank() == 2 && value.0.rank() == 2 {
        out_dims = vec![n, dv];
    }
    let output_shape = shape(&out_dims);
    if output_shape.is_empty() {
        return (output_shape, Vec::new());
    }
    let batch_count: usize = batches.iter().product();
    let scale = f64::from(options.scale.unwrap_or_else(|| (d as f32).sqrt().recip()));
    let mut output = Vec::with_capacity(output_shape.numel());
    for batch in 0..batch_count {
        let prefix = coords(batch, &batches);
        for head in 0..heads {
            for row in 0..n {
                let mut logits = vec![f64::NEG_INFINITY; keys];
                for (column, score) in logits.iter_mut().enumerate() {
                    if options
                        .causal
                        .is_some_and(|offset| column as i128 > row as i128 + i128::from(offset))
                    {
                        continue;
                    }
                    let mut score_coords = prefix.clone();
                    score_coords.extend([head, row, column]);
                    let bias = mask.map_or(0.0, |(shape, data)| {
                        f64::from(data[address(shape.dims(), &score_coords)])
                    });
                    if bias == f64::NEG_INFINITY {
                        continue;
                    }
                    let mut qc = prefix.clone();
                    qc.extend([head, row, 0]);
                    let mut kc = prefix.clone();
                    kc.extend([head / group, column, 0]);
                    let mut dot = 0.0;
                    for inner in 0..d {
                        *qc.last_mut().unwrap() = inner;
                        *kc.last_mut().unwrap() = inner;
                        dot += f64::from(query.1[address(&q, &qc)])
                            * f64::from(key.1[address(&k, &kc)]);
                    }
                    *score = dot * scale + bias;
                }
                let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if max == f64::NEG_INFINITY {
                    output.extend(vec![0.0; dv]);
                    continue;
                }
                let weights: Vec<f64> = logits.iter().map(|&x| (x - max).exp()).collect();
                let sum: f64 = weights.iter().sum();
                for channel in 0..dv {
                    let mut vc = prefix.clone();
                    vc.extend([head / group, 0, channel]);
                    let result = weights
                        .iter()
                        .enumerate()
                        .map(|(col, &weight)| {
                            vc[prefix.len() + 1] = col;
                            weight / sum * f64::from(value.1[address(&v, &vc)])
                        })
                        .sum();
                    output.push(result);
                }
            }
        }
    }
    (output_shape, output)
}

/// Covers dense/tail/strided attention, GQA, batch broadcasting, mask semantics,
/// signed causal offsets, empty geometry and numerical stability on every backend.
pub fn check_attention_backend<B: TensorAttentionBackend>(b: &B) -> Result<(), B::Error> {
    let options = AttentionOptions::default();
    for (m, n, d, dv) in [
        (3, 5, 7, 9),
        (33, 65, 17, 67),
        (2, 257, 65, 3),
        (1, 4097, 3, 5),
    ] {
        check(
            b,
            data(&[m, d], 1),
            data(&[n, d], 2),
            data(&[n, dv], 3),
            Mask::None,
            options,
        )?;
    }
    check(
        b,
        data(&[2, 1, 6, 3, 5], 1),
        data(&[1, 3, 2, 7, 5], 2),
        data(&[3, 2, 7, 9], 3),
        Mask::None,
        options,
    )?;
    let mut q = data(&[2, 5, 4, 3], 1);
    q.permutation = Some(vec![0, 2, 3, 1]);
    let mut k = data(&[1, 7, 2, 5], 2);
    k.permutation = Some(vec![0, 2, 1, 3]);
    let mut v = data(&[2, 9, 7], 3);
    v.permutation = Some(vec![0, 2, 1]);
    let keep = Host {
        shape: shape(&[7, 3]),
        values: (0..21)
            .map(|i| {
                if i % 3 == 0 || i % 5 == 0 {
                    0
                } else {
                    u32::MAX
                }
            })
            .collect(),
        permutation: Some(vec![1, 0]),
    };
    check(b, q, k, v, Mask::Keep(keep), options)?;
    for causal in [
        None,
        Some(0),
        Some(4),
        Some(-2),
        Some(i32::MIN),
        Some(i32::MAX),
    ] {
        check(
            b,
            data(&[2, 4, 3, 5], 1),
            data(&[1, 2, 7, 5], 2),
            data(&[1, 2, 7, 4], 3),
            Mask::None,
            AttentionOptions {
                causal,
                scale: None,
            },
        )?;
    }
    for mode in 0..3 {
        let mask = match mode {
            0 => Mask::Keep(Host {
                shape: shape(&[]),
                values: vec![0],
                permutation: None,
            }),
            1 => Mask::Additive(Host {
                shape: shape(&[3, 7]),
                values: (0..21)
                    .map(|i| {
                        if i / 7 == 1 || i % 4 == 0 {
                            f32::NEG_INFINITY
                        } else {
                            (i % 5) as f32 - 2.0
                        }
                    })
                    .collect(),
                permutation: None,
            }),
            _ => Mask::Keep(Host {
                shape: shape(&[7]),
                values: vec![0, 1, u32::MAX, 0, 7, 0, 1],
                permutation: None,
            }),
        };
        check(
            b,
            data(&[4, 3, 5], 1),
            data(&[2, 7, 5], 2),
            data(&[2, 7, 3], 3),
            mask,
            AttentionOptions {
                scale: Some(-0.5),
                causal: Some(1),
            },
        )?;
    }
    for scale in [Some(0.0), Some(2.0), None] {
        let mut q = data(&[3, 1], 1);
        q.values = vec![1., -1., 0.];
        let mut k = data(&[65, 1], 2);
        k.values = (0..65).map(|i| 10000. + (i % 7) as f32).collect();
        check(
            b,
            q,
            k,
            data(&[65, 5], 3),
            Mask::None,
            AttentionOptions {
                scale,
                causal: None,
            },
        )?;
    }
    // Finite convex outputs must survive an overflowing unnormalized PV sum.
    for (m, d, large) in [(1, 3, 1e30), (1, 64, f32::MAX), (17, 64, f32::MAX)] {
        let mut q = data(&[m, d], 1);
        q.values.fill(0.0);
        let mut k = data(&[65, d], 2);
        k.values.fill(0.0);
        let mut v = data(&[65, d], 3);
        v.values.fill(large);
        check(b, q, k, v, Mask::None, options)?;
    }
    // A discarded large-valued tile must not destroy a later ordinary value.
    let mut q = data(&[1, 1], 1);
    q.values.fill(1.0);
    let mut k = data(&[65, 1], 2);
    k.values = (0..65).map(|i| if i < 32 { 0.0 } else { 200.0 }).collect();
    let mut v = data(&[65, 1], 3);
    v.values = (0..65)
        .map(|i| if i < 32 { f32::MAX } else { 1.0 })
        .collect();
    check(b, q, k, v, Mask::None, options)?;
    let mut q = data(&[1, 1], 1);
    q.values.fill(0.0);
    let mut v = data(&[65, 1], 3);
    v.values = (0..65)
        .map(|i| if i < 32 { f32::MAX } else { 1.0 })
        .collect();
    let keep = Host {
        shape: shape(&[65]),
        values: (0..65).map(|i| u32::from(i >= 32)).collect(),
        permutation: None,
    };
    check(b, q, data(&[65, 1], 2), v, Mask::Keep(keep), options)?;
    // These route native optimized kernels on implementations with common head sizes.
    for m in [1, 17] {
        let keep = Host {
            shape: shape(&[m, 65]),
            values: (0..m * 65)
                .map(|i| if i / 65 == 0 { 0 } else { 1 })
                .collect(),
            permutation: None,
        };
        check(
            b,
            data(&[1, 4, m, 64], 1),
            data(&[1, 2, 65, 64], 2),
            data(&[1, 2, 65, 64], 3),
            Mask::Keep(keep),
            options,
        )?;
    }
    for (m, n, dv) in [(0, 3, 5), (3, 0, 5), (3, 5, 0)] {
        check(
            b,
            data(&[m, 4], 1),
            data(&[n, 4], 2),
            data(&[n, dv], 3),
            Mask::None,
            options,
        )?;
    }
    check(
        b,
        data(&[0, 4, 3, 5], 1),
        data(&[1, 2, 7, 5], 2),
        data(&[2, 7, 3], 3),
        Mask::None,
        options,
    )?;
    let query = b.upload_f32(shape(&[3, 4]), &[1.; 12])?;
    let key = b.upload_f32(shape(&[7, 4]), &[1.; 28])?;
    let value = b.upload_f32(shape(&[7, 5]), &[1.; 35])?;
    let bad = b.upload_u32(shape(&[2, 3, 7]), &[1; 42])?;
    assert!(
        b.attention(&query, &key, &value, AttentionMask::Keep(&bad), options)
            .is_err()
    );
    for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            b.attention(
                &query,
                &key,
                &value,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(scale),
                    causal: None
                }
            )
            .is_err()
        );
    }
    let scalar = b.upload_f32(shape(&[]), &[1.])?;
    assert!(
        b.attention(&scalar, &key, &value, AttentionMask::None, options)
            .is_err()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_matches_hand_computed_uniform_grouped_attention() {
        let q = shape(&[4, 1, 1]);
        let k = shape(&[2, 2, 1]);
        let v = shape(&[2, 2, 1]);
        let (result_shape, values) = reference(
            (&q, &[1., 2., 3., 4.]),
            (&k, &[3., 5., 7., 11.]),
            (&v, &[2., 4., 6., 10.]),
            None,
            AttentionOptions {
                scale: Some(0.0),
                causal: None,
            },
        );
        assert_eq!(result_shape, shape(&[4, 1, 1]));
        assert_eq!(values, [3.0, 3.0, 8.0, 8.0]);
    }

    #[test]
    fn reference_respects_bias_exclusion_and_signed_causal_rows() {
        let q = shape(&[3, 1]);
        let k = shape(&[2, 1]);
        let mask = shape(&[2]);
        let (_, values) = reference(
            (&q, &[1., 1., 1.]),
            (&k, &[0., 0.]),
            (&k, &[5., 17.]),
            Some((&mask, &[0., f32::NEG_INFINITY])),
            AttentionOptions {
                scale: None,
                causal: Some(-1),
            },
        );
        assert_eq!(values, [0.0, 5.0, 5.0]);
    }
}
