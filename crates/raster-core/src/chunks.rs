//! Shared WGSL chunks and the `// @chunk` marker expansion.
//!
//! The `.wgsl` files in `crates/raster-core/shaders/` are the single source
//! of truth, but they no longer repeat the shared blocks (the `Scene`/`Obj`
//! structs, the uniform bindings, the shadow-map bindings, the PCF helper,
//! the section clip/cap statements). Instead they carry single-line markers:
//!
//! ```wgsl
//! // @chunk scene_struct
//! // @chunk obj_struct
//! // @chunk scene_binding
//! // @chunk obj_binding
//! // @chunk shadow_bindings group(2) bindings(0,1)
//! // @chunk shadow_pcf
//! // @chunk section_clip
//! // @chunk section_clip_inverted
//! // @chunk section_cap
//! ```
//!
//! A marker is a whole line whose trimmed text starts with `// @chunk `; the
//! expansion replaces the line with the canonical chunk text below (markers
//! may be indented — the chunk carries its own indentation). Only
//! `shadow_bindings` takes parameters, because the binding slots differ per
//! shader: mesh/meshToon bind the depth map at group(2) bindings 0/1,
//! meshPbr/meshMatcap share group(2) with the env/matcap at bindings 2/3, and
//! the grid uses group(1) bindings 0/1.
//!
//! [`expand_chunks`] is the single expansion implementation: the `wgsl_export`
//! codegen expands markers when emitting the TypeScript shader library, and
//! the native renderer/tests expand before handing text to wgpu/naga.

use std::borrow::Cow;

/// Canonical themed Scene struct (object shaders, the grid, and the shadow
/// pass share it). Mirrors `SCENE_UNIFORM_LAYOUT` field for field.
pub const SCENE_STRUCT: &str = "struct Scene { vp: mat4x4f, eye: vec4f, light: vec4f, ambient: vec4f, section: vec4f, options: vec4f, inverseVP: mat4x4f, selectionColor: vec3f, hoverColor: vec3f, edgeColor: vec3f, xrayColor: vec3f, gridColor: vec3f, capColor: vec3f, lightVP: mat4x4f, shadowParams: vec4f }";

/// Short legacy Scene struct: line and selection-overlay shaders never read
/// past `options`, so they keep the compact declaration.
pub const SCENE_STRUCT_SHORT: &str =
    "struct Scene { vp: mat4x4f, eye: vec4f, light: vec4f, ambient: vec4f, section: vec4f, options: vec4f }";

/// Canonical Obj struct (one per-object uniform record).
pub const OBJ_STRUCT: &str = "struct Obj { model: mat4x4f, nmat: mat4x4f, color: vec4f, style: vec4f, morph: vec4f, baseColor: vec3f, metallic: f32, emissive: vec3f, roughness: f32, materialId: f32 }";

/// Scene uniform binding (always group(0) binding(0)).
pub const SCENE_BINDING: &str = "@group(0) @binding(0) var<uniform> sc: Scene;";

/// Obj uniform binding (always group(1) binding(0)).
pub const OBJ_BINDING: &str = "@group(1) @binding(0) var<uniform> ob: Obj;";

/// Section-plane discard shared by object/overlay fragment shaders (indented
/// for the fragment body it sits in).
pub const SECTION_CLIP: &str =
    "  if (sc.options.x > 0.5 && dot(v.w, sc.section.xyz) < sc.section.w) { discard; }";

/// Inverted discard of the section-cap pass: keeps only the clipped side.
pub const SECTION_CLIP_INVERTED: &str =
    "  if (sc.options.x > 0.5 && dot(v.w, sc.section.xyz) >= sc.section.w) { discard; }";

/// Epsilon accent shared by the mesh-surface shaders: fragments just inside
/// the clip plane (within a fixed world-space epsilon) shade flat and unlit
/// with a tint toward the theme cap color.
pub const SECTION_CAP: &str = "  if (sc.options.x > 0.5 && dot(v.w, sc.section.xyz) < sc.section.w + 0.02) { return vec4f(mix(ob.baseColor, sc.capColor, 0.6) + ob.emissive * 0.2, ob.style.x); }";

