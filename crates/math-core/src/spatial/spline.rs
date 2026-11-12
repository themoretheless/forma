//! Catmull-Rom splines.

use super::*;

/// 3D Catmull-Rom spline segment defined by four control points `(p0, p1, p2, p3)` and
/// chordal exponent `alpha` (`0.0` = uniform, `0.5` = centripetal, `1.0` = chordal).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatmullRomSpline3 {
    pub p0: V3,
    pub p1: V3,
    pub p2: V3,
    pub p3: V3,
    pub alpha: f64,
}

impl CatmullRomSpline3 {
    #[inline]
    pub const fn new(p0: V3, p1: V3, p2: V3, p3: V3, alpha: f64) -> Self {
        Self {
            p0,
            p1,
            p2,
            p3,
            alpha,
        }
    }

    /// Construct a centripetal (`alpha = 0.5`) 3D Catmull-Rom segment, which guarantees no cusps or self-intersections.
    #[inline]
    pub const fn centripetal(p0: V3, p1: V3, p2: V3, p3: V3) -> Self {
        Self::new(p0, p1, p2, p3, 0.5)
    }

    #[inline]
    pub fn evaluate(&self, t: f64) -> V3 {
        evaluate_catmull_rom_3d(self.p0, self.p1, self.p2, self.p3, t, self.alpha)
    }

    #[inline]
    pub fn tangent(&self, t: f64) -> V3 {
        evaluate_catmull_rom_tangent_3d(self.p0, self.p1, self.p2, self.p3, t, self.alpha)
    }
}

/// Evaluate a 3D Barry–Goldman parameterized Catmull-Rom spline between `p1` (`t = 0`) and `p2` (`t = 1`)
/// with neighbor controls `p0`, `p3` and parameterization exponent `alpha` (`0.0` = uniform, `0.5` = centripetal).
pub fn evaluate_catmull_rom_3d(p0: V3, p1: V3, p2: V3, p3: V3, t: f64, alpha: f64) -> V3 {
    let u = t.clamp(0.0, 1.0);
    let a = alpha.clamp(0.0, 1.0);
    let knot_delta = |pa: V3, pb: V3| -> f64 {
        let d = norm(sub(pb, pa));
        let dt = d.powf(a);
        if dt.is_finite() && dt > 1e-9 { dt } else { 1.0 }
    };
    let t0 = 0.0;
    let t1 = t0 + knot_delta(p0, p1);
    let t2 = t1 + knot_delta(p1, p2);
    let t3 = t2 + knot_delta(p2, p3);
    let tau = t1 + u * (t2 - t1);

    let lerp_pt = |pa: V3, pb: V3, ta: f64, tb: f64| -> V3 {
        let denom = tb - ta;
        if denom.abs() <= 1e-15 {
            pa
        } else {
            let w = (tau - ta) / denom;
            add(scale(pa, 1.0 - w), scale(pb, w))
        }
    };

    let a1 = lerp_pt(p0, p1, t0, t1);
    let a2 = lerp_pt(p1, p2, t1, t2);
    let a3 = lerp_pt(p2, p3, t2, t3);

    let b1 = lerp_pt(a1, a2, t0, t2);
    let b2 = lerp_pt(a2, a3, t1, t3);

    lerp_pt(b1, b2, t1, t2)
}

/// Evaluate the first derivative $dC/dt$ (with respect to normalized segment parameter $t \in [0, 1]$)
/// of a 3D Barry–Goldman Catmull-Rom spline between `p1` and `p2`.
pub fn evaluate_catmull_rom_tangent_3d(p0: V3, p1: V3, p2: V3, p3: V3, t: f64, alpha: f64) -> V3 {
    let u = t.clamp(0.0, 1.0);
    let a = alpha.clamp(0.0, 1.0);
    let knot_delta = |pa: V3, pb: V3| -> f64 {
        let d = norm(sub(pb, pa));
        let dt = d.powf(a);
        if dt.is_finite() && dt > 1e-9 { dt } else { 1.0 }
    };
    let t0 = 0.0;
    let t1 = t0 + knot_delta(p0, p1);
    let t2 = t1 + knot_delta(p1, p2);
    let t3 = t2 + knot_delta(p2, p3);
    let dt12 = t2 - t1;
    let tau = t1 + u * dt12;

    let lerp_pt = |pa: V3, pb: V3, ta: f64, tb: f64| -> (V3, V3) {
        let denom = tb - ta;
        if denom.abs() <= 1e-15 {
            (pa, [0.0, 0.0, 0.0])
        } else {
            let inv = 1.0 / denom;
            let w = (tau - ta) * inv;
            let val = add(scale(pa, 1.0 - w), scale(pb, w));
            let dval = scale(sub(pb, pa), inv);
            (val, dval)
        }
    };

    let (a1, da1) = lerp_pt(p0, p1, t0, t1);
    let (a2, da2) = lerp_pt(p1, p2, t1, t2);
    let (a3, da3) = lerp_pt(p2, p3, t2, t3);

    let combine = |va: V3, dva: V3, vb: V3, dvb: V3, ta: f64, tb: f64| -> (V3, V3) {
        let denom = tb - ta;
        if denom.abs() <= 1e-15 {
            (va, dva)
        } else {
            let inv = 1.0 / denom;
            let w = (tau - ta) * inv;
            let val = add(scale(va, 1.0 - w), scale(vb, w));
            let term1 = scale(sub(vb, va), inv);
            let term2 = add(scale(dva, 1.0 - w), scale(dvb, w));
            (val, add(term1, term2))
        }
    };

    let (b1, db1) = combine(a1, da1, a2, da2, t0, t2);
    let (b2, db2) = combine(a2, da2, a3, da3, t1, t3);
    let (_c, dc_dtau) = combine(b1, db1, b2, db2, t1, t2);
    scale(dc_dtau, dt12)
}
