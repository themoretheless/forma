// @chunk scene_struct_short
// @chunk scene_binding

struct V { @builtin(position) p: vec4f, @location(0) c: vec4f, @location(1) w: vec3f }

@vertex fn vs(@location(0) pos: vec3f, @location(1) col: vec4f) -> V {
  var clip = sc.vp * vec4f(pos, 1);
  clip.z -= 0.00018 * clip.w;
  return V(clip, col, pos);
}

@fragment fn fs(v: V) -> @location(0) vec4f {
  // @chunk section_clip
  return v.c;
}
