//! Numerical consistency checks for transported geometric witnesses.
//! These reject contradictory reports; they do not establish geometric proofs.
type Points = [[f64; 3]; 2];
type Boxes = [[[f64; 2]; 3]; 2];
fn finite(points: Points, bounds: Boxes, lo: f64, hi: f64) -> bool {
    points.iter().flatten().all(|x| x.is_finite())
        && bounds
            .iter()
            .flatten()
            .all(|x| x[0].is_finite() && x[1].is_finite() && x[0] <= x[1])
        && lo.is_finite()
        && hi.is_finite()
        && 0. <= lo
        && lo <= hi
}
fn length(p: [f64; 3]) -> f64 {
    p[0].hypot(p[1]).hypot(p[2])
}
pub fn distance_consistent(
    points: Points,
    bounds: Boxes,
    lo: f64,
    hi: f64,
    strict_upper: bool,
) -> bool {
    if !finite(points, bounds, lo, hi) {
        return false;
    }
    let gap = length(std::array::from_fn(|k| points[0][k] - points[1][k]));
    if strict_upper {
        let scale = points
            .iter()
            .flatten()
            .chain(bounds.iter().flatten().flatten())
            .map(|x| x.abs())
            .fold(hi.max(1.), f64::max);
        let slack = f64::EPSILON * scale * 32.;
        if (0..2).any(|i| {
            (0..3).any(|k| {
                points[i][k] < bounds[i][k][0] - slack || points[i][k] > bounds[i][k][1] + slack
            })
        }) {
            return false;
        }
        let upper = length(std::array::from_fn(|k| {
            (bounds[0][k][0] - bounds[1][k][1])
                .abs()
                .max((bounds[0][k][1] - bounds[1][k][0]).abs())
        }));
        gap.is_finite()
            && upper.is_finite()
            && gap >= lo - slack
            && gap <= hi + slack
            && upper <= hi + slack
    } else {
        let uncertainty = (0..2)
            .flat_map(|i| {
                (0..3).map(move |k| {
                    (points[i][k] - bounds[i][k][0]).max(bounds[i][k][1] - points[i][k])
                })
            })
            .sum::<f64>();
        let scale = points
            .iter()
            .flatten()
            .map(|x| x.abs())
            .fold(gap.max(1.), f64::max);
        let slack = uncertainty + f64::EPSILON * scale * 16.;
        gap.is_finite() && slack.is_finite() && gap >= lo - slack && gap <= hi + slack
    }
}
pub fn material_consistent(bounds: Boxes, lo: f64, hi: f64) -> bool {
    if !finite([[0.; 3]; 2], bounds, lo, hi) {
        return false;
    }
    let lower = length(std::array::from_fn(|k| {
        0f64.max(bounds[0][k][0] - bounds[1][k][1])
            .max(bounds[1][k][0] - bounds[0][k][1])
    }));
    let upper = length(std::array::from_fn(|k| {
        (bounds[0][k][0] - bounds[1][k][1])
            .abs()
            .max((bounds[0][k][1] - bounds[1][k][0]).abs())
    }));
    let scale = bounds
        .iter()
        .flatten()
        .flatten()
        .map(|x| x.abs())
        .fold(upper.max(1.), f64::max);
    let slack = f64::EPSILON * scale * 32.;
    lower.is_finite() && upper.is_finite() && lo <= lower + slack && hi >= upper - slack
}
/// Necessary transported dominance consistency, not a new quotient proof.
pub fn quotient_consistent([a, b, e, c]: [f64; 4], margin: f64) -> bool {
    [a, b, e, c, margin].iter().all(|x| x.is_finite())
        && a > 0.
        && b > 0.
        && e >= 0.
        && c >= 0.
        && margin > 0.
        && margin <= a * b - c * e
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_forged_distance_and_enclosure() {
        let p = [[0., 0., 0.], [3., 4., 0.]];
        let b = p.map(|p| p.map(|x| [x, x]));
        assert!(distance_consistent(p, b, 5., 5., true));
        assert!(!distance_consistent(p, b, 4., 4., true));
        assert!(material_consistent(b, 5., 5.));
        assert!(!material_consistent(b, 6., 6.));
        assert!(!material_consistent(b, f64::NAN, 5.));
    }
}
