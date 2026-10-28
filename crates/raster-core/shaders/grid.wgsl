// @chunk scene_struct
// @chunk scene_binding
// Contact shadow map (key-light depth), bound per renderer at group(1). With
// shadowParams.x == 0 the factor below is 1: the default grid is unchanged.
// @chunk shadow_bindings group(1) bindings(0,1)

// @chunk shadow_pcf

struct V { @builtin(position) p: vec4f, @location(0) near: vec4f, @location(1) far: vec4f }
struct F { @location(0) color: vec4f, @builtin(frag_depth) depth: f32 }

@vertex fn vs(@location(0) corner: vec2f) -> V {
  return V(vec4f(corner, 0.0, 1.0),
    sc.inverseVP * vec4f(corner, 0.0, 1.0),
    sc.inverseVP * vec4f(corner, 1.0, 1.0));
}

fn lineMask(coord: vec2f, width: vec2f) -> f32 {
  let g = abs(fract(coord - 0.5) - 0.5) / max(width, vec2f(0.000001));
  return 1.0 - min(min(g.x, g.y), 1.0);
}

@fragment fn fs(v: V) -> F {
  let near = v.near.xyz / v.near.w;
  let far = v.far.xyz / v.far.w;
  let ray = far - near;
  let dz = select(-1.0, 1.0, ray.z >= 0.0) * max(abs(ray.z), 0.000001);
  let t = -near.z / dz;
  let world = near + t * ray;
  let pixelWidth = max(fwidth(world.xy), vec2f(0.000001));
  // Blend decade levels so distant areas retain a readable grid instead of
  // losing both fixed levels. Derivatives precede all non-uniform discards.
  let level = max(0.0, log2(max(pixelWidth.x, pixelWidth.y) * 8.0 / sc.options.y) / log2(10.0));
  let step = sc.options.y * pow(10.0, floor(level));
  let blend = fract(level);
  let minor = lineMask(world.xy / step, pixelWidth / step);
  let major = lineMask(world.xy / (step * 10.0), pixelWidth / (step * 10.0));
  let coarse = lineMask(world.xy / (step * 100.0), pixelWidth / (step * 100.0));
  var alpha = max(max(minor * 0.28 * (1.0 - blend), major * mix(0.55, 0.28, blend)), coarse * 0.55 * blend);
  var color = sc.gridColor;
  // Ground contact shadow: darken the floor where the model occludes the key
  // light. Nudged off the plane along the light to avoid self-banding.
  let shadow = shadowFactor(vec3f(world.xy, 0.02));
  color = color * shadow;
  let axisW = pixelWidth * 1.2;
  if (abs(world.y) < axisW.y) { color = vec3f(0.95, 0.18, 0.16); alpha = max(alpha, 0.9); }
  if (abs(world.x) < axisW.x) { color = vec3f(0.2, 0.85, 0.25); alpha = max(alpha, 0.9); }
  let clip = sc.vp * vec4f(world.xy, 0.0, 1.0);
  let depth = clip.z / clip.w;
  if (abs(ray.z) < 0.000001 || t < 0.0 || depth < 0.0 || alpha < 0.004) { discard; }
  // Keep the distant grid as background even beyond the model clipping range.
  return F(vec4f(color, alpha), min(depth, 0.999999));
}
