@group(0) @binding(0) var image: texture_2d<f32>;
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * vec2<f32>(2., -2.) + vec2<f32>(-1., 1.), 0., 1.);
}
// Area-weighted box: odd extents include edge texels instead of discarding them.
// The sRGB view linearizes RGB on load and the target encodes on store.
@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let size = textureDimensions(image);
    let dst = max(size / 2u, vec2<u32>(1u));
    let scale = vec2<f32>(size) / vec2<f32>(dst);
    let start = floor(p.xy) * scale;
    let end = (floor(p.xy) + 1.) * scale;
    var sum = vec4<f32>(0.);
    for (var y = i32(floor(start.y)); y < i32(ceil(end.y)); y += 1) {
        for (var x = i32(floor(start.x)); x < i32(ceil(end.x)); x += 1) {
            let weight = max(vec2<f32>(0.), min(end, vec2<f32>(f32(x+1), f32(y+1))) - max(start, vec2<f32>(f32(x), f32(y))));
            sum += textureLoad(image, vec2<i32>(x,y), 0) * weight.x * weight.y;
        }
    }
    return sum / (scale.x * scale.y);
}
