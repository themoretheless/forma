//! Codegen: exports the WGSL sources to the TypeScript shader library.
//!
//! `crates/raster-core/shaders/*.wgsl` is the single source of truth. The
//! browser renderer consumes generated copies of these exact texts; this
//! module renders `src/services/shaders/generated/sources.ts` and the
//! `wgsl_export` binary writes it to disk. A `--check` mode and the
//! `generated_ts_matches_the_wgsl_sources` test keep the checked-in file in
//! lockstep with the WGSL sources.
//!
//! Raw sources carry `// @chunk` markers (see [`crate::chunks`]); everything
//! emitted here is the EXPANDED text. The same codegen also renders the
//! uniform-layout mirrors (`generated/layouts.ts` and
//! `crates/raster-core/src/generated_layouts.rs`) from the single declarative
//! table in [`crate::layout`].

use crate::chunks::expand_chunks;
use crate::shaders::SHADER_TABLE;

/// (export name, WGSL source, doc comment) in stable emission order, derived
/// from the single declarative table in `shaders.rs`.
pub const TS_SOURCES: [(&str, &str, &str); 12] = {
    let mut out: [(&str, &str, &str); 12] = [("", "", ""); 12];
    let mut index = 0;
    while index < SHADER_TABLE.len() {
        let entry = &SHADER_TABLE[index];
        out[index] = (entry.ts_name, entry.source, entry.doc);
        index += 1;
    }
    assert!(index == 12, "TS_SOURCES length drifted from SHADER_TABLE");
    out
};

const HEADER: &str = "\
// GENERATED FILE — do not edit.
// Generated from crates/raster-core/shaders/*.wgsl (the single source of truth)
// by the raster-core `wgsl_export` codegen. Regenerate:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export
// Verify:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export -- --check
";

/// Escapes a WGSL source for inclusion in a TS template literal.
fn ts_escape(source: &str) -> String {
    source.replace('\\', "\\\\").replace('`', "\\`").replace("${", "\\${")
}

/// Renders the full `sources.ts` content. Shader bodies are emitted with
/// every `// @chunk` marker expanded; the canonical chunk texts are appended
/// as named exports so TypeScript consumers (tests, composers) never
/// duplicate them.
pub fn generate_ts_sources() -> String {
    let mut out = String::from(HEADER);
    for (name, source, doc) in TS_SOURCES {
        out.push_str(&format!("\n/** {doc} */\nexport const {name} = /* wgsl */`\n"));
        // Emit without the leading/trailing blank lines the .wgsl files carry.
        out.push_str(&ts_escape(expand_chunks(source).trim()));
        out.push_str("`\n");
    }
    out.push_str(
        "\n/* ── Shared WGSL chunks (canonical text: crates/raster-core/src/chunks.rs) ── */\n",
    );
    let chunk_exports: [(&str, &str, &str); 4] = [
        ("SCENE_STRUCT", crate::chunks::SCENE_STRUCT,
         "Canonical themed Scene struct (object shaders, the grid, and the shadow pass share it)."),
        ("OBJ_STRUCT", crate::chunks::OBJ_STRUCT,
         "Canonical Obj struct (one per-object uniform record)."),
        ("SECTION_CLIP_WGSL", crate::chunks::SECTION_CLIP.trim(),
         "Section-plane discard shared by object/overlay fragment shaders."),
        ("SECTION_CAP_WGSL", crate::chunks::SECTION_CAP.trim(),
         "Epsilon accent: fragments just inside the clip plane shade flat, tinted toward the theme cap color."),
    ];
    for (name, text, doc) in chunk_exports {
        out.push_str(&format!("\n/** {doc} */\nexport const {name} = /* wgsl */`"));
        out.push_str(&ts_escape(text));
        out.push_str("`\n");
    }
    out
}

/// Repo-relative path of the generated TS module, resolved against the crate
/// manifest directory (`crates/raster-core`).
pub fn generated_ts_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/services/shaders/generated/sources.ts")
}

