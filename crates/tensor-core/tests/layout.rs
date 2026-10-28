use tensor_core::{Layout, Shape, TensorError, matmul_shape};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn scalars_empty_shapes_and_overflows_are_distinct() {
    assert_eq!(shape(&[]).numel(), 1);
    assert_eq!(shape(&[2, 0, 3]).numel(), 0);
    assert_eq!(shape(&[usize::MAX, usize::MAX, 0]).numel(), 0);
    assert!(matches!(
        Shape::new(vec![usize::MAX, 2]),
        Err(TensorError::ShapeOverflow)
    ));
    assert!(Layout::contiguous(shape(&[usize::MAX, 0, usize::MAX])).is_ok());
    assert!(
        Layout::contiguous(shape(&[]))
            .unwrap()
            .validate_storage_len(0)
            .is_err()
    );
    assert!(
        Layout::contiguous(shape(&[0]))
            .unwrap()
            .validate_storage_len(0)
            .is_ok()
    );
}

#[test]
fn broadcasting_handles_zero_axes_and_leading_rank_expansion() {
    assert_eq!(
        shape(&[2, 1, 4]).broadcast(&shape(&[3, 1])).unwrap(),
        shape(&[2, 3, 4])
    );
    assert_eq!(
        shape(&[0, 3]).broadcast(&shape(&[1, 3])).unwrap(),
        shape(&[0, 3])
    );
    assert_eq!(
        shape(&[]).broadcast(&shape(&[0, 3])).unwrap(),
        shape(&[0, 3])
    );
    assert!(shape(&[0]).broadcast(&shape(&[2])).is_err());
    assert!(shape(&[2, 3]).broadcast(&shape(&[2])).is_err());
    let layout = Layout::contiguous(shape(&[2, 1]))
        .unwrap()
        .broadcast_to(shape(&[3, 2, 4]))
        .unwrap();
    assert_eq!(layout.strides(), &[0, 1, 0]);
    layout.validate_storage_len(2).unwrap();
    let expected: Vec<usize> = (0..3).flat_map(|_| [0, 0, 0, 0, 1, 1, 1, 1]).collect();
    assert_eq!(
        (0..24)
            .map(|i| layout.element_offset(i).unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(!layout.is_contiguous());
    assert!(layout.reshape(shape(&[24])).is_err());
}

#[test]
fn views_preserve_offsets_and_reject_out_of_bounds_storage() {
    let layout = Layout::new(shape(&[2, 3]), vec![5, 1], 2).unwrap();
    assert_eq!(layout.required_storage_len().unwrap(), 10);
    assert!(layout.validate_storage_len(9).is_err());
    layout.validate_storage_len(10).unwrap();
    let transposed = layout.permute(&[1, 0]).unwrap();
    assert_eq!(transposed.shape(), &shape(&[3, 2]));
    assert_eq!(
        (0..6)
            .map(|i| transposed.element_offset(i).unwrap())
            .collect::<Vec<_>>(),
        [2, 7, 3, 8, 4, 9]
    );
    assert!(transposed.element_offset(6).is_err());
    let row = layout.narrow(0, 1, 1).unwrap();
    assert_eq!(row.offset(), 7);
    assert!(row.is_contiguous()); // The singleton leading stride is irrelevant.
    let reshaped = row.reshape(shape(&[3])).unwrap();
    assert_eq!(reshaped.offset(), 7);
    assert_eq!(reshaped.element_offset(2).unwrap(), 9);
    assert!(layout.narrow(1, 3, 1).is_err());
    assert!(layout.narrow(1, usize::MAX, 0).is_err());
    assert!(Layout::new(shape(&[2]), vec![usize::MAX], 0).is_err());
    assert!(Layout::new(shape(&[1]), vec![0], usize::MAX).is_err());
    assert!(Layout::new(shape(&[2]), vec![], 0).is_err());
}

#[test]
fn empty_slices_of_strided_views_keep_valid_storage_bounds() {
    let strided = Layout::new(shape(&[3]), vec![2], 1).unwrap();
    strided.validate_storage_len(6).unwrap();
    let empty = strided.narrow(0, 3, 0).unwrap();
    assert_eq!(empty.shape().numel(), 0);
    empty.validate_storage_len(6).unwrap();
    assert!(empty.element_offset(0).is_err());

    // No offset arithmetic is needed when a different axis is already empty.
    let empty = Layout::new(shape(&[0, 3]), vec![usize::MAX, usize::MAX], 1).unwrap();
    let narrowed = empty.narrow(1, 2, 1).unwrap().narrow(1, 1, 0).unwrap();
    narrowed.validate_storage_len(1).unwrap();
    assert!(empty.narrow(1, 4, 0).is_err());
}

#[test]
fn reduction_axes_are_unique_and_keep_dimensions_when_requested() {
    let s = shape(&[2, 0, 4]);
    assert_eq!(s.reduce(&[1], false).unwrap(), shape(&[2, 4]));
    assert_eq!(s.reduce(&[2, 0], true).unwrap(), shape(&[1, 0, 1]));
    assert_eq!(s.reduce(&[], false).unwrap(), s);
    assert_eq!(s.reduce(&[0, 1, 2], false).unwrap(), shape(&[]));
    assert!(matches!(
        s.reduce(&[1, 1], false),
        Err(TensorError::DuplicateAxis { axis: 1 })
    ));
    assert!(matches!(
        s.reduce(&[3], false),
        Err(TensorError::AxisOutOfBounds { .. })
    ));
    assert!(s.permute(&[0, 0, 2]).is_err());
    assert!(s.permute(&[0, 1]).is_err());
    assert_eq!(shape(&[]).permute(&[]).unwrap(), shape(&[]));
}

#[test]
fn batched_matrix_shapes_broadcast_without_broadcasting_contraction() {
    assert_eq!(
        matmul_shape(
            &shape(&[usize::MAX, 1, 0, 0]),
            &shape(&[1, usize::MAX, 0, 0])
        )
        .unwrap(),
        shape(&[usize::MAX, usize::MAX, 0, 0])
    );
    assert_eq!(
        matmul_shape(&shape(&[2, 1, 3, 4]), &shape(&[5, 4, 6])).unwrap(),
        shape(&[2, 5, 3, 6])
    );
    assert_eq!(
        matmul_shape(&shape(&[2, 0]), &shape(&[0, 3])).unwrap(),
        shape(&[2, 3])
    );
    assert_eq!(
        matmul_shape(&shape(&[0, 2, 4]), &shape(&[1, 4, 3])).unwrap(),
        shape(&[0, 2, 3])
    );
    assert!(matmul_shape(&shape(&[2, 1]), &shape(&[4, 3])).is_err());
    assert!(matmul_shape(&shape(&[2, 3, 4]), &shape(&[5, 4, 3])).is_err());
    assert_eq!(
        matmul_shape(&shape(&[4]), &shape(&[4, 3])).unwrap(),
        shape(&[3])
    );
    assert!(matmul_shape(&shape(&[]), &shape(&[1, 1])).is_err());
}

#[test]
fn axis_permutations_and_broadcasts_match_coordinate_reference() {
    let original = Layout::contiguous(shape(&[2, 3, 4])).unwrap();
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let view = original.permute(&order).unwrap();
        let dims = view.shape().dims();
        for x in 0..dims[0] {
            for y in 0..dims[1] {
                for z in 0..dims[2] {
                    let mut source = [0; 3];
                    for (dest, value) in [x, y, z].into_iter().enumerate() {
                        source[order[dest]] = value;
                    }
                    let expected = source[0] * 12 + source[1] * 4 + source[2];
                    let flat = x * dims[1] * dims[2] + y * dims[2] + z;
                    assert_eq!(view.element_offset(flat).unwrap(), expected);
                }
            }
        }
    }
}
