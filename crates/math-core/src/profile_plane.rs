//! Bounded common-plane inference and projection of authored control points.
use crate::{Error, Result, cross, dot};
#[derive(Clone, Copy)]
pub struct Plane {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}
fn norm(p: [f64; 3]) -> f64 {
    p[0].hypot(p[1]).hypot(p[2])
}
fn unit(p: [f64; 3]) -> Result<[f64; 3]> {
    let n = norm(p);
    if !n.is_finite() || n < 1e-9 {
        return Err(Error::new("GEOMETRY_INVALID_INPUT", "Zero direction"));
    }
    Ok(p.map(|x| x / n))
}
pub fn infer(points: &[[f64; 3]]) -> Result<Plane> {
    let origin = *points
        .first()
        .ok_or_else(|| Error::new("GEOMETRY_INVALID_INPUT", "Empty profile controls"))?;
    if points.len() > 65536 || !points.iter().flatten().all(|x| x.is_finite()) {
        return Err(Error::new(
            "GEOMETRY_INVALID_INPUT",
            "Invalid profile controls",
        ));
    }
    if points.iter().all(|p| p[2] == origin[2]) {
        return Ok(Plane {
            origin: [0., 0., origin[2]],
            u: [1., 0., 0.],
            v: [0., 1., 0.],
        });
    }
    let vectors = points
        .iter()
        .map(|p| std::array::from_fn(|k| p[k] - origin[k]))
        .collect::<Vec<[f64; 3]>>();
    let longest = vectors
        .iter()
        .copied()
        .reduce(|a, b| if norm(a) > norm(b) { a } else { b })
        .unwrap();
    let u = unit(longest)?;
    let mut normal = vectors
        .iter()
        .map(|v| cross(u, *v))
        .reduce(|a, b| if norm(a) > norm(b) { a } else { b })
        .unwrap();
    if norm(normal) < 1e-9 {
        let mut axes = [0, 1, 2];
        axes.sort_by(|a, b| u[*a].abs().total_cmp(&u[*b].abs()));
        let mut reference = [0.; 3];
        reference[axes[0]] = 1.;
        normal = cross(u, reference);
    }
    normal = unit(normal)?;
    let mut axes = [0, 1, 2];
    axes.sort_by(|a, b| normal[*b].abs().total_cmp(&normal[*a].abs()));
    if normal[axes[0]] < 0. {
        normal = normal.map(|x| -x);
    }
    Ok(Plane {
        origin,
        u,
        v: unit(cross(normal, u))?,
    })
}
pub fn project(points: &[[f64; 3]], plane: Plane) -> Result<(Vec<[f64; 2]>, f64)> {
    if points.len() > 65536
        || !points
            .iter()
            .flatten()
            .chain(plane.origin.iter())
            .chain(plane.u.iter())
            .chain(plane.v.iter())
            .all(|x| x.is_finite())
    {
        return Err(Error::new(
            "GEOMETRY_INVALID_INPUT",
            "Invalid profile plane",
        ));
    }
    let mut error = 0f64;
    let mut projected = Vec::with_capacity(points.len());
    for p in points {
        let delta = std::array::from_fn(|k| p[k] - plane.origin[k]);
        let local = [dot(delta, plane.u), dot(delta, plane.v)];
        let reconstructed: [f64; 3] = std::array::from_fn(|k| {
            plane.origin[k] + local[0] * plane.u[k] + local[1] * plane.v[k]
        });
        let deviation = norm(std::array::from_fn(|k| p[k] - reconstructed[k]));
        if !deviation.is_finite() || deviation > 1e-7 {
            return Err(Error::new(
                "GEOMETRY_INVALID_INPUT",
                "Profile input must lie in the same sketch plane; control deviation exceeds 0.0000001 mm.",
            ));
        }
        error = error.max(deviation);
        projected.push(local);
    }
    Ok((projected, error))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tilted_controls_project_without_editing_source() {
        let points = [[0., 0., 0.], [1., 0., 1.], [1., 1., 1.]];
        let before = points;
        let plane = infer(&points).unwrap();
        let (xy, error) = project(&points, plane).unwrap();
        assert!(error < 1e-12);
        assert_eq!(xy.len(), 3);
        assert_eq!(points, before);
        assert!(
            project(
                &[[0., 0., 1.]],
                Plane {
                    origin: [0.; 3],
                    u: [1., 0., 0.],
                    v: [0., 1., 0.]
                }
            )
            .is_err()
        );
    }
}