/// Writes the generated TS module to disk, creating parent directories.
pub fn write_ts_sources() -> std::io::Result<std::path::PathBuf> {
    let path = generated_ts_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, generate_ts_sources())?;
    Ok(path)
}

/// (golden key, variant text) pairs covering both transforms over every
/// object shader. The TypeScript `variants.ts` is hand-written; the checked-in
/// golden pins it to the Rust implementations byte-for-byte. Transforms run
/// over the canonical TS const value (`"\n" + trim(source)`, the template
/// literal convention of the generated sources module) so both languages
/// consume identical input text.
pub fn variant_goldens() -> Vec<(String, String)> {
    use crate::variants::{VertexOutput, immediate_object_shader, instanced_object_shader};
    let mut goldens = Vec::new();
    for (name, source) in crate::shaders::OBJECT_SHADERS {
        let expanded = expand_chunks(source);
        let ts_source = format!("\n{}", expanded.trim());
        goldens.push((format!("immediate:{name}"), immediate_object_shader(&ts_source)));
        let output = if name == "edge" { VertexOutput::EdgeV } else { VertexOutput::V };
        goldens.push((format!("instanced:{name}"), instanced_object_shader(&ts_source, output)));
    }
    goldens
}

const VARIANTS_HEADER: &str = "\
// GENERATED FILE — do not edit.
// Golden outputs of crates/raster-core/src/variants.rs (immediate/instanced
// shader transforms), used by vitest to pin the hand-written TypeScript
// variants.ts to the Rust implementations. Regenerate with `wgsl_export`.
";

/// Renders the variant golden TS module.
pub fn generate_variant_goldens_ts() -> String {
    let mut out = String::from(VARIANTS_HEADER);
    out.push_str("\nexport const VARIANTS_GOLDEN: Record<string, string> = {\n");
    for (key, text) in variant_goldens() {
        out.push_str(&format!("  '{key}': /* wgsl */`"));
        out.push_str(&ts_escape(&text));
        out.push_str("`,\n");
    }
    out.push_str("}\n");
    out
}

/// Repo-relative path of the generated variant golden module.
pub fn variant_goldens_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/services/shaders/generated/variants.golden.ts")
}

/// Writes the variant golden module next to the generated sources.
pub fn write_variant_goldens() -> std::io::Result<std::path::PathBuf> {
    let path = variant_goldens_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, generate_variant_goldens_ts())?;
    Ok(path)
}

/* ── Uniform layout codegen (item 8: one table, two languages) ─────────── */

const LAYOUTS_TS_HEADER: &str = "\
// GENERATED FILE — do not edit.
// Generated from crates/raster-core/src/layout.rs (the single source of truth
// for the Scene/Obj uniform layouts) by the raster-core `wgsl_export` codegen.
// Regenerate:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export
// Verify:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export -- --check
";

const LAYOUTS_RS_HEADER: &str = "\
// GENERATED FILE — do not edit.
// Generated from crates/raster-core/src/layout.rs (the single source of truth
// for the Scene/Obj uniform layouts) by the raster-core `wgsl_export` codegen.
// Regenerate:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export
// Verify:
//   cargo run --manifest-path crates/Cargo.toml -p raster-core --bin wgsl_export -- --check
";

fn generate_layout_ts_object(name: &str, doc: &str, floats: usize, bytes: u64, fields: &[crate::layout::LayoutField]) -> String {
    let mut out = format!("\n/**\n * {doc}\n */\nexport const {name} = {{\n  floats: {floats},\n  bytes: {bytes},\n");
    for field in fields {
        out.push_str(&format!("  /** {} */\n", field.doc));
        out.push_str(&format!("  {}FloatOffset: {},\n", field.ts, field.float_offset));
        if field.ts_byte {
            out.push_str(&format!("  {}ByteOffset: {},\n", field.ts, field.float_offset * 4));
        }
    }
    out.push_str("} as const\n");
    out
}

