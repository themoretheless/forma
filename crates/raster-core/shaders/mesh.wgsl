// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding
// Contact shadow map (key-light depth), bound per renderer at group(2). The
// renderer always binds something: with shadowParams.x == 0 the factor below
// is 1, so the default look is pixel-identical to the pre-shadow one.
// @chunk shadow_bindings group(2) bindings(0,1)

struct V { @builtin(position) p: vec4f, @location(0) n: vec3f, @location(1) w: vec3f }

// @chunk shadow_pcf

@vertex fn vs(@location(0) pos: vec3f, @location(1) norm: vec3f, @location(2) fromPos: vec3f) -> V {
  let local = mix(fromPos, pos, ob.morph.x);
  let wp = (ob.model * vec4f(local,1)).xyz;
  let wn = normalize((ob.nmat * vec4f(norm,0)).xyz);
  return V(sc.vp * vec4f(wp,1), wn, wp);
}
@fragment fn fs(v: V) -> @location(0) vec4f {
  // @chunk section_clip
  // Cheap section-cap approximation: fragments just inside the clip plane
  // (within a fixed world-space epsilon) shade flat/unlit to suggest the cut.
  // Shadows darken the ambient (indirect) term only; the key light stays.
  // Hoisted above the section-cap early return: textureSampleCompare must run
  // in uniform control flow (WGSL uniformity analysis).
  let shadow = shadowFactor(v.w);
  // @chunk section_cap
  let N = normalize(v.n);
  let L = normalize(sc.light.xyz);
  let V2 = normalize(sc.eye.xyz - v.w);
  let H = normalize(L + V2);
  let d = max(dot(N, L), 0.0);
  let s = pow(max(dot(N, H), 0.0), 40.0);
  let bd = max(dot(-N, L), 0.0) * 0.25;
  let selected = mix(ob.color.rgb, sc.selectionColor, ob.style.y * 0.48);
  let base = mix(selected, sc.hoverColor, ob.style.w * 0.38) * ob.baseColor;
  // Defaults (baseColor white, metallic 0, emissive black) reproduce the
  // legacy Blinn-Phong look exactly; roughness/materialId are reserved.
  let c = sc.ambient.rgb * base * shadow + d * base + s * vec3f(0.25) * (1.0 - ob.metallic) + bd * base * 0.5 + ob.emissive;
  return vec4f(c, ob.style.x);
}
