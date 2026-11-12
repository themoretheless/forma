//! Triangle queries.

use super::*;

/// Compute the 3D barycentric coordinates `[u, v, w]` of `p` projected onto triangle `(a, b, c)`
/// such that the orthogonal projection is `u*a + v*b + w*c` with `u + v + w == 1.0`.
/// Returns `None` if the triangle is degenerate (zero area) or inputs are non-finite.
pub fn barycentric_coords_3d(p: V3, a: V3, b: V3, c: V3) -> Option<V3> {
    let v0 = sub(b, a);
    let v1 = sub(c, a);
    let v2 = sub(p, a);
    let d00 = dot(v0, v0);
    let d01 = dot(v0, v1);
    let d11 = dot(v1, v1);
    let d20 = dot(v2, v0);
    let d21 = dot(v2, v1);
    let denom = d00 * d11 - d01 * d01;
    if !denom.is_finite() || denom.abs() <= 1e-24 {
        return None;
    }
    let inv = 1.0 / denom;
    let v = (d11 * d20 - d01 * d21) * inv;
    let w = (d00 * d21 - d01 * d20) * inv;
    let u = 1.0 - v - w;
    if u.is_finite() && v.is_finite() && w.is_finite() {
        Some([u, v, w])
    } else {
        None
    }
}

/// Find the closest point on 3D triangle `(a, b, c)` to query point `p` (Christer Ericson's Voronoi region algorithm).
pub fn closest_point_on_triangle_3d(p: V3, a: V3, b: V3, c: V3) -> V3 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let denom = d1 - d3;
        let v = if denom.abs() > 1e-24 { d1 / denom } else { 0.0 };
        return add(a, scale(ab, v));
    }

    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let denom = d2 - d6;
        let w = if denom.abs() > 1e-24 { d2 / denom } else { 0.0 };
        return add(a, scale(ac, w));
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let bc = sub(c, b);
        let denom = (d4 - d3) + (d5 - d6);
        let w = if denom.abs() > 1e-24 {
            (d4 - d3) / denom
        } else {
            0.0
        };
        return add(b, scale(bc, w));
    }

    let sum = va + vb + vc;
    if sum.abs() <= 1e-24 {
        return a;
    }
    let denom = 1.0 / sum;
    let v = vb * denom;
    let w = vc * denom;
    add(a, add(scale(ab, v), scale(ac, w)))
}

/// Möller–Trumbore ray-triangle intersection returning `Some((t, u, v))` when `ray` hits
/// triangle `(v0, v1, v2)` at distance `t >= 0.0` with barycentric weights `u >= 0.0`, `v >= 0.0`,
/// `u + v <= 1.0` such that the intersection point is `(1 - u - v) * v0 + u * v1 + v * v2`.
#[inline]
pub fn ray_triangle_intersect_moller_trumbore(
    ray: &Ray3,
    v0: V3,
    v1: V3,
    v2: V3,
) -> Option<(f64, f64, f64)> {
    const EPS: f64 = 1e-12;
    let edge1 = sub(v1, v0);
    let edge2 = sub(v2, v0);
    let h = cross(ray.direction, edge2);
    let a = dot(edge1, h);
    if !a.is_finite() || a.abs() < EPS {
        return None;
    }
    let f = 1.0 / a;
    let s = sub(ray.origin, v0);
    let u = f * dot(s, h);
    if !(-EPS..=1.0 + EPS).contains(&u) {
        return None;
    }
    let q = cross(s, edge1);
    let v = f * dot(ray.direction, q);
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = f * dot(edge2, q);
    if t >= 0.0 && t.is_finite() {
        Some((t, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
    } else {
        None
    }
}
