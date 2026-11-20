//! Six-plane view frustum extraction and culling predicates.

use super::*;

/// Six-plane view frustum (inward-pointing normals: `signed_distance(p) >= 0` is inside).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frustum {
    /// Order: left, right, bottom, top, near, far.
    pub planes: [Plane3; 6],
}

impl Frustum {
    /// Extract a 6-plane frustum (Gribb-Hartmann) from a row-major 4x4 view-projection matrix.
    /// Near plane uses standard OpenGL/clip `[-w, w]` (`r3 + r2`); see [`Self::from_view_proj_zero_to_one`] for WebGPU `[0, w]`.
    pub fn from_view_proj_row_major(m: AffineMatrix) -> Option<Self> {
        let near = Plane3::from_coeffs(
            m[3][0] + m[2][0],
            m[3][1] + m[2][1],
            m[3][2] + m[2][2],
            m[3][3] + m[2][3],
        )?;
        Self::from_view_proj_with_near(m, near)
    }

    /// Extract a 6-plane frustum from a row-major 4x4 view-projection matrix using WebGPU/Vulkan/D3D
    /// zero-to-one `[0, w]` clip-space depth (`z >= 0` from `r2`).
    pub fn from_view_proj_zero_to_one(m: AffineMatrix) -> Option<Self> {
        let r2 = m[2];
        let near = Plane3::from_coeffs(r2[0], r2[1], r2[2], r2[3])?;
        Self::from_view_proj_with_near(m, near)
    }

    fn from_view_proj_with_near(m: AffineMatrix, near: Plane3) -> Option<Self> {
        let r0 = m[0];
        let r1 = m[1];
        let r2 = m[2];
        let r3 = m[3];
        let left = Plane3::from_coeffs(r3[0] + r0[0], r3[1] + r0[1], r3[2] + r0[2], r3[3] + r0[3])?;
        let right =
            Plane3::from_coeffs(r3[0] - r0[0], r3[1] - r0[1], r3[2] - r0[2], r3[3] - r0[3])?;
        let bottom =
            Plane3::from_coeffs(r3[0] + r1[0], r3[1] + r1[1], r3[2] + r1[2], r3[3] + r1[3])?;
        let top = Plane3::from_coeffs(r3[0] - r1[0], r3[1] - r1[1], r3[2] - r1[2], r3[3] - r1[3])?;
        let far = Plane3::from_coeffs(r3[0] - r2[0], r3[1] - r2[1], r3[2] - r2[2], r3[3] - r2[3])?;
        Some(Self {
            planes: [left, right, bottom, top, near, far],
        })
    }

    /// Extract a 6-plane frustum from a column-major 16-element view-projection matrix (WebGPU/wgpu layout).
    pub fn from_view_proj_col_major(col_major: &[f64; 16]) -> Option<Self> {
        let m = crate::affine::solid_column_major(*col_major);
        Self::from_view_proj_row_major(m)
    }

    #[inline]
    pub fn contains_point(&self, point: V3) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance(point) >= -1e-9)
    }

    #[inline]
    pub fn intersects_sphere(&self, center: V3, radius: f64) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance(center) >= -radius)
    }

    /// Conservative frustum-AABB intersection test using the positive-vertex (p-vertex) test.
    #[inline]
    pub fn intersects_aabb(&self, min: V3, max: V3) -> bool {
        for plane in &self.planes {
            let p_vertex = [
                if plane.normal[0] >= 0.0 {
                    max[0]
                } else {
                    min[0]
                },
                if plane.normal[1] >= 0.0 {
                    max[1]
                } else {
                    min[1]
                },
                if plane.normal[2] >= 0.0 {
                    max[2]
                } else {
                    min[2]
                },
            ];
            if plane.signed_distance(p_vertex) < 0.0 {
                return false;
            }
        }
        true
    }

    /// Batch SIMD-friendly frustum culling mask over a slice of [`Aabb3`] bounding boxes.
    /// Precomputes plane normal sign selectors so each AABB evaluation avoids per-plane branchy sign checks.
    /// Returns a `Vec<bool>` where `true` indicates the AABB is visible (intersects the frustum).
    pub fn cull_aabbs_mask(&self, aabbs: &[Aabb3]) -> Vec<bool> {
        let sign_pos: [[bool; 3]; 6] = std::array::from_fn(|p| {
            let n = self.planes[p].normal;
            [n[0] >= 0.0, n[1] >= 0.0, n[2] >= 0.0]
        });
        aabbs
            .iter()
            .map(|aabb| {
                if aabb.is_empty() {
                    return false;
                }
                for (plane, signs) in self.planes.iter().zip(sign_pos.iter()) {
                    let px = if signs[0] { aabb.max[0] } else { aabb.min[0] };
                    let py = if signs[1] { aabb.max[1] } else { aabb.min[1] };
                    let pz = if signs[2] { aabb.max[2] } else { aabb.min[2] };
                    if plane.normal[0].mul_add(
                        px,
                        plane.normal[1].mul_add(py, plane.normal[2].mul_add(pz, plane.d)),
                    ) < 0.0
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_zero_to_one_enforces_non_negative_clip_z() {
        let frustum = Frustum::from_view_proj_zero_to_one(IDENTITY).unwrap();
        assert!(frustum.contains_point([0.0, 0.0, 0.5]));
        assert!(frustum.contains_point([0.0, 0.0, 0.0]));
        assert!(!frustum.contains_point([0.0, 0.0, -0.1]));
        assert!(!frustum.contains_point([0.0, 0.0, 1.1]));
    }
}
