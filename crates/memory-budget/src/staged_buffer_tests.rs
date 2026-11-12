use super::*;

#[test]
fn staged_growth_charges_all_ancestors_and_overflow_releases_credit() {
    let root = MemoryBudget::new(32);
    let child = root.child(24);
    let leaf = child.child(16);
    let other_owner = root.try_reserve(16).unwrap();
    let reserved = leaf.try_reserve(4).unwrap();
    let buffer = reserved.try_buffer(4, 11u32).unwrap().freeze();
    assert_eq!(&*buffer, &[11; 4]);
    assert_eq!(root.used(), 32);
    assert_eq!(child.used(), 16);
    assert_eq!(leaf.used(), 16);
    drop(buffer);
    assert_eq!(root.used(), 16);
    assert_eq!(child.used(), 0);
    assert_eq!(leaf.used(), 0);

    let reserved = leaf.try_reserve(4).unwrap();
    assert!(matches!(
        reserved.try_buffer(usize::MAX, 0u16),
        Err(BufferError::Overflow)
    ));
    assert_eq!(root.used(), 16);
    assert_eq!(child.used(), 0);
    assert_eq!(leaf.used(), 0);
    drop(other_owner);
    assert_eq!(root.used(), 0);
}

#[test]
fn staged_buffer_reuses_credit_and_preserves_child_and_last_owner_accounting() {
    let root = MemoryBudget::new(32);
    let child = root.child(16);
    let reservation = child.try_reserve(16).unwrap();
    let mut buffer = reservation.try_buffer(4, 7u32).unwrap();
    assert_eq!(root.used(), 16);
    assert_eq!(child.used(), 16);
    assert_eq!(root.peak(), 16);
    buffer[1] = 9;
    let buffer = buffer.freeze();
    let clone = buffer.clone();
    drop(buffer);
    assert_eq!(&*clone, &[7, 9, 7, 7]);
    assert_eq!(root.used(), 16);
    drop(clone);
    assert_eq!(root.used(), 0);
    assert_eq!(child.used(), 0);

    let reservation = child.try_reserve(16).unwrap();
    assert!(matches!(
        reservation.try_buffer(5, 0u32),
        Err(BufferError::Capacity { .. })
    ));
    assert_eq!(root.used(), 0);
    assert_eq!(child.used(), 0);
    let reservation = child.try_reserve(16).unwrap();
    let smaller = reservation.try_buffer(1, 1u8).unwrap();
    assert_eq!(root.used(), 16);
    drop(smaller);
    assert_eq!(root.used(), 0);
}
