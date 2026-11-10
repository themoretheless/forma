struct Parameters {
    clip_from_world: mat4x4<f32>,
    eye: vec4<f32>,
    light_direction: vec4<f32>, // xyz points toward light; w radiance scale
    light_color: vec4<f32>,
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    factors: vec4<f32>, // metallic, roughness, normal scale, occlusion strength
    alpha: vec4<f32>, // cutoff, mode, normal-map present, double-sided
}
@group(0) @binding(0) var<uniform> p: Parameters;
@group(0) @binding(1) var base_tex: texture_2d<f32>;
@group(0) @binding(2) var base_sampler: sampler;
@group(0) @binding(3) var mr_tex: texture_2d<f32>;
@group(0) @binding(4) var mr_sampler: sampler;
@group(0) @binding(5) var normal_tex: texture_2d<f32>;
@group(0) @binding(6) var normal_sampler: sampler;
@group(0) @binding(7) var ao_tex: texture_2d<f32>;
@group(0) @binding(8) var ao_sampler: sampler;
@group(0) @binding(9) var emissive_tex: texture_2d<f32>;
@group(0) @binding(10) var emissive_sampler: sampler;
struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) uv_base: vec2<f32>,
    @location(5) uv_mr: vec2<f32>,
    @location(6) uv_normal: vec2<f32>,
    @location(7) uv_ao: vec2<f32>,
    @location(8) uv_emissive: vec2<f32>,
}
struct Surface {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) uv_base: vec2<f32>,
    @location(5) uv_mr: vec2<f32>,
    @location(6) uv_normal: vec2<f32>,
    @location(7) uv_ao: vec2<f32>,
    @location(8) uv_emissive: vec2<f32>,
}
@vertex fn vs_main(v: Vertex) -> Surface {
    return Surface(p.clip_from_world * vec4<f32>(v.position,1.),v.position,v.normal,v.tangent,clamp(v.color,vec4<f32>(0.),vec4<f32>(1.)),v.uv_base,v.uv_mr,v.uv_normal,v.uv_ao,v.uv_emissive);
}
fn unit(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let scale = max(max(abs(v.x),abs(v.y)),abs(v.z));
    if scale == 0. { return fallback; }
    return normalize(v / scale);
}
@fragment fn fs_main(s: Surface, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // All derivatives/samples precede discard and divergent material branches.
    let dx = dpdx(s.world); let dy = dpdy(s.world);
    let du = dpdx(s.uv_normal); let dv = dpdy(s.uv_normal);
    let base = textureSample(base_tex,base_sampler,s.uv_base) * p.base_color * s.color;
    let mr = textureSample(mr_tex,mr_sampler,s.uv_mr);
    let map = textureSample(normal_tex,normal_sampler,s.uv_normal).xyz * 2. - 1.;
    let ao_sample = textureSample(ao_tex,ao_sampler,s.uv_ao).r;
    let emission = textureSample(emissive_tex,emissive_sampler,s.uv_emissive).rgb * p.emissive.rgb;
    // dpdy is down-screen: dy x dx gives the ordinary CCW world face normal.
    var n = unit(s.normal,unit(cross(dy,dx),vec3<f32>(0.,0.,1.))*select(-1.,1.,front));
    if p.alpha.z != 0. {
        var tangent = s.tangent.xyz;
        var handed = s.tangent.w;
        if handed == 0. || all(s.normal == vec3<f32>(0.)) {
            let determinant = du.x*dv.y - du.y*dv.x;
            tangent = dx*dv.y - dy*du.y;
            if determinant < 0. { tangent = -tangent; }
            let bitangent = (dy*du.x - dx*dv.x)*select(1.,-1.,determinant < 0.);
            handed = select(1.,-1.,dot(cross(n,tangent),bitangent) < 0.);
        }
        let t = unit(tangent - n*dot(n,tangent),vec3<f32>(0.));
        let b = cross(n,t) * handed;
        let mapped = unit(vec3<f32>(map.xy*p.factors.z,map.z),vec3<f32>(0.,0.,1.));
        n = unit(t*mapped.x+b*mapped.y+n*mapped.z,n);
    }
    if p.alpha.w != 0. && !front { n = -n; }
    var opacity = 1.;
    if p.alpha.y == 1. && base.a < p.alpha.x { discard; }
    if p.alpha.y == 2. { opacity = base.a; }
    let metallic = clamp(p.factors.x*mr.b,0.,1.);
    let roughness = clamp(p.factors.y*mr.g,0.,1.);
    let v = unit(select(p.eye.xyz-s.world,p.eye.xyz,p.emissive.w == 1.),n);
    let l = unit(p.light_direction.xyz,n);
    let h = unit(v+l,n);
    let nv = clamp(dot(n,v),0.,1.); let nl = clamp(dot(n,l),0.,1.);
    let nh = clamp(dot(n,h),0.,1.); let vh = clamp(dot(v,h),0.,1.);
    let f0 = mix(vec3<f32>(0.04),base.rgb,metallic);
    let fresnel = f0 + (1.-f0)*pow(1.-vh,5.);
    // GGX + height-correlated Smith. Regularize the delta-lobe at roughness 0.
    let a2 = max(pow(roughness,4.),1e-8);
    let denominator = (1.-nh*nh)+nh*nh*a2;
    let distribution = a2 / max(3.141592653589793*denominator*denominator,1e-16);
    let smith_v = nl*sqrt(nv*nv*(1.-a2)+a2);
    let smith_l = nv*sqrt(nl*nl*(1.-a2)+a2);
    let visibility = 0.5 / max(smith_v+smith_l,1e-8);
    let diffuse = (1.-fresnel)*(1.-metallic)*base.rgb/3.141592653589793;
    let direct = (diffuse+fresnel*distribution*visibility)*nl*p.light_color.rgb*p.light_direction.w;
    // AO modulates indirect light only. eye.w is neutral diffuse environment intensity.
    let occlusion = 1.+p.factors.w*(ao_sample-1.);
    let indirect = base.rgb*(1.-metallic)*occlusion*p.eye.w;
    return vec4<f32>(clamp(direct+indirect+emission,vec3<f32>(0.),vec3<f32>(65504.)),opacity);
}
