// GENERATED FILE — do not edit.
// Generated from crates/raster-core/src/layout.rs (the single source of truth
// for the Scene/Obj uniform layouts) by the raster-core `wgsl_export` codegen.
// Regenerate:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export
// Verify:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export -- --check

pub const SCENE_UNIFORM_FLOATS: usize = 96;
pub const SCENE_UNIFORM_BYTES: u64 = 384;
/// View-projection (column-major mat4) at +0..15.
pub const VP_FLOAT_OFFSET: usize = 0;
pub const VP_BYTE_OFFSET: u64 = 0;
/// Eye position (w = 1).
pub const EYE_FLOAT_OFFSET: usize = 16;
pub const EYE_BYTE_OFFSET: u64 = 64;
/// Light direction.
pub const LIGHT_FLOAT_OFFSET: usize = 20;
pub const LIGHT_BYTE_OFFSET: u64 = 80;
/// Ambient color.
pub const AMBIENT_FLOAT_OFFSET: usize = 24;
pub const AMBIENT_BYTE_OFFSET: u64 = 96;
/// Section plane (normal xyz, offset w).
pub const SECTION_FLOAT_OFFSET: usize = 28;
pub const SECTION_BYTE_OFFSET: u64 = 112;
/// Options (section enabled, grid step, extent, fade).
pub const OPTIONS_FLOAT_OFFSET: usize = 32;
pub const OPTIONS_BYTE_OFFSET: u64 = 128;
/// Inverse view-projection (column-major mat4) at +0..15.
pub const INVERSE_VP_FLOAT_OFFSET: usize = 36;
pub const INVERSE_VP_BYTE_OFFSET: u64 = 144;
/// Theme block: selectionColor at +0..2, then hover/edge/xray/grid/cap colors every 4 floats (vec3 + pad, 16-byte aligned).
pub const THEME_FLOAT_OFFSET: usize = 52;
pub const THEME_BYTE_OFFSET: u64 = 208;
/// hoverColor rgb at +0..2.
pub const HOVER_FLOAT_OFFSET: usize = 56;
pub const HOVER_BYTE_OFFSET: u64 = 224;
/// edgeColor rgb at +0..2.
pub const EDGE_FLOAT_OFFSET: usize = 60;
pub const EDGE_BYTE_OFFSET: u64 = 240;
/// xrayColor rgb at +0..2.
pub const XRAY_FLOAT_OFFSET: usize = 64;
pub const XRAY_BYTE_OFFSET: u64 = 256;
/// gridColor rgb at +0..2.
pub const GRID_FLOAT_OFFSET: usize = 68;
pub const GRID_BYTE_OFFSET: u64 = 272;
/// capColor rgb at +0..2.
pub const CAP_FLOAT_OFFSET: usize = 72;
pub const CAP_BYTE_OFFSET: u64 = 288;
/// Light view-projection (column-major mat4) at +0..15.
pub const LIGHT_VP_FLOAT_OFFSET: usize = 76;
pub const LIGHT_VP_BYTE_OFFSET: u64 = 304;
/// Shadow params (enabled, texel, bias, strength) at +0..3.
pub const SHADOW_FLOAT_OFFSET: usize = 92;
pub const SHADOW_BYTE_OFFSET: u64 = 368;

pub const OBJECT_UNIFORM_FLOATS: usize = 56;
pub const OBJECT_UNIFORM_BYTES: u64 = 224;
/// style = (alpha, selected, edge, hovered).
pub const STYLE_FLOAT_OFFSET: usize = 36;
pub const STYLE_BYTE_OFFSET: u64 = 144;
/// morph.x drives the GPU vertex blend.
pub const MORPH_FLOAT_OFFSET: usize = 40;
pub const MORPH_BYTE_OFFSET: u64 = 160;
/// baseColor rgb at +0..2, metallic at +3.
pub const MATERIAL_FLOAT_OFFSET: usize = 44;
pub const MATERIAL_BYTE_OFFSET: u64 = 176;
/// metallic scalar.
pub const METALLIC_FLOAT_OFFSET: usize = 47;
pub const METALLIC_BYTE_OFFSET: u64 = 188;
/// emissive rgb at +0..2, roughness at +3.
pub const EMISSIVE_FLOAT_OFFSET: usize = 48;
pub const EMISSIVE_BYTE_OFFSET: u64 = 192;
/// roughness scalar.
pub const ROUGHNESS_FLOAT_OFFSET: usize = 51;
pub const ROUGHNESS_BYTE_OFFSET: u64 = 204;
/// materialId scalar.
pub const MATERIAL_ID_FLOAT_OFFSET: usize = 52;
pub const MATERIAL_ID_BYTE_OFFSET: u64 = 208;
