// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding

struct V { @builtin(position) p: vec4f, @location(0) n: vec3f, @location(1) w: vec3f }

// Vertex stage identical to mesh.wgsl (same layout, morph blend, transforms).
@vertex fn vs(@location(0) pos: vec3f, @location(1) norm: vec3f, @location(2) fromPos: vec3f) -> V {
  let local = mix(fromPos, pos, ob.morph.x);
  let wp = (ob.model * vec4f(local,1)).xyz;
  let wn = normalize((ob.nmat * vec4f(norm,0)).xyz);
  return V(sc.vp * vec4f(wp,1), wn, wp);
}
@fragment fn fs(v: V) -> @location(0) vec4f {
  // Inverted clip test: the surface pass keeps dot(w, n) >= w, so this pass —
  // drawn with front-face culling right after it — keeps only the clipped
  // side's back faces. For a closed solid those interior back faces read as a
  // filled, unlit cut surface (stencil-free section cap).
  // @chunk section_clip_inverted
  return vec4f(sc.capColor, 1.0);
}
