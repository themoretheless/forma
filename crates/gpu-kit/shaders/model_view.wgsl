struct Parameters { view: vec4<f32> }
@group(0) @binding(0) var<uniform> parameters: Parameters;
struct VertexOutput { @builtin(position) clip: vec4<f32>, @location(0) position: vec3<f32> }
@vertex fn vs_main(@location(0) position: vec3<f32>) -> VertexOutput {
    let yaw = parameters.view.y;
    let pitch = parameters.view.z;
    let y = vec3<f32>(cos(yaw)*position.x + sin(yaw)*position.z, position.y, -sin(yaw)*position.x + cos(yaw)*position.z);
    let p = vec3<f32>(y.x, cos(pitch)*y.y - sin(pitch)*y.z, sin(pitch)*y.y + cos(pitch)*y.z);
    let aspect = parameters.view.x;
    let scale = parameters.view.w * 0.85;
    var output: VertexOutput;
    output.clip = vec4<f32>(p.x * scale / max(aspect, 1.0), p.y * scale * min(aspect, 1.0), 0.5 - p.z * 0.2, 1.0);
    output.position = p;
    return output;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let cross_normal = cross(dpdx(input.position), dpdy(input.position));
    let normal = cross_normal / max(length(cross_normal), 0.0000001);
    let light = 0.2 + 0.8 * abs(dot(normal, normalize(vec3<f32>(0.3, 0.5, 1.0))));
    return vec4<f32>(vec3<f32>(0.45, 0.55, 0.65) * light, 1.0);
}
