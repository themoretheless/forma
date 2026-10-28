// @chunk scene_struct
// @chunk obj_struct
// @chunk scene_binding
// @chunk obj_binding
// Equirectangular environment map, bound per renderer (not per object). The
// renderer always binds something: a 1x1 dummy selects the analytic-lighting
// fallback below (textureDimensions == 1), a real map switches to the IBL
// path. Same binding contract as mesh_matcap.wgsl's capture.
@group(2) @binding(0) var envTex: texture_2d<f32>;
@group(2) @binding(1) var envSampler: sampler;
// Contact shadow map (key-light depth), sharing the environment group. With
// shadowParams.x == 0 the factor below is 1: the default look is unchanged.
// @chunk shadow_bindings group(2) bindings(2,3)

struct V { @builtin(position) p: vec4f, @location(0) n: vec3f, @location(1) w: vec3f }

// @chunk shadow_pcf

@vertex fn vs(@location(0) pos: vec3f, @location(1) norm: vec3f, @location(2) fromPos: vec3f) -> V {
  let local = mix(fromPos, pos, ob.morph.x);
  let wp = (ob.model * vec4f(local,1)).xyz;
  let wn = normalize((ob.nmat * vec4f(norm,0)).xyz);
  return V(sc.vp * vec4f(wp,1), wn, wp);
}

fn fresnelSchlick(cosTheta: f32, F0: vec3f) -> vec3f {
  return F0 + (vec3f(1.0) - F0) * pow(clamp(1.0 - cosTheta, 0.0, 1.0), 5.0);
}
fn distributionGGX(N: vec3f, H: vec3f, rough: f32) -> f32 {
  let a = max(rough * rough, 0.001);
  let a2 = a * a;
  let ndh = max(dot(N, H), 0.0);
  let denom = ndh * ndh * (a2 - 1.0) + 1.0;
  return a2 / (3.14159265 * denom * denom);
}
fn geometrySchlickGGX(ndv: f32, rough: f32) -> f32 {
  let r = rough + 1.0;
  let k = (r * r) / 8.0;
  return ndv / (ndv * (1.0 - k) + k);
}
fn geometrySmith(N: vec3f, V2: vec3f, L: vec3f, rough: f32) -> f32 {
  return geometrySchlickGGX(max(dot(N, V2), 0.0), rough)
    * geometrySchlickGGX(max(dot(N, L), 0.0), rough);
}

// Equirectangular lookup in the viewer's Z-up world: u wraps the azimuth
// around +Z, v = 0 is the +Z zenith, which is the PNG's top row (uploaded
// without flipY). On the pole axis the azimuth is undefined (atan2(0, 0));
// any u is correct there, so it is pinned instead of left to the backend.
fn envUv(dir: vec3f) -> vec2f {
  let d = normalize(dir);
  let azimuth = select(atan2(d.y, d.x), 0.0, abs(d.x) + abs(d.y) < 1e-6);
  let u = azimuth / 6.2831853 + 0.5;
  let v = acos(clamp(d.z, -1.0, 1.0)) / 3.14159265;
  return vec2f(u, v);
}
fn envSample(dir: vec3f, lod: f32) -> vec3f {
  return textureSampleLevel(envTex, envSampler, envUv(dir), lod).rgb;
}
// Diffuse irradiance approximation: a 5-tap cone average around N. Equirect
// textures carry no prefiltered irradiance map, and mips are not generated,
// so the taps integrate a small hemisphere cap cheaply (offset ~0.55 rad in
// tangent space around the normal; a wide soft cap matches lambert well for
// the smooth studio/sky maps shipped in public/env/).
fn envIrradiance(N: vec3f) -> vec3f {
  let upRef = select(vec3f(0.0, 1.0, 0.0), vec3f(0.0, 0.0, 1.0), abs(N.y) > 0.99);
  let t = normalize(cross(upRef, N));
  let b = cross(N, t);
  let w = 0.55;
  return (envSample(N, 0.0)
    + envSample(normalize(N + t * w), 0.0)
    + envSample(normalize(N - t * w), 0.0)
    + envSample(normalize(N + b * w), 0.0)
    + envSample(normalize(N - b * w), 0.0)) * 0.2;
}

