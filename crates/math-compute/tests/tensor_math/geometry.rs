use math_compute::tensor::TensorMath;
use std::error::Error;
use tensor_core::{
    HasShape, MatmulPrecision, Shape, TensorBackend, TensorReduceBackend, TensorStatsBackend,
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

/// Independent logical f64 reference: never calls a tensor shape/layout planner
/// or a production math-core numerical helper.
fn stats(points: &[[f32; 3]]) -> ([f64; 3], [f64; 9], [f64; 9]) {
    let n = points.len() as f64;
    let origin = points[0].map(f64::from);
    let delta_mean: [f64; 3] = std::array::from_fn(|a| {
        points
            .iter()
            .map(|p| f64::from(p[a]) - origin[a])
            .sum::<f64>()
            / n
    });
    let mean = std::array::from_fn(|a| origin[a] + delta_mean[a]);
    let raw = std::array::from_fn(|ij| {
        points
            .iter()
            .map(|p| f64::from(p[ij / 3]) * f64::from(p[ij % 3]) / n)
            .sum()
    });
    let covariance = std::array::from_fn(|ij| {
        let (a, b) = (ij / 3, ij % 3);
        points
            .iter()
            .map(|p| {
                (f64::from(p[a]) - origin[a] - delta_mean[a])
                    * (f64::from(p[b]) - origin[b] - delta_mean[b])
                    / n
            })
            .sum()
    });
    (mean, raw, covariance)
}

fn close(actual: &[f32], expected: &[f64], relative: f64, floor: f64) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            e.is_finite() && e.abs() <= f64::from(f32::MAX),
            "reference[{i}] outside finite f32: {e}"
        );
        let bound = relative * e.abs().max(floor);
        assert!(
            a.is_finite() && (f64::from(a) - e).abs() <= bound,
            "[{i}] {a:?} != {e:?}, bound {bound}"
        );
    }
}

fn input<B: TensorBackend>(b: &B, points: &[[f32; 3]], strided: bool) -> Result<B::Tensor>
where
    B::Error: 'static,
{
    if strided {
        let values: Vec<_> = (0..3)
            .flat_map(|a| points.iter().map(move |p| p[a]))
            .collect();
        let physical = b.upload_f32(shape(&[3, points.len()]), &values)?;
        Ok(b.permute(&physical, &[1, 0])?)
    } else {
        Ok(b.upload_f32(shape(&[points.len(), 3]), points.as_flattened())?)
    }
}

