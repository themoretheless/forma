//! CPU-side mirror of the WGSL uniform structs. Field order and float offsets
//! are the GPU contract: `write_f32` emits exactly the layout the shaders
//! read, and the tests pin the offsets against the browser renderer.
//!
//! The offset constants (`SCENE_UNIFORM_FLOATS`, `STYLE_FLOAT_OFFSET`, …) are
//! generated from the single declarative table in [`crate::layout`] by the
//! `wgsl_export` codegen (see [`crate::generated_layouts`]) and re-exported
//! here, so this module's public API is unchanged.

pub use crate::generated_layouts::*;

/// Object uniform: model (16 floats) + nmat (16) + color (4) + style (4)
/// + morph (4) + baseColor (3) + metallic (1) + emissive (3) + roughness (1)
/// + materialId (1) + pad (3) = 56 floats = 224 bytes.
///
/// `style` is (alpha, selected, edge, hovered); `morph.x` drives the GPU
/// vertex blend toward the slot-1 morph source positions. The material tail
/// starts at float 44, so legacy field offsets are unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectUniform {
    /// Column-major 4x4 model matrix.
    pub model: [f32; 16],
    /// Column-major inverse-transpose of the model (normal matrix).
    pub nmat: [f32; 16],
    pub color: [f32; 4],
    pub style: [f32; 4],
    pub morph: [f32; 4],
    /// Material base color multiplier (white = legacy look).
    pub base_color: [f32; 3],
    pub metallic: f32,
    /// Additive emission (black = none).
    pub emissive: [f32; 3],
    pub roughness: f32,
    /// Material preset/flags slot (0 = none).
    pub material_id: f32,
}

impl ObjectUniform {
    pub const fn new(model: [f32; 16], nmat: [f32; 16], color: [f32; 4]) -> Self {
        Self {
            model, nmat, color,
            style: [1.0, 0.0, 0.0, 0.0], morph: [1.0, 0.0, 0.0, 0.0],
            base_color: [1.0, 1.0, 1.0], metallic: 0.0,
            emissive: [0.0, 0.0, 0.0], roughness: 0.7, material_id: 0.0,
        }
    }

    /// Writes the full 56-float record into `out`.
    pub fn write_f32(&self, out: &mut [f32; OBJECT_UNIFORM_FLOATS]) {
        out[..16].copy_from_slice(&self.model);
        out[16..32].copy_from_slice(&self.nmat);
        out[32..36].copy_from_slice(&self.color);
        out[36..40].copy_from_slice(&self.style);
        out[40..44].copy_from_slice(&self.morph);
        out[44..47].copy_from_slice(&self.base_color);
        out[47] = self.metallic;
        out[48..51].copy_from_slice(&self.emissive);
        out[51] = self.roughness;
        out[52] = self.material_id;
        out[53..56].fill(0.0);
    }
}

impl Default for ObjectUniform {
    fn default() -> Self {
        // Rest state: weight 1 renders the vertex-buffer target positions and
        // keeps the zero slot-1 morph dummy inert (mix(dummy, pos, 1) = pos),
        // matching the browser renderer's convention.
        Self::new(
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            [0.8, 0.8, 0.8, 1.0],
        )
    }
}

/// Scene uniform: view-projection (16) + eye (4) + light (4) + ambient (4)
/// + section (4) + options (4) + inverse view-projection (16) + theme block
/// (selection/hover/edge/xray/grid/cap colors, 3 floats + pad each = 24)
/// + shadow block (light view-projection 16 + params 4) = 96 floats = 384
/// bytes. The theme tail starts at float 52 and the shadow tail at float 76,
/// so legacy field offsets are unchanged; shaders that do not read the theme
/// simply bind a smaller struct view of the same buffer.
///
/// `shadow_params` = (enabled, 1/map size, depth bias, strength); enabled 0
/// keeps every sampling shader on its unshadowed path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneUniform {
    /// Column-major view-projection matrix.
    pub view_projection: [f32; 16],
    pub eye: [f32; 4],
    pub light: [f32; 4],
    pub ambient: [f32; 4],
    /// xyz: section plane normal, w: plane offset.
    pub section: [f32; 4],
    /// x: section enabled, y: grid step, z: grid extent, w: grid fade distance.
    pub options: [f32; 4],
    /// Column-major inverse view-projection (grid ray reconstruction).
    pub inverse_vp: [f32; 16],
    /// Selection tint (mesh family mixes toward it).
    pub selection_color: [f32; 3],
    /// Hover tint.
    pub hover_color: [f32; 3],
    /// Base wireframe edge color.
    pub edge_color: [f32; 3],
    /// X-ray (deep_mesh) fresnel glow color.
    pub xray_color: [f32; 3],
    /// Ground grid line color (axes stay fixed red/green).
    pub grid_color: [f32; 3],
    /// Section-cap fill color (mesh_section_cap and the epsilon accent).
    pub cap_color: [f32; 3],
    /// Column-major key-light orthographic view-projection (shadow map).
    pub light_vp: [f32; 16],
    /// (enabled, 1/map size, depth bias, strength).
    pub shadow_params: [f32; 4],
}

