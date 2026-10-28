// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding
// Contact shadow map (key-light depth), bound per renderer at group(2). With
// shadowParams.x == 0 the factor below is 1: the default look is unchanged.
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
  // Cheap section-cap approximation (world-space epsilon highlight).
  // Shadows darken the ambient (indirect) term only; the toon steps stay.
  // Hoisted above the section-cap early return: textureSampleCompare must run
  // in uniform control flow (WGSL uniformity analysis).
  let shadow = shadowFactor(v.w);
  // @chunk section_cap
  let N = normalize(v.n);
  let L = normalize(sc.light.xyz);
  let V2 = normalize(sc.eye.xyz - v.w);
  let selected = mix(ob.color.rgb, sc.selectionColor, ob.style.y * 0.48);
  let base = mix(selected, sc.hoverColor, ob.style.w * 0.38) * ob.baseColor;
  // Quantized (4-step) diffuse for the technical-illustration look.
  let d = max(dot(N, L), 0.0);
  let step = clamp(floor(d * 4.0) / 4.0 + 0.12, 0.0, 1.0);
  // Fresnel rim darkening suggests an outline without a second pass.
  let rim = pow(1.0 - clamp(abs(dot(N, V2)), 0.0, 1.0), 3.0);
  let outline = 1.0 - smoothstep(0.55, 0.95, rim) * 0.85;
  let c = base * (sc.ambient.rgb * 0.6 * shadow + step) * outline + ob.emissive;
  return vec4f(c, ob.style.x);
}