fn check_stats<B: TensorReduceBackend + TensorStatsBackend>(
    b: &B,
    points: &[[f32; 3]],
    strided: bool,
) -> Result
where
    B::Error: 'static,
{
    let math = TensorMath::new(b);
    let source = input(b, points, strided)?;
    let result = math.point_cloud_stats(&source)?;
    assert_eq!(result.bounds.samples, points.len());
    assert_eq!(result.moments.samples, points.len());
    assert_eq!(result.bounds.min.shape().dims(), [3]);
    assert_eq!(result.moments.centroid.shape().dims(), [3]);
    assert_eq!(result.moments.covariance.shape().dims(), [3, 3]);
    assert_eq!(result.moments.second_moment.shape().dims(), [3, 3]);
    for (tensor, minimum) in [(&result.bounds.min, true), (&result.bounds.max, false)] {
        let expected: Vec<_> = (0..3)
            .map(|a| {
                points
                    .iter()
                    .map(|p| p[a])
                    .reduce(|a, b| if minimum { a.min(b) } else { a.max(b) })
                    .unwrap()
            })
            .collect();
        assert_eq!(b.read_f32(tensor)?, expected);
    }
    let (mean, raw, covariance) = stats(points);
    let actual_mean = b.read_f32(&result.moments.centroid)?;
    // Mean conditioning is E[|x|]/|E[x]|. An almost symmetric cloud cannot
    // promise relative accuracy in its small signed mean: our anisotropic
    // fixture has condition ~104, while an observed ~5.6e-6 relative mean
    // error is only ~5.4e-8 of E[|x|]. Keep the same 3e-6 coefficient, using
    // the independent f64 sum scale. The all-positive sparse fixture still
    // has E[|x|]==E[x], so this retains the gate that caught midpoint-mean
    // reconstruction errors of 43/91 output ULPs on WGSL/MLX.
    for axis in 0..3 {
        let mean_abs =
            points.iter().map(|p| f64::from(p[axis]).abs()).sum::<f64>() / points.len() as f64;
        close(
            &actual_mean[axis..axis + 1],
            &mean[axis..axis + 1],
            3e-6,
            mean_abs,
        );
    }
    // Cross entries near zero use their Cauchy-Schwarz scale. This retains a
    // strict relative check for tiny anisotropic diagonal entries.
    let actual_raw = b.read_f32(&result.moments.second_moment)?;
    let actual_cov = b.read_f32(&result.moments.covariance)?;
    for i in 0..3 {
        for j in 0..3 {
            let raw_scale = (raw[3 * i + i] * raw[3 * j + j]).abs().sqrt();
            let cov_scale = (covariance[3 * i + i] * covariance[3 * j + j]).abs().sqrt();
            let underflow_floor = f64::from(f32::MIN_POSITIVE) / 3e-4;
            close(
                &actual_raw[3 * i + j..3 * i + j + 1],
                &raw[3 * i + j..3 * i + j + 1],
                3e-4,
                raw_scale.max(underflow_floor),
            );
            close(
                &actual_cov[3 * i + j..3 * i + j + 1],
                &covariance[3 * i + j..3 * i + j + 1],
                3e-4,
                cov_scale.max(underflow_floor),
            );
            assert_eq!(
                actual_cov[3 * i + j].to_bits(),
                actual_cov[3 * j + i].to_bits(),
                "covariance symmetry"
            );
        }
    }
    // Covariance should be PSD to its rounding scale, including a planar null
    // space. This is independent of any production eigensolver.
    let covariance_scale = covariance.iter().map(|v| v.abs()).fold(0., f64::max);
    for vector in [
        [1., 0., 0.],
        [0., 1., 0.],
        [0., 0., 1.],
        [1., -2., 1.],
        [-1., 1., 2.],
    ] {
        let mut quadratic = 0.;
        for i in 0..3 {
            for j in 0..3 {
                quadratic += vector[i] * f64::from(actual_cov[3 * i + j]) * vector[j];
            }
        }
        assert!(
            quadratic >= -1e-3 * covariance_scale,
            "negative quadratic {quadratic}"
        );
    }
    Ok(())
}

