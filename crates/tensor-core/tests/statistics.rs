use tensor_core::{Shape, TensorError, normalization_shape, statistics_shape, validate_epsilon};

#[test]
fn distribution_and_statistics_shapes_distinguish_empty_groups() {
    let input = Shape::new(vec![2, 0, 3]).unwrap();
    assert_eq!(normalization_shape(&input, &[1]).unwrap(), input);
    assert_eq!(
        statistics_shape(&input, &[1], false),
        Err(TensorError::EmptyReduction)
    );
    assert_eq!(
        statistics_shape(&input, &[1], true),
        Err(TensorError::EmptyReduction)
    );
    assert_eq!(
        statistics_shape(&input, &[0], false).unwrap().dims(),
        &[0, 3]
    );
    assert_eq!(statistics_shape(&input, &[], false).unwrap(), input);
    assert!(normalization_shape(&input, &[1, 1]).is_err());
    assert!(statistics_shape(&input, &[3], true).is_err());
    let scalar = Shape::new(vec![]).unwrap();
    assert_eq!(statistics_shape(&scalar, &[], false).unwrap(), scalar);
    assert!(normalization_shape(&scalar, &[0]).is_err());
}

#[test]
fn normalization_epsilon_accepts_every_finite_positive_scale() {
    for value in [f32::from_bits(1), f32::MIN_POSITIVE, 1e-5, 1., f32::MAX] {
        validate_epsilon(value).unwrap();
    }
    for value in [0., -0., -1e-5, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        assert_eq!(validate_epsilon(value), Err(TensorError::InvalidEpsilon));
    }
}
