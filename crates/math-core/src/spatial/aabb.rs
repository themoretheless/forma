//! 3D Axis-Aligned Bounding Box (`Aabb3`).

use super::*;

/// 3D Axis-Aligned Bounding Box defined by inclusive `min` and `max` corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb3 {
    pub min: V3,
    pub max: V3,
}

impl Default for Aabb3 {
    fn default() -> Self {
        Self::empty()
    }
}

impl Aabb3 {
    /// Return an inverted (empty) bounding box ready for incremental expansion.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            min: [f64::INFINITY, f64::INFINITY, f64::INFINITY],
            max: [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        }
    }

    #[inline]
    pub const fn new(min: V3, max: V3) -> Self {
        Self { min, max }
    }

    /// True when `min[i] > max[i]` on any axis.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }

    /// Build the smallest AABB enclosing `points`, or [`Self::empty()`] if `points` is empty.
    pub fn from_points(points: &[V3]) -> Self {
        let mut bbox = Self::empty();
        for &p in points {
            bbox.expand_point(p);
        }
        bbox
    }

    /// Expand this AABB in-place to include `point` (ignores non-finite coordinates).
    #[inline]
    pub fn expand_point(&mut self, point: V3) {
        if !point.iter().all(|v| v.is_finite()) {
            return;
        }
        for ((min_v, max_v), p) in self.min.iter_mut().zip(self.max.iter_mut()).zip(point) {
            *min_v = min_v.min(p);
            *max_v = max_v.max(p);
        }
    }

    /// Smallest AABB enclosing both `self` and `other`.
    #[inline]
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        Self {
            min: [
                self.min[0].min(other.min[0]),
                self.min[1].min(other.min[1]),
                self.min[2].min(other.min[2]),
            ],
            max: [
                self.max[0].max(other.max[0]),
                self.max[1].max(other.max[1]),
                self.max[2].max(other.max[2]),
            ],
        }
    }

    /// Intersection of `self` and `other` (returns an empty box if disjoint).
    #[inline]
    pub fn intersection(&self, other: &Self) -> Self {
        if self.is_empty() || other.is_empty() {
            return Self::empty();
        }
        let min = [
            self.min[0].max(other.min[0]),
            self.min[1].max(other.min[1]),
            self.min[2].max(other.min[2]),
        ];
        let max = [
            self.max[0].min(other.max[0]),
            self.max[1].min(other.max[1]),
            self.max[2].min(other.max[2]),
        ];
        if min[0] > max[0] || min[1] > max[1] || min[2] > max[2] {
            Self::empty()
        } else {
            Self { min, max }
        }
    }

    /// Side lengths `max - min` clamped to `>= 0.0` (returns `[0.0; 3]` if empty).
    #[inline]
    pub fn extents(&self) -> V3 {
        if self.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        [
            (self.max[0] - self.min[0]).max(0.0),
            (self.max[1] - self.min[1]).max(0.0),
            (self.max[2] - self.min[2]).max(0.0),
        ]
    }

    /// Center `(min + max) * 0.5`.
    #[inline]
    pub fn center(&self) -> V3 {
        if self.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }

    /// Surface area `2 * (dx*dy + dy*dz + dz*dx)`.
    #[inline]
    pub fn surface_area(&self) -> f64 {
        let e = self.extents();
        2.0 * (e[0] * e[1] + e[1] * e[2] + e[2] * e[0])
    }

    /// Volume `dx * dy * dz`.
    #[inline]
    pub fn volume(&self) -> f64 {
        let e = self.extents();
        e[0] * e[1] * e[2]
    }

    /// Index (`0`, `1`, or `2`) of the longest extent axis.
    #[inline]
    pub fn longest_axis(&self) -> usize {
        let e = self.extents();
        if e[0] >= e[1] && e[0] >= e[2] {
            0
        } else if e[1] >= e[2] {
            1
        } else {
            2
        }
    }

    /// True when `point` lies inside or on the boundary of this AABB.
    #[inline]
    pub fn contains_point(&self, point: V3) -> bool {
        !self.is_empty()
            && point[0] >= self.min[0]
            && point[0] <= self.max[0]
            && point[1] >= self.min[1]
            && point[1] <= self.max[1]
            && point[2] >= self.min[2]
            && point[2] <= self.max[2]
    }

    /// True when `self` and `other` overlap (including touching boundaries).
    #[inline]
    pub fn intersects(&self, other: &Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.min[0] <= other.max[0]
            && self.max[0] >= other.min[0]
            && self.min[1] <= other.max[1]
            && self.max[1] >= other.min[1]
            && self.min[2] <= other.max[2]
            && self.max[2] >= other.min[2]
    }
}