@fragment fn fs(v: V) -> @location(0) vec4f {
  // @chunk section_clip
  // Cheap section-cap approximation: fragments just inside the clip plane
  // (within a fixed world-space epsilon) shade flat/unlit to suggest the cut.
  // Shadows darken the indirect terms (irradiance/ambient) only; the direct
  // key light and specular stay. Hoisted above the section-cap early return:
  // textureSampleCompare must run in uniform control flow (WGSL uniformity).
  let shadow = shadowFactor(v.w);
  // @chunk section_cap
  let N = normalize(v.n);
  let L = normalize(sc.light.xyz);
  let V2 = normalize(sc.eye.xyz - v.w);
  let H = normalize(L + V2);
  let selected = mix(ob.color.rgb, sc.selectionColor, ob.style.y * 0.48);
  let albedo = mix(selected, sc.hoverColor, ob.style.w * 0.38) * ob.baseColor;
  let metallic = clamp(ob.metallic, 0.0, 1.0);
  let rough = clamp(ob.roughness, 0.05, 1.0);
  let F0 = mix(vec3f(0.04), albedo, metallic);
  let ndf = distributionGGX(N, H, rough);
  let g = geometrySmith(N, V2, L, rough);
  let f = fresnelSchlick(max(dot(H, V2), 0.0), F0);
  let specular = (ndf * g * f) / max(4.0 * max(dot(N, V2), 0.0) * max(dot(N, L), 0.0), 0.0001);
  let kd = (vec3f(1.0) - f) * (1.0 - metallic);
  let ndl = max(dot(N, L), 0.0);
  // Fill term mirroring the Phong shader's back-light contribution.
  let bd = max(dot(-N, L), 0.0) * 0.25;
  var c: vec3f;
  if (textureDimensions(envTex).x > 1u) {
    // IBL path: the environment replaces the analytic key light entirely —
    // it already supplies both the diffuse irradiance and the specular
    // highlight, so adding the key light on top would double-light the model.
    // Diffuse: cone-averaged irradiance at the surface normal.
    // Specular: reflect view about the normal. The shipped env textures have
    // no mip chain (textureNumLevels == 1), so roughness is approximated by
    // bending the reflection toward the normal (softens the lobe) and scaling
    // by a (1-rough)^2 gloss factor; when a mipmapped env map is bound the
    // roughness-driven LOD is used instead. Fresnel(F0) weights the lobe.
    let irradiance = envIrradiance(N);
    var R = reflect(-V2, N);
    R = normalize(mix(R, N, rough * rough));
    var envSpec: vec3f;
    if (textureNumLevels(envTex) > 1u) {
      envSpec = envSample(R, rough * f32(textureNumLevels(envTex) - 1u));
    } else {
      envSpec = envSample(R, 0.0) * pow(1.0 - rough, 2.0);
    }
    let fEnv = fresnelSchlick(max(dot(N, V2), 0.0), F0);
    c = irradiance * albedo * (vec3f(1.0) - fEnv) * (1.0 - metallic) * shadow
      + envSpec * fEnv
      + sc.ambient.rgb * albedo * 0.25 * shadow + ob.emissive;
  } else {
    c = (kd * albedo / 3.14159265 + specular) * ndl
      + sc.ambient.rgb * albedo * shadow + bd * albedo * 0.5 + ob.emissive;
  }
  // Reinhard tone map + gamma so metals do not blow out under the key light.
  c = c / (c + vec3f(1.0));
  c = pow(c, vec3f(1.0 / 2.2));
  return vec4f(c, ob.style.x);
}
