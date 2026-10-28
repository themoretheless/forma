//! Adjacent binary64 values for outward rounded enclosures.
/// Returns the adjacent representable value toward negative infinity.
pub fn next_down(x: f64) -> f64 {
    if x == f64::NEG_INFINITY || x.is_nan() {
        x
    } else if x == 0. {
        -f64::from_bits(1)
    } else {
        f64::from_bits(x.to_bits().wrapping_add(if x < 0. { 1 } else { u64::MAX }))
    }
}
/// Returns the adjacent representable value toward positive infinity.
pub fn next_up(x: f64) -> f64 {
    if x == f64::INFINITY || x.is_nan() {
        x
    } else if x == 0. {
        f64::from_bits(1)
    } else {
        f64::from_bits(x.to_bits().wrapping_add(if x < 0. { u64::MAX } else { 1 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directed_neighbors_cover_zero_underflow_overflow_and_special_values() {
        for x in [
            0.,
            -0.,
            f64::from_bits(1),
            -f64::from_bits(1),
            f64::MIN_POSITIVE,
            -f64::MIN_POSITIVE,
            1.,
            -1.,
            f64::MAX,
            -f64::MAX,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(next_down(x).to_bits(), x.next_down().to_bits());
            assert_eq!(next_up(x).to_bits(), x.next_up().to_bits());
        }
        for bits in [0x7ff8000000000001, 0xfff8000000000012] {
            let x = f64::from_bits(bits);
            assert_eq!(next_up(x).to_bits(), bits);
            assert_eq!(next_down(x).to_bits(), bits);
        }
    }
}
