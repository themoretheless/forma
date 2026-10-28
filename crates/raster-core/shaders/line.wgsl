// @chunk scene_struct_short
// @chunk scene_binding

struct V { @builtin(position) p: vec4f, @location(0) c: vec4f }

@vertex fn vs(@location(0) pos: vec3f, @location(1) col: vec4f) -> V {
  return V(sc.vp * vec4f(pos,1), col);
}
@fragment fn fs(v: V) -> @location(0) vec4f { return v.c; }