/// Column-major key-light orthographic view-projection covering the scene
/// bounds, ported 1:1 from the browser renderer's `shadowLightVP` (pure
/// row-major JS math in webgpuRenderer.ts + math3d.lookAt/orthographic).
/// `light` is the scene light direction (scene uniform floats 20..22),
/// `center`/`radius` the scene bounds; the extent is radius × 1.25.
pub fn shadow_light_vp(light: [f32; 3], center: [f32; 3], radius: f32) -> [f32; 16] {
    let len = (light[0] * light[0] + light[1] * light[1] + light[2] * light[2])
        .sqrt()
        .max(f32::EPSILON);
    let l = [light[0] / len, light[1] / len, light[2] / len];
    let extent = radius.max(1e-3) * 1.25;
    let eye = [
        center[0] + l[0] * extent * 2.0,
        center[1] + l[1] * extent * 2.0,
        center[2] + l[2] * extent * 2.0,
    ];
    let view = look_at_row_major(eye, center, [0.0, 0.0, 1.0]);
    let proj = orthographic_row_major(-extent, extent, -extent, extent, 0.0, extent * 4.0);
    let vp = multiply_row_major(&proj, &view);
    // Row-major product → column-major output (the WGSL mat4 layout).
    let mut out = [0.0f32; 16];
    for row in 0..4 {
        for column in 0..4 {
            out[column * 4 + row] = vp[row * 4 + column];
        }
    }
    out
}

/// Row-major mat4 product (a · b), mirroring `multiplyRowMajor`.
fn multiply_row_major(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for row in 0..4 {
        for column in 0..4 {
            out[row * 4 + column] = a[row * 4] * b[column]
                + a[row * 4 + 1] * b[4 + column]
                + a[row * 4 + 2] * b[8 + column]
                + a[row * 4 + 3] * b[12 + column];
        }
    }
    out
}

/// Right-handed WebGPU orthographic projection (depth range [0, 1]),
/// row-major; mirrors math3d.orthographic.
fn orthographic_row_major(
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    near: f32,
    far: f32,
) -> [f32; 16] {
    let lr = 1.0 / (right - left);
    let bt = 1.0 / (top - bottom);
    let nf = 1.0 / (near - far);
    let mut m = [0.0f32; 16];
    m[0] = 2.0 * lr;
    m[3] = -(right + left) * lr;
    m[5] = 2.0 * bt;
    m[7] = -(top + bottom) * bt;
    m[10] = nf;
    m[11] = near * nf;
    m[15] = 1.0;
    m
}

/// Row-major lookAt view matrix; mirrors math3d.lookAt, including the
/// parallel-up-axis fallback.
fn look_at_row_major(eye: [f32; 3], center: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let mut zx = eye[0] - center[0];
    let mut zy = eye[1] - center[1];
    let mut zz = eye[2] - center[2];
    let mut len = (zx * zx + zy * zy + zz * zz).sqrt();
    if len < 1e-10 {
        zx = 0.0;
        zy = 0.0;
        zz = 1.0;
        len = 1.0;
    }
    let fz = [zx / len, zy / len, zz / len];

    let mut xx = up[1] * fz[2] - up[2] * fz[1];
    let mut xy = up[2] * fz[0] - up[0] * fz[2];
    let mut xz = up[0] * fz[1] - up[1] * fz[0];
    len = (xx * xx + xy * xy + xz * xz).sqrt();
    if len < 1e-10 {
        let (ax, ay, az) = if fz[2].abs() > 0.9 { (0.0, 1.0, 0.0) } else { (0.0, 0.0, 1.0) };
        xx = ay * fz[2] - az * fz[1];
        xy = az * fz[0] - ax * fz[2];
        xz = ax * fz[1] - ay * fz[0];
        len = (xx * xx + xy * xy + xz * xz).sqrt();
    }
    let fx = [xx / len, xy / len, xz / len];
    let fy = [
        fz[1] * fx[2] - fz[2] * fx[1],
        fz[2] * fx[0] - fz[0] * fx[2],
        fz[0] * fx[1] - fz[1] * fx[0],
    ];
    let mut m = [0.0f32; 16];
    m[0] = fx[0]; m[1] = fx[1]; m[2] = fx[2]; m[3] = -(fx[0] * eye[0] + fx[1] * eye[1] + fx[2] * eye[2]);
    m[4] = fy[0]; m[5] = fy[1]; m[6] = fy[2]; m[7] = -(fy[0] * eye[0] + fy[1] * eye[1] + fy[2] * eye[2]);
    m[8] = fz[0]; m[9] = fz[1]; m[10] = fz[2]; m[11] = -(fz[0] * eye[0] + fz[1] * eye[1] + fz[2] * eye[2]);
    m[15] = 1.0;
    m
}

