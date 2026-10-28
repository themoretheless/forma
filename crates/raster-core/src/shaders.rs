//! WGSL sources, embedded at compile time. These files are the single source
//! of truth: the browser renderer consumes generated copies, native wgpu
//! consumes these strings after chunk expansion.
//!
//! The raw sources carry `// @chunk` markers for the shared blocks (see
//! [`crate::chunks`]); [`ShaderEntry::expanded`] and [`expand_chunks`] produce
//! the final WGSL text, and that expanded text is the ONLY text fit for
//! naga/wgpu or TS emission — the raw consts still contain comment markers.

/// One declarative row per shipped shader. The `include_str!` consts,
/// [`OBJECT_SHADERS`], and codegen's `TS_SOURCES` all derive from
/// [`SHADER_TABLE`], so ids, docs, and membership cannot drift apart.
pub struct ShaderEntry {
    /// Stable snake_case id (diagnostics, tests, variant golden keys).
    pub id: &'static str,
    /// TypeScript export name in the generated sources module.
    pub ts_name: &'static str,
    /// Doc comment emitted above the TS export.
    pub doc: &'static str,
    /// True when the shader declares the shared `Obj` uniform and morph blend.
    pub is_object: bool,
    /// Raw WGSL source (may contain `// @chunk` markers).
    pub source: &'static str,
}

impl ShaderEntry {
    /// Final WGSL text with every `// @chunk` marker expanded.
    pub fn expanded(&self) -> std::borrow::Cow<'static, str> {
        crate::chunks::expand_chunks(self.source)
    }
}

macro_rules! declare_shaders {
    ($($id:literal => $name:ident = $file:literal, $doc:literal, $is_object:expr);* $(;)?) => {
        $(pub const $name: &str = include_str!(concat!("../shaders/", $file));)*

        /// Every shipped shader, in stable emission order.
        pub const SHADER_TABLE: &[ShaderEntry] = &[
            $(ShaderEntry { id: $id, ts_name: stringify!($name), doc: $doc, is_object: $is_object, source: $name },)*
        ];
    };
}

declare_shaders! {
    "mesh" => MESH_WGSL = "mesh.wgsl",
        "Lit opaque/transparent mesh surface with per-object style and GPU morph blend.", true;
    "mesh_pbr" => MESH_PBR_WGSL = "mesh_pbr.wgsl",
        "Cook-Torrance PBR mesh surface (GGX/Smith/Schlick) driven by the Obj material tail.", true;
    "mesh_matcap" => MESH_MATCAP_WGSL = "mesh_matcap.wgsl",
        "Matcap mesh surface: samples a bound capture texture when present, else procedural studio key + rim + specular blob.", true;
    "mesh_toon" => MESH_TOON_WGSL = "mesh_toon.wgsl",
        "Toon/technical-illustration mesh surface (quantized diffuse + fresnel outline).", true;
    "mesh_unlit" => MESH_UNLIT_WGSL = "mesh_unlit.wgsl",
        "Unlit mesh surface: tinted base color plus emission only.", true;
    "mesh_section_cap" => MESH_SECTION_CAP_WGSL = "mesh_section_cap.wgsl",
        "Stencil-free section cap: inverted clip + front-face culling fills the cut surface flat with the theme cap color.", true;
    "deep_mesh" => DEEP_MESH_WGSL = "deep_mesh.wgsl",
        "X-ray deep-selection mesh (depth Always, fresnel-weighted translucency).", true;
    "edge" => EDGE_WGSL = "edge.wgsl",
        "Per-mesh wireframe edges with selection/hover tinting.", true;
    "line" => LINE_WGSL = "line.wgsl",
        "Plain colored line list (measurements, grid axes).", false;
    "grid" => GRID_WGSL = "grid.wgsl",
        "Full-viewport XY grid, reconstructed from camera rays with adaptive spacing.", false;
    "selection_overlay" => SELECTION_OVERLAY_WGSL = "selection_overlay.wgsl",
        "Per-vertex colored overlay for source-face highlighting.", false;
    "mesh_shadow" => MESH_SHADOW_WGSL = "mesh_shadow.wgsl",
        "Depth-only key-light shadow pass: mesh vertex contract, no fragment stage, writes the shadow map.", false;
}

const OBJECT_SHADER_COUNT: usize = 8;

/// Shaders that declare the shared `Obj` uniform and morph blend, filtered
/// from [`SHADER_TABLE`]. The first element is a stable name used in
/// diagnostics and tests.
pub const OBJECT_SHADERS: [(&str, &str); OBJECT_SHADER_COUNT] = {
    let mut out: [(&str, &str); OBJECT_SHADER_COUNT] = [("", ""); OBJECT_SHADER_COUNT];
    let mut table_index = 0;
    let mut object_index = 0;
    while table_index < SHADER_TABLE.len() {
        let entry = &SHADER_TABLE[table_index];
        if entry.is_object {
            out[object_index] = (entry.id, entry.source);
            object_index += 1;
        }
        table_index += 1;
    }
    assert!(object_index == OBJECT_SHADER_COUNT, "OBJECT_SHADER_COUNT drifted from SHADER_TABLE");
    out
};

/// Vertex input locations shared by the object pipelines: slot 0 is the
/// interleaved position+normal mesh vertex (24 bytes), slot 1 is the morph
/// source positions (12 bytes, zeros while a mesh is not morphing).
pub const MESH_VERTEX_STRIDE: u64 = 24;
pub const MORPH_VERTEX_STRIDE: u64 = 12;

/// Per-instance storage record layout (used by the instanced pipelines):
/// the same 56 floats as [`crate::uniform::ObjectUniform`].
pub const INSTANCE_RECORD_FLOATS: usize = 56;
pub const INSTANCE_RECORD_BYTES: u64 = 224;
