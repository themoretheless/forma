//! The page buffer never hands out a slice longer than what it allocated.
use crate::AsdPageBuffer;

#[test]
fn capacity_is_the_length_rounded_up_to_a_page_and_starts_zeroed() {
    let page = AsdPageBuffer::native_page_size();
    for (len, capacity) in [(0, page), (1, page), (page, page), (page + 1, 2 * page)] {
        let buffer = AsdPageBuffer::new_zeroed(len);
        assert_eq!(buffer.len(), len);
        assert_eq!(buffer.aligned_capacity(), capacity);
        assert!(buffer.as_slice().iter().all(|byte| *byte == 0));
    }
    let copied = AsdPageBuffer::from_slice(&[1, 2, 3]);
    assert_eq!(copied.as_slice(), [1, 2, 3]);
}

/// Rounding these lengths up to a page wraps `usize`. Wrapped, the allocation would be a page
/// or two while `as_slice` still claimed the requested length.
#[test]
fn a_length_whose_page_rounding_overflows_is_refused() {
    let page = AsdPageBuffer::native_page_size();
    for len in [usize::MAX, usize::MAX - page + 2, isize::MAX as usize] {
        let refused = std::panic::catch_unwind(|| AsdPageBuffer::new_zeroed(len).len());
        let message = refused.expect_err("an impossible capacity was accepted");
        let message = message
            .downcast_ref::<String>()
            .expect("the refusal carries a message");
        assert!(message.contains("capacity overflow"), "{message}");
    }
}