impl SceneUniform {
    pub const fn new(view_projection: [f32; 16], eye: [f32; 4]) -> Self {
        Self {
            view_projection,
            eye,
            light: [0.55, 0.75, 0.45, 0.0],
            ambient: [0.22, 0.22, 0.24, 1.0],
            section: [0.0, 0.0, 1.0, 0.0],
            options: [0.0, 1.0, 0.0, 0.0],
            inverse_vp: [0.0; 16],
            // Defaults reproduce the previously hard-coded shader colors.
            selection_color: [1.0, 0.52, 0.06],
            hover_color: [0.12, 0.78, 1.0],
            edge_color: [0.025, 0.03, 0.04],
            xray_color: [1.0, 0.42, 0.06],
            grid_color: [0.42, 0.42, 0.42],
            cap_color: [0.85, 0.87, 0.9],
            // Shadows off by default: enabled 0 keeps the unshadowed path.
            light_vp: [0.0; 16],
            shadow_params: [0.0, 1.0 / 1024.0, 0.0015, 1.0],
        }
    }

    /// Writes the full 96-float record into `out`.
    pub fn write_f32(&self, out: &mut [f32; SCENE_UNIFORM_FLOATS]) {
        out[VP_FLOAT_OFFSET..EYE_FLOAT_OFFSET].copy_from_slice(&self.view_projection);
        out[EYE_FLOAT_OFFSET..LIGHT_FLOAT_OFFSET].copy_from_slice(&self.eye);
        out[LIGHT_FLOAT_OFFSET..AMBIENT_FLOAT_OFFSET].copy_from_slice(&self.light);
        out[AMBIENT_FLOAT_OFFSET..SECTION_FLOAT_OFFSET].copy_from_slice(&self.ambient);
        out[SECTION_FLOAT_OFFSET..OPTIONS_FLOAT_OFFSET].copy_from_slice(&self.section);
        out[OPTIONS_FLOAT_OFFSET..INVERSE_VP_FLOAT_OFFSET].copy_from_slice(&self.options);
        out[INVERSE_VP_FLOAT_OFFSET..THEME_FLOAT_OFFSET].copy_from_slice(&self.inverse_vp);
        out[THEME_FLOAT_OFFSET..THEME_FLOAT_OFFSET + 3].copy_from_slice(&self.selection_color);
        out[HOVER_FLOAT_OFFSET..HOVER_FLOAT_OFFSET + 3].copy_from_slice(&self.hover_color);
        out[EDGE_FLOAT_OFFSET..EDGE_FLOAT_OFFSET + 3].copy_from_slice(&self.edge_color);
        out[XRAY_FLOAT_OFFSET..XRAY_FLOAT_OFFSET + 3].copy_from_slice(&self.xray_color);
        out[GRID_FLOAT_OFFSET..GRID_FLOAT_OFFSET + 3].copy_from_slice(&self.grid_color);
        out[CAP_FLOAT_OFFSET..CAP_FLOAT_OFFSET + 3].copy_from_slice(&self.cap_color);
        out[LIGHT_VP_FLOAT_OFFSET..SHADOW_FLOAT_OFFSET].copy_from_slice(&self.light_vp);
        out[SHADOW_FLOAT_OFFSET..SCENE_UNIFORM_FLOATS].copy_from_slice(&self.shadow_params);
        out[THEME_FLOAT_OFFSET + 3] = 0.0;
        out[HOVER_FLOAT_OFFSET + 3] = 0.0;
        out[EDGE_FLOAT_OFFSET + 3] = 0.0;
        out[XRAY_FLOAT_OFFSET + 3] = 0.0;
        out[GRID_FLOAT_OFFSET + 3] = 0.0;
        out[CAP_FLOAT_OFFSET + 3] = 0.0;
    }
}
