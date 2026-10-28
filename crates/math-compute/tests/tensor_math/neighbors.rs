use math_compute::tensor::{NeighborOptions, TensorMath};
use tensor_core::{
    BinaryOp, HasShape, Shape, TensorBackend, TensorEvalBackend, TensorReduceBackend,
    TensorScatterBackend,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn reference(queries: &[[f32; 3]], targets: &[[f32; 3]], k: usize) -> (Vec<u32>, Vec<f32>) {
    let mut indices = Vec::new();
    let mut distances = Vec::new();
    for query in queries {
        let mut candidates = targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let d = (0..3)
                    .map(|axis| (f64::from(query[axis]) - f64::from(target[axis])).powi(2))
                    .sum::<f64>();
                (d, index as u32)
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for rank in 0..k {
            let (distance, index) = candidates
                .get(rank)
                .map_or((f32::MAX, u32::MAX), |&(d, i)| (d as f32, i));
            indices.push(index);
            distances.push(distance);
        }
    }
    (indices, distances)
}

fn assert_neighbors<B>(
    backend: &B,
    queries: &B::Tensor,
    targets: &B::Tensor,
    logical_queries: &[[f32; 3]],
    logical_targets: &[[f32; 3]],
    k: usize,
    options: NeighborOptions,
) -> TestResult
where
    B: TensorReduceBackend + TensorScatterBackend + TensorEvalBackend,
    B::Error: 'static,
{
    let got = TensorMath::new(backend).nearest(queries, targets, k, options)?;
    assert_eq!(got.indices.shape().dims(), [logical_queries.len(), k]);
    assert_eq!(
        got.squared_distances.shape().dims(),
        [logical_queries.len(), k]
    );
    let (expected_i, expected_d) = reference(logical_queries, logical_targets, k);
    assert_eq!(
        backend.read_u32(&got.indices)?,
        expected_i,
        "neighbor indices, k={k}"
    );
    assert_eq!(
        backend.read_f32(&got.squared_distances)?,
        expected_d,
        "neighbor distances, k={k}"
    );
    Ok(())
}

fn transposed<B: TensorBackend>(backend: &B, points: &[[f32; 3]]) -> Result<B::Tensor, B::Error> {
    let physical = (0..3)
        .flat_map(|axis| points.iter().map(move |point| point[axis]))
        .collect::<Vec<_>>();
    let source = backend.upload_f32(Shape::new(vec![3, points.len()]).unwrap(), &physical)?;
    backend.permute(&source, &[1, 0])
}

pub fn run<B>(backend: &B) -> TestResult
where
    B: TensorReduceBackend + TensorScatterBackend + TensorEvalBackend,
    B::Error: 'static,
{
    let math = TensorMath::new(backend);
    let options = NeighborOptions {
        query_tile: 2,
        target_tile: 3,
        ..NeighborOptions::default()
    };
    let query_values = [
        [0., 0., 0.],
        [1., 0., 0.],
        [3., 1., 0.],
        [-1., 2., 0.],
        [0., 0., 0.],
    ];
    let target_values = [
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [1., 0., 0.],
        [4., 0., 0.],
        [0., 0., 0.],
        [0., -1., 0.],
    ];
    let queries = transposed(backend, &query_values)?;
    let targets = transposed(backend, &target_values)?;
    for k in [1, 2, 4, 9] {
        assert_neighbors(
            backend,
            &queries,
            &targets,
            &query_values,
            &target_values,
            k,
            options,
        )?;
    }
    // Tile boundaries must not change tie ordering or returned original indices.
    assert_neighbors(
        backend,
        &queries,
        &targets,
        &query_values,
        &target_values,
        4,
        NeighborOptions::default(),
    )?;

    // A large common translation exposes cancellation in a Gram-matrix shortcut.
    let translated_q = [
        [1_000_000., 1_000_000., 1_000_000.],
        [1_000_002., 1_000_000., 1_000_000.],
    ];
    let translated_t = [
        [1_000_001., 1_000_000., 1_000_000.],
        [999_999., 1_000_000., 1_000_000.],
        [1_000_003., 1_000_000., 1_000_000.],
    ];
    let tq = math.upload_points(&translated_q)?;
    let tt = math.upload_points(&translated_t)?;
    assert_neighbors(backend, &tq, &tt, &translated_q, &translated_t, 4, options)?;

    // Logical repeated targets with zero strides still have distinct IDs.
    let one_target = math.upload_points(&[[2., 0., 0.]])?;
    let repeated = backend.broadcast_to(&one_target, Shape::new(vec![5, 3])?)?;
    let one_query = math.upload_points(&[[0., 0., 0.]])?;
    assert_neighbors(
        backend,
        &one_query,
        &repeated,
        &[[0., 0., 0.]],
        &[[2., 0., 0.]; 5],
        7,
        options,
    )?;
    assert_neighbors(
        backend,
        &one_query,
        &one_target,
        &[[0., 0., 0.]],
        &[[2., 0., 0.]],
        37,
        options,
    )?;

    // Consume resident IDs before any readback, and pass a computed point tensor.
    let shift = backend.upload_f32(Shape::new(vec![])?, &[1.])?;
    let shifted = backend.binary(BinaryOp::Add, &one_query, &shift)?;
    let chosen = math.nearest(&shifted, &targets, 2, options)?;
    let gathered = backend.gather_f32(&targets, &chosen.indices, 0)?;
    backend.evaluate(&[&gathered.values], &[&gathered.invalid_count])?;
    assert_eq!(gathered.values.shape().dims(), [1, 2, 3]);
    let (ids, _) = reference(&[[1., 1., 1.]], &target_values, 2);
    let expected = ids
        .into_iter()
        .flat_map(|id| target_values[id as usize])
        .collect::<Vec<_>>();
    assert_eq!(backend.read_f32(&gathered.values)?, expected);
    assert_eq!(backend.read_u32(&gathered.invalid_count)?, [0]);

    let empty = math.upload_points(&[])?;
    assert_neighbors(backend, &empty, &targets, &[], &target_values, 4, options)?;
    assert_neighbors(backend, &queries, &empty, &query_values, &[], 4, options)?;
    assert!(math.nearest(&queries, &targets, 0, options).is_err());
    for changed in [
        NeighborOptions {
            query_tile: 0,
            ..options
        },
        NeighborOptions {
            target_tile: 0,
            ..options
        },
        NeighborOptions {
            max_work_elements: 1,
            ..options
        },
        NeighborOptions {
            max_workspace_bytes: 1,
            ..options
        },
        NeighborOptions {
            max_output_bytes: 1,
            ..options
        },
    ] {
        assert!(math.nearest(&queries, &targets, 4, changed).is_err());
    }
    let wrong = backend.upload_f32(Shape::new(vec![3])?, &[1., 2., 3.])?;
    assert!(math.nearest(&wrong, &targets, 1, options).is_err());

    let cq = math.upload_points(&[[0., 0., 0.], [2., 0., 0.]])?;
    let ct = math.upload_points(&[[0., 0., 0.]])?;
    let summary = math.directed_chamfer(&cq, &ct, options)?;
    assert_eq!(summary.samples, 2);
    for tensor in [
        &summary.mean_squared_distance,
        &summary.rms_distance,
        &summary.max_squared_distance,
    ] {
        assert!(tensor.shape().dims().is_empty());
    }
    assert_eq!(backend.read_f32(&summary.mean_squared_distance)?, [2.]);
    assert_eq!(backend.read_f32(&summary.max_squared_distance)?, [4.]);
    assert!((backend.read_f32(&summary.rms_distance)?[0] - 2_f32.sqrt()).abs() < 1e-6);
    assert!(math.directed_chamfer(&empty, &targets, options).is_err());
    assert!(math.directed_chamfer(&queries, &empty, options).is_err());
    Ok(())
}
