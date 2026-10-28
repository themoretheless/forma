//! Small dense linear algebra shared by the geometry and photogrammetry cores. No external runtime.
pub type V2 = [f64; 2];
pub type V3 = [f64; 3];
pub type M3 = [[f64; 3]; 3];
pub const ID: M3 = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
