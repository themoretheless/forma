// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding

struct V { @builtin(position) p: vec4f, @location(0) w: vec3f, @location(1) n: vec3f }

@vertex fn vs(@location(0) pos: vec3f, @location(1) norm: vec3f, @location(2) fromPos: vec3f) -> V {
  let local = mix(fromPos, pos, ob.morph.x);
  let world = (ob.model * vec4f(local, 1)).xyz;
  let wn = normalize((ob.nmat * vec4f(norm, 0)).xyz);
  return V(sc.vp * vec4f(world, 1), world, wn);
}

@fragment fn fs(v: V) -> @location(0) vec4f {
  // @chunk section_clip
  // Fresnel-weighted translucency: grazing angles (silhouette edges) glow,
  // face-on fragments stay clear so the geometry behind reads through.
  let N = normalize(v.n);
  let V2 = normalize(sc.eye.xyz - v.w);
  let fresnel = pow(1.0 - clamp(abs(dot(N, V2)), 0.0, 1.0), 2.0);
  let glow = sc.xrayColor * (0.85 + 0.55 * fresnel);
  return vec4f(glow, mix(0.1, 0.55, fresnel));
}
