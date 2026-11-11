struct Uniforms {
    layer_size: vec2<f32>,
    layer_scale: f32,
    transport: f32,
}

fn effect(coord: vec2<f32>) -> vec4<f32> {
    let uv = coord / max(uniforms.layer_size, vec2<f32>(1.0));
    if (uniforms.transport > 0.5) {
        let d = (uv - vec2<f32>(0.50, 1.0)) * vec2<f32>(4.7, 1.55);
        let edge = 1.0 - smoothstep(0.0, 1.0, dot(d, d));
        let alpha = edge * edge * 0.63;
        return vec4<f32>(vec3<f32>(0.98, 0.22, 0.09) * alpha, alpha);
    }
    let d = (uv - vec2<f32>(0.50, 0.41)) * vec2<f32>(2.05, 2.4);
    let edge = 1.0 - smoothstep(0.0, 1.0, dot(d, d));
    let alpha = edge * 0.58;
    return vec4<f32>(vec3<f32>(0.22, 0.16, 0.21) * alpha, alpha);
}