pub fn run<B: TensorReduceBackend + TensorStatsBackend>(b: &B) -> Result
where
    B::Error: 'static,
{
    let math = TensorMath::new(b);
    let matrix_values = [1f32, 0.25, -0.5, -0.25, 2., 0.5, 0.5, -1., 0.25];
    // The matrix itself is also strided; this upload stores its transpose.
    let physical_matrix: Vec<_> = (0..3)
        .flat_map(|r| (0..3).map(move |c| matrix_values[c * 3 + r]))
        .collect();
    let matrix = b.upload_f32(shape(&[3, 3]), &physical_matrix)?;
    let matrix = b.permute(&matrix, &[1, 0])?;
    let translation_values = [2f32, -3., 0.5];
    let translation = b.upload_f32(shape(&[3]), &translation_values)?;
    for count in [0, 1, 3, 257, 513] {
        let points: Vec<_> = (0..count)
            .map(|i| {
                [
                    i as f32 * 0.125 - 5.,
                    ((i * 7) % 23) as f32 * 0.25,
                    ((i * 3) % 11) as f32 - 2.,
                ]
            })
            .collect();
        let source = input(b, &points, true)?;
        let transformed = math.transform(&source, &matrix, &translation, MatmulPrecision::F32)?;
        let expected: Vec<_> = points
            .iter()
            .flat_map(|p| {
                (0..3).map(move |r| {
                    (0..3)
                        .map(|c| f64::from(matrix_values[3 * r + c]) * f64::from(p[c]))
                        .sum::<f64>()
                        + f64::from(translation_values[r])
                })
            })
            .collect();
        close(&b.read_f32(&transformed)?, &expected, 2e-5, 1.);
        let paired: Vec<_> = points
            .iter()
            .map(|p| [p[0] + 0.5, p[1] - 0.25, p[2] + 1.])
            .collect();
        let target = input(b, &paired, false)?;
        let distances = math.squared_distance_pairs(&source, &target)?;
        assert_eq!(distances.shape().dims(), [count]);
        let distances_ref: Vec<_> = points
            .iter()
            .zip(&paired)
            .map(|(p, q)| {
                (0..3)
                    .map(|a| (f64::from(p[a]) - f64::from(q[a])).powi(2))
                    .sum()
            })
            .collect();
        close(&b.read_f32(&distances)?, &distances_ref, 1e-6, 1.);
        let sum = math.squared_distance_pair_sum(&source, &target)?;
        assert_eq!(sum.shape().dims(), []);
        close(&b.read_f32(&sum)?, &[distances_ref.iter().sum()], 1e-6, 1.);
        if count == 0 {
            assert!(math.bounds(&source).is_err());
            assert!(math.moments(&source).is_err());
            assert!(math.point_cloud_stats(&source).is_err());
        } else {
            check_stats(b, &points, count % 2 == 1)?;
        }
    }
    // Scaled mean avoids overflowing raw sums, and centered covariance retains
    // variation that survives f32 upload despite a large common translation.
    for count in [3, 257, 4097] {
        let shifted: Vec<_> = (0..count)
            .map(|i| {
                let x = ((i % 17) as f32 - 8.) * 8.;
                let y = ((i % 13) as f32 - 6.) * 16.;
                [
                    100_000_000. + x,
                    -200_000_000. + y,
                    50_000_000. + x * 0.5 - y * 0.25,
                ]
            })
            .collect();
        check_stats(b, &shifted, true)?;
    }
    // The f32 center is not the exact mean: residual-mean correction matters.
    check_stats(
        b,
        &[
            [1_000_000.; 3],
            [(1_000_000. + 0.125); 3],
            [(1_000_000. + 0.125); 3],
        ],
        false,
    )?;
    check_stats(b, &[[1e18, -1e18, 0.]; 257], true)?;
    let anisotropic: Vec<_> = (0..513)
        .map(|i| {
            let x = ((i % 17) as f32 - 8.) / 8.;
            [x * 1e18, x * 1e-18, -x * 0.25]
        })
        .collect();
    check_stats(b, &anisotropic, false)?;
    // A raw square would overflow, although E[x*x] and covariance fit f32.
    let mut sparse = vec![[0.; 3]; 513];
    sparse[256] = [1e20, -1e20, 0.5e20];
    check_stats(b, &sparse, true)?;
    // Bounds alone cover the full finite f32 range without forming moments.
    let extreme = input(b, &[[-f32::MAX, 0., -1.], [f32::MAX, 0., 1.]], false)?;
    let bounds = math.bounds(&extreme)?;
    assert_eq!(b.read_f32(&bounds.min)?, [-f32::MAX, 0., -1.]);
    assert_eq!(b.read_f32(&bounds.max)?, [f32::MAX, 0., 1.]);
    // Broadcast views and dependent resident composition use the same recipes.
    let one = input(b, &[[1., -2., 3.]], false)?;
    let repeated = b.broadcast_to(&one, shape(&[257, 3]))?;
    let moments = math.moments(&repeated)?;
    assert_eq!(b.read_f32(&moments.covariance)?, [0.; 9]);
    let transformed = math.transform(&repeated, &matrix, &translation, MatmulPrecision::F32)?;
    let stats = math.point_cloud_stats(&transformed)?;
    assert_eq!(b.read_f32(&stats.moments.covariance)?, [0.; 9]);
    // Shape errors are domain validation, including errors on empty inputs.
    let malformed = b.upload_f32(shape(&[0, 2]), &[])?;
    assert!(
        math.transform(&malformed, &matrix, &translation, MatmulPrecision::F32)
            .is_err()
    );
    assert!(math.bounds(&malformed).is_err());
    let wrong_translation = b.upload_f32(shape(&[1, 3]), &[0.; 3])?;
    assert!(
        math.transform(&one, &matrix, &wrong_translation, MatmulPrecision::F32)
            .is_err()
    );
    let empty = input(b, &[], false)?;
    assert!(math.squared_distance_pairs(&one, &empty).is_err());
    assert!(math.squared_distance_pair_sum(&empty, &one).is_err());
    Ok(())
}

#[test]
fn independent_reference_retains_shifted_covariance_and_finite_sparse_moments() {
    let (_, _, covariance) = stats(&[
        [1_000_000.; 3],
        [(1_000_000. + 0.125); 3],
        [(1_000_000. + 0.125); 3],
    ]);
    for value in covariance {
        assert!((value - 0.125f64.powi(2) * 2. / 9.).abs() < 1e-15);
    }
    let mut sparse = vec![[0.; 3]; 513];
    sparse[256] = [1e20, -1e20, 0.5e20];
    let (_, raw, covariance) = stats(&sparse);
    assert!((1e20f32 * 1e20f32).is_infinite());
    assert!(
        raw.into_iter()
            .chain(covariance)
            .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
    );
}