/// Renders `src/services/shaders/generated/layouts.ts`: the browser-side
/// `SCENE_UNIFORM_LAYOUT`/`OBJECT_UNIFORM_LAYOUT` objects.
pub fn generate_layouts_ts() -> String {
    let mut out = String::from(LAYOUTS_TS_HEADER);
    out.push_str(&generate_layout_ts_object(
        "SCENE_UNIFORM_LAYOUT",
        "CPU-side mirror of the Scene uniform struct: vp (16 floats) + eye (4)\n * + light (4) + ambient (4) + section (4) + options (4) + inverseVP (16)\n * + theme block (6 colors × vec3+pad = 24) + shadow block (lightVP 16\n * + shadowParams 4). shadowParams = (enabled, 1/mapSize, depth bias,\n * strength); enabled 0 keeps every sampling shader on its unshadowed path.",
        crate::layout::SCENE_UNIFORM_FLOATS_VALUE,
        crate::layout::SCENE_UNIFORM_BYTES_VALUE,
        crate::layout::SCENE_FIELDS,
    ));
    out.push_str(&generate_layout_ts_object(
        "OBJECT_UNIFORM_LAYOUT",
        "CPU-side mirror of the Obj uniform struct: model (16 floats) + nmat (16)\n * + color (4) + style (4) + morph (4) + baseColor (3) + metallic (1)\n * + emissive (3) + roughness (1) + materialId (1) + pad (3).",
        crate::layout::OBJECT_UNIFORM_FLOATS_VALUE,
        crate::layout::OBJECT_UNIFORM_BYTES_VALUE,
        crate::layout::OBJECT_FIELDS,
    ));
    out
}

fn generate_layout_rs_consts(floats: usize, bytes: u64, floats_name: &str, bytes_name: &str, fields: &[crate::layout::LayoutField]) -> String {
    let mut out = format!("\npub const {floats_name}: usize = {floats};\npub const {bytes_name}: u64 = {bytes};\n");
    for field in fields {
        out.push_str(&format!("/// {}\n", field.doc));
        out.push_str(&format!("pub const {}_FLOAT_OFFSET: usize = {};\n", field.rs, field.float_offset));
        out.push_str(&format!("pub const {}_BYTE_OFFSET: u64 = {};\n", field.rs, field.float_offset * 4));
    }
    out
}

/// Renders `crates/raster-core/src/generated_layouts.rs`: the native mirror
/// constants re-exported by `uniform.rs`.
pub fn generate_layouts_rs() -> String {
    let mut out = String::from(LAYOUTS_RS_HEADER);
    out.push_str(&generate_layout_rs_consts(
        crate::layout::SCENE_UNIFORM_FLOATS_VALUE,
        crate::layout::SCENE_UNIFORM_BYTES_VALUE,
        "SCENE_UNIFORM_FLOATS",
        "SCENE_UNIFORM_BYTES",
        crate::layout::SCENE_FIELDS,
    ));
    out.push_str(&generate_layout_rs_consts(
        crate::layout::OBJECT_UNIFORM_FLOATS_VALUE,
        crate::layout::OBJECT_UNIFORM_BYTES_VALUE,
        "OBJECT_UNIFORM_FLOATS",
        "OBJECT_UNIFORM_BYTES",
        crate::layout::OBJECT_FIELDS,
    ));
    out
}

/// Repo-relative path of the generated TS layouts module.
pub fn layouts_ts_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/services/shaders/generated/layouts.ts")
}

/// Repo-relative path of the generated Rust layouts module.
pub fn layouts_rs_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/generated_layouts.rs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_module_exports_every_source_as_template_literal() {
        let ts = generate_ts_sources();
        for (name, source, _doc) in TS_SOURCES {
            assert!(ts.contains(&format!("export const {name} = /* wgsl */`")), "missing export {name}");
            let expanded = expand_chunks(source);
            assert!(ts.contains(expanded.trim()), "{name} body drifted from the expanded WGSL source");
        }
        // TS template-literal hazards are escaped.
        assert!(!ts.contains("${"));
    }
}
