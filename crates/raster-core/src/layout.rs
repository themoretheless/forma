//! Single declarative table of the Scene/Obj uniform layouts. This is the
//! one source of truth for the CPU-side field offsets: the `wgsl_export`
//! codegen renders BOTH `src/services/shaders/generated/layouts.ts` (the
//! browser renderer's `SCENE_UNIFORM_LAYOUT`/`OBJECT_UNIFORM_LAYOUT`) and
//! `crates/raster-core/src/generated_layouts.rs` (the native mirror constants
//! re-exported by `uniform.rs`) from these tables, so the two languages
//! cannot drift apart.

/// One named field offset in a uniform record.
pub struct LayoutField {
    /// TypeScript key stem: `vp` → `vpFloatOffset` / `vpByteOffset`.
    pub ts: &'static str,
    /// Rust const stem: `VP` → `VP_FLOAT_OFFSET` / `VP_BYTE_OFFSET`.
    pub rs: &'static str,
    /// Float offset of the field within the record.
    pub float_offset: usize,
    /// Whether the TS layout object carries the byte-offset key too (the
    /// browser only reads byte offsets for the buffer-view boundaries).
    pub ts_byte: bool,
    /// Doc comment emitted above the generated constants.
    pub doc: &'static str,
}

/// Scene uniform fields in declaration order: vp (16 floats) + eye (4)
/// + light (4) + ambient (4) + section (4) + options (4) + inverseVP (16)
/// + theme block (6 colors × vec3+pad = 24) + shadow block (lightVP 16
/// + shadowParams 4).
pub const SCENE_FIELDS: &[LayoutField] = &[
    LayoutField { ts: "vp", rs: "VP", float_offset: 0, ts_byte: true,
        doc: "View-projection (column-major mat4) at +0..15." },
    LayoutField { ts: "eye", rs: "EYE", float_offset: 16, ts_byte: true,
        doc: "Eye position (w = 1)." },
    LayoutField { ts: "light", rs: "LIGHT", float_offset: 20, ts_byte: true,
        doc: "Light direction." },
    LayoutField { ts: "ambient", rs: "AMBIENT", float_offset: 24, ts_byte: true,
        doc: "Ambient color." },
    LayoutField { ts: "section", rs: "SECTION", float_offset: 28, ts_byte: true,
        doc: "Section plane (normal xyz, offset w)." },
    LayoutField { ts: "options", rs: "OPTIONS", float_offset: 32, ts_byte: true,
        doc: "Options (section enabled, grid step, extent, fade)." },
    LayoutField { ts: "inverseVP", rs: "INVERSE_VP", float_offset: 36, ts_byte: true,
        doc: "Inverse view-projection (column-major mat4) at +0..15." },
    LayoutField { ts: "theme", rs: "THEME", float_offset: 52, ts_byte: true,
        doc: "Theme block: selectionColor at +0..2, then hover/edge/xray/grid/cap colors every 4 floats (vec3 + pad, 16-byte aligned)." },
    LayoutField { ts: "hover", rs: "HOVER", float_offset: 56, ts_byte: false,
        doc: "hoverColor rgb at +0..2." },
    LayoutField { ts: "edge", rs: "EDGE", float_offset: 60, ts_byte: false,
        doc: "edgeColor rgb at +0..2." },
    LayoutField { ts: "xray", rs: "XRAY", float_offset: 64, ts_byte: false,
        doc: "xrayColor rgb at +0..2." },
    LayoutField { ts: "grid", rs: "GRID", float_offset: 68, ts_byte: false,
        doc: "gridColor rgb at +0..2." },
    LayoutField { ts: "cap", rs: "CAP", float_offset: 72, ts_byte: true,
        doc: "capColor rgb at +0..2." },
    LayoutField { ts: "lightVP", rs: "LIGHT_VP", float_offset: 76, ts_byte: true,
        doc: "Light view-projection (column-major mat4) at +0..15." },
    LayoutField { ts: "shadow", rs: "SHADOW", float_offset: 92, ts_byte: true,
        doc: "Shadow params (enabled, texel, bias, strength) at +0..3." },
];

pub const SCENE_UNIFORM_FLOATS_VALUE: usize = 96;
pub const SCENE_UNIFORM_BYTES_VALUE: u64 = 384;

/// Obj uniform fields in declaration order: model (16) + nmat (16) + color
/// (4) + style (4) + morph (4) + baseColor (3) + metallic (1) + emissive (3)
/// + roughness (1) + materialId (1) + pad (3).
pub const OBJECT_FIELDS: &[LayoutField] = &[
    LayoutField { ts: "style", rs: "STYLE", float_offset: 36, ts_byte: true,
        doc: "style = (alpha, selected, edge, hovered)." },
    LayoutField { ts: "morph", rs: "MORPH", float_offset: 40, ts_byte: true,
        doc: "morph.x drives the GPU vertex blend." },
    LayoutField { ts: "material", rs: "MATERIAL", float_offset: 44, ts_byte: true,
        doc: "baseColor rgb at +0..2, metallic at +3." },
    LayoutField { ts: "metallic", rs: "METALLIC", float_offset: 47, ts_byte: false,
        doc: "metallic scalar." },
    LayoutField { ts: "emissive", rs: "EMISSIVE", float_offset: 48, ts_byte: true,
        doc: "emissive rgb at +0..2, roughness at +3." },
    LayoutField { ts: "roughness", rs: "ROUGHNESS", float_offset: 51, ts_byte: false,
        doc: "roughness scalar." },
    LayoutField { ts: "materialId", rs: "MATERIAL_ID", float_offset: 52, ts_byte: true,
        doc: "materialId scalar." },
];

pub const OBJECT_UNIFORM_FLOATS_VALUE: usize = 56;
pub const OBJECT_UNIFORM_BYTES_VALUE: u64 = 224;
