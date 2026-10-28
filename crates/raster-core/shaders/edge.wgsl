// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding

struct EdgeV { @builtin(position) p: vec4f, @location(0) w: vec3f }

@vertex fn vs(@location(0) pos: vec3f, @location(1) fromPos: vec3f) -> EdgeV {
  let local = mix(fromPos, pos, ob.morph.x);
  let world = (ob.model * vec4f(local, 1)).xyz;
  var p = sc.vp * ob.model * vec4f(local, 1);
  p.z -= 0.00008 * p.w;
  return EdgeV(p, world);
}

@fragment fn fs(v: EdgeV) -> @location(0) vec4f {
  // @chunk section_clip
  // Edge accents stay brighter than the theme selection/hover surface tints
  // (they apply at full strength); only the base edge color is themed.
  let selected = mix(sc.edgeColor, vec3f(1.0, 0.55, 0.08), ob.style.y);
  let color = mix(selected, vec3f(0.1, 0.82, 1.0), ob.style.w);
  return vec4f(color, ob.style.z);
}