/// PCF 3x3 visibility of the key light at a world position; 1 when shadows
/// are disabled or the point lies outside the light frustum. The two comment
/// lines are part of the chunk so the expansion is byte-identical to the
/// historical per-shader copies.
pub const SHADOW_PCF: &str = "\
// PCF 3x3 visibility of the key light at a world position; 1 when shadows
// are disabled or the point lies outside the light frustum.
fn shadowFactor(wp: vec3f) -> f32 {
  if (sc.shadowParams.x < 0.5) { return 1.0; }
  let lp = sc.lightVP * vec4f(wp, 1.0);
  let ndc = lp.xyz / lp.w;
  let uv = ndc.xy * vec2f(0.5, -0.5) + vec2f(0.5);
  let inb = ndc.z >= 0.0 && ndc.z <= 1.0 && uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
  let texel = sc.shadowParams.y;
  let depth = ndc.z - sc.shadowParams.z;
  var sum = 0.0;
  for (var dy = -1; dy <= 1; dy++) {
    for (var dx = -1; dx <= 1; dx++) {
      sum += textureSampleCompare(shadowMap, shadowSampler, clamp(uv, vec2f(0.0), vec2f(1.0)) + vec2f(f32(dx) * texel, f32(dy) * texel), depth);
    }
  }
  return select(1.0, mix(1.0, sum / 9.0, sc.shadowParams.w), inb);
}";

/// Shadow-map binding pair at the parameterizable slots.
fn shadow_bindings(args: &str) -> String {
    let mut group: Option<&str> = None;
    let mut bindings: Option<(&str, &str)> = None;
    for token in args.split_whitespace() {
        if let Some(value) = token.strip_prefix("group(").and_then(|t| t.strip_suffix(')')) {
            group = Some(value);
        } else if let Some(value) = token.strip_prefix("bindings(").and_then(|t| t.strip_suffix(')')) {
            let (map, sampler) = value
                .split_once(',')
                .unwrap_or_else(|| panic!("@chunk shadow_bindings: malformed bindings({value})"));
            bindings = Some((map, sampler));
        } else {
            panic!("@chunk shadow_bindings: unexpected argument '{token}'");
        }
    }
    let (group, (map, sampler)) = match (group, bindings) {
        (Some(group), Some(bindings)) => (group, bindings),
        _ => panic!("@chunk shadow_bindings: expected group(N) bindings(M,S)"),
    };
    format!(
        "@group({group}) @binding({map}) var shadowMap: texture_depth_2d;\n@group({group}) @binding({sampler}) var shadowSampler: sampler_comparison;"
    )
}

/// Resolves one marker (the text after `// @chunk `) to its expansion.
fn chunk_text(marker: &str) -> Cow<'static, str> {
    let mut parts = marker.split_whitespace();
    let name = parts.next().unwrap_or_else(|| panic!("empty @chunk marker"));
    let args_start = marker[name.len()..].trim_start();
    let has_args = !args_start.is_empty();
    let no_args = |text: &'static str| -> Cow<'static, str> {
        assert!(!has_args, "@chunk {name}: unexpected arguments '{args_start}'");
        Cow::Borrowed(text)
    };
    match name {
        "scene_struct" => no_args(SCENE_STRUCT),
        "scene_struct_short" => no_args(SCENE_STRUCT_SHORT),
        "obj_struct" => no_args(OBJ_STRUCT),
        "scene_binding" => no_args(SCENE_BINDING),
        "obj_binding" => no_args(OBJ_BINDING),
        "section_clip" => no_args(SECTION_CLIP),
        "section_clip_inverted" => no_args(SECTION_CLIP_INVERTED),
        "section_cap" => no_args(SECTION_CAP),
        "shadow_pcf" => no_args(SHADOW_PCF),
        "shadow_bindings" => Cow::Owned(shadow_bindings(args_start)),
        _ => panic!("unknown @chunk marker '{name}'"),
    }
}

/// Expands every `// @chunk` marker line in `source`. Returns the input
/// unchanged (borrowed) when no markers are present, so shaders without
/// chunks cost nothing.
pub fn expand_chunks(source: &str) -> Cow<'_, str> {
    if !source.contains("@chunk") {
        return Cow::Borrowed(source);
    }
    let mut out = String::with_capacity(source.len() + 256);
    for (index, line) in source.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let trimmed = line.trim();
        match trimmed.strip_prefix("// @chunk ") {
            Some(marker) => out.push_str(&chunk_text(marker)),
            None => out.push_str(line),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_is_line_based_and_preserves_everything_else() {
        let source = "head\n  // @chunk section_clip\ntail\n";
        assert_eq!(
            expand_chunks(source).as_ref(),
            format!("head\n{}\ntail\n", SECTION_CLIP).as_str()
        );
    }

    #[test]
    fn sources_without_markers_are_borrowed() {
        let source = "struct Scene { vp: mat4x4f }";
        assert!(matches!(expand_chunks(source), Cow::Borrowed(_)));
    }

    #[test]
    fn shadow_bindings_parameterize_group_and_slots() {
        assert_eq!(
            chunk_text("shadow_bindings group(2) bindings(2,3)").as_ref(),
            "@group(2) @binding(2) var shadowMap: texture_depth_2d;\n@group(2) @binding(3) var shadowSampler: sampler_comparison;"
        );
    }

    #[test]
    #[should_panic(expected = "unknown @chunk marker")]
    fn unknown_markers_panic() {
        expand_chunks("// @chunk nope");
    }
}
