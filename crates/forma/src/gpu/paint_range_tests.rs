use super::changed_paint_range;
#[test]
fn partial_updates_reconstruct_exact_bits_and_handle_resizes() {
    for length in [0, 1, 8, 256] {
        let old = vec![0f32; length];
        assert_eq!(changed_paint_range(&old, &old), None);
        for index in 0..length {
            let mut next = old.clone();
            next[index] = 1.;
            let range = changed_paint_range(&old, &next).unwrap();
            assert_eq!(range, index..index + 1);
            let mut gpu = old.clone();
            gpu[range.clone()].copy_from_slice(&next[range]);
            assert_eq!(gpu, next);
        }
    }
    assert_eq!(changed_paint_range(&[0.; 8], &[1.; 4]), Some(0..4));
    assert_eq!(changed_paint_range(&[0.; 4], &[0.; 8]), Some(0..8));
    assert_eq!(changed_paint_range(&[0.; 4], &[1., 0., 0., 2.]), Some(0..4));
    assert_eq!(changed_paint_range(&[0.], &[-0.]), Some(0..1));
    assert_eq!(changed_paint_range(&[f32::NAN], &[f32::NAN]), None);
}
