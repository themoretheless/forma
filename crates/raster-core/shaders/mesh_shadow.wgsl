// Depth-only shadow pass: renders mesh depth from the key light's
// orthographic view into the shadow map. Same vertex contract as the mesh
// surface shaders (interleaved pos+normal slot 0, morph source slot 1,
// Scene + Obj uniforms); no fragment stage — the pipeline has no color
// targets and writes depth32float only. sc.lightVP is the light-space
// view-projection built on the CPU per frame.
// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding

@vertex fn vs(@location(0) pos: vec3f, @location(1) norm: vec3f, @location(2) fromPos: vec3f) -> @builtin(position) vec4f {
  let local = mix(fromPos, pos, ob.morph.x);
  return sc.lightVP * (ob.model * vec4f(local, 1.0));
}
