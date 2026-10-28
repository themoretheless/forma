//! Homogeneous point evaluation and finite triangle-domain validation.
//! Matrices are row-major; callers retain topology and choose error presentation.
use crate::affine::AffineMatrix;
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectiveError {
    NonFiniteInput,
    Horizon,
    NonFiniteOutput,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPoint {
    pub position: [f64; 3],
    pub w: f64,
}
pub fn point(matrix: AffineMatrix, position: [f64; 3]) -> Result<ProjectedPoint, ProjectiveError> {
    if !matrix
        .iter()
        .flatten()
        .chain(position.iter())
        .all(|v| v.is_finite())
    {
        return Err(ProjectiveError::NonFiniteInput);
    }
    let homogeneous: [f64; 4] = std::array::from_fn(|row| {
        matrix[row][3]
            + (0..3)
                .map(|column| matrix[row][column] * position[column])
                .sum::<f64>()
    });
    if !homogeneous.iter().all(|v| v.is_finite()) {
        return Err(ProjectiveError::NonFiniteOutput);
    }
    let w = homogeneous[3];
    if w == 0. {
        return Err(ProjectiveError::Horizon);
    }
    let position = std::array::from_fn(|axis| homogeneous[axis] / w);
    if !position.iter().all(|v| v.is_finite()) {
        return Err(ProjectiveError::NonFiniteOutput);
    }
    Ok(ProjectedPoint { position, w })
}
/// W is affine on a triangle. Strictly equal signs at its vertices prove that
/// the whole triangle avoids the horizon; opposite signs require refusal.
pub fn triangle_domain(vertices: [ProjectedPoint; 3]) -> Result<(), ProjectiveError> {
    if vertices
        .iter()
        .any(|v| !v.w.is_finite() || !v.position.iter().all(|p| p.is_finite()))
    {
        return Err(ProjectiveError::NonFiniteInput);
    }
    if vertices
        .iter()
        .any(|v| v.w == 0. || v.w.is_sign_negative() != vertices[0].w.is_sign_negative())
    {
        return Err(ProjectiveError::Horizon);
    }
    Ok(())
}
/// Sign of the full homogeneous determinant. Positive row scaling avoids
/// overflow; zero or non-finite elimination refuses an unresolved orientation.
pub fn orientation(mut matrix: AffineMatrix) -> Option<bool> {
    for row in &mut matrix {
        let scale = row.iter().map(|v| v.abs()).fold(0., f64::max);
        if !scale.is_finite() || scale == 0. || !row.iter().all(|v| v.is_finite()) {
            return None;
        }
        for value in row {
            *value /= scale;
        }
    }
    let mut negative = false;
    for column in 0..4 {
        let pivot = (column..4)
            .max_by(|&a, &b| matrix[a][column].abs().total_cmp(&matrix[b][column].abs()))
            .unwrap();
        if matrix[pivot][column] == 0. {
            return None;
        }
        if pivot != column {
            matrix.swap(pivot, column);
            negative = !negative;
        }
        negative ^= matrix[column][column].is_sign_negative();
        for row in column + 1..4 {
            let factor = matrix[row][column] / matrix[column][column];
            for k in column + 1..4 {
                matrix[row][k] -= factor * matrix[column][k];
                if !matrix[row][k].is_finite() {
                    return None;
                }
            }
        }
    }
    Some(negative)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_projective_row_and_checks_triangle_interior() {
        let mut matrix = crate::affine::IDENTITY;
        matrix[3][0] = 0.5;
        assert_eq!(point(matrix, [2., 4., 6.]).unwrap().position, [1., 2., 3.]);
        let vertices =
            [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]].map(|p| point(matrix, p).unwrap());
        assert!(triangle_domain(vertices).is_ok());
        let vertices =
            [[-3., 0., 0.], [2., 0., 0.], [0., 2., 0.]].map(|p| point(matrix, p).unwrap());
        assert_eq!(triangle_domain(vertices), Err(ProjectiveError::Horizon));
        assert_eq!(point(matrix, [-2., 0., 0.]), Err(ProjectiveError::Horizon));
    }
    #[test]
    fn negative_chart_is_valid_and_overflow_is_explicit() {
        let mut matrix = crate::affine::IDENTITY;
        matrix[3][3] = -1.;
        let vertices =
            [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]].map(|p| point(matrix, p).unwrap());
        assert!(triangle_domain(vertices).is_ok());
        matrix[0][0] = f64::MAX;
        assert_eq!(
            point(matrix, [2., 0., 0.]),
            Err(ProjectiveError::NonFiniteOutput)
        );
        assert_eq!(
            point(matrix, [f64::NAN, 0., 0.]),
            Err(ProjectiveError::NonFiniteInput)
        );
    }
}
