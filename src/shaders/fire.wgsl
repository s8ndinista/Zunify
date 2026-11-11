struct Uniforms {
    layer_size: vec2<f32>,
    layer_scale: f32,
    power: f32,
    bands0: vec4<f32>,
    bands1: vec4<f32>,
    bands2: vec4<f32>,
    bands3: vec4<f32>,
    bands4: vec4<f32>,
    bands5: vec4<f32>,
    bands6: vec4<f32>,
    bands7: vec4<f32>,
    bands8: vec4<f32>,
    bands9: vec4<f32>,
    bands10: vec4<f32>,
    bands11: vec4<f32>,
    bands12: vec4<f32>,
    bands13: vec4<f32>,
    bands14: vec4<f32>,
    bands15: vec4<f32>,
}

fn band(index: i32) -> f32 {
    let group = index / 4;
    var values = uniforms.bands15;
    switch group {
        case 0: { values = uniforms.bands0; }
        case 1: { values = uniforms.bands1; }
        case 2: { values = uniforms.bands2; }
        case 3: { values = uniforms.bands3; }
        case 4: { values = uniforms.bands4; }
        case 5: { values = uniforms.bands5; }
        case 6: { values = uniforms.bands6; }
        case 7: { values = uniforms.bands7; }
        case 8: { values = uniforms.bands8; }
        case 9: { values = uniforms.bands9; }
        case 10: { values = uniforms.bands10; }
        case 11: { values = uniforms.bands11; }
        case 12: { values = uniforms.bands12; }
        case 13: { values = uniforms.bands13; }
        case 14: { values = uniforms.bands14; }
        default: {}
    }
    return values[index - group * 4];
}

fn effect(coord: vec2<f32>) -> vec4<f32> {
    let uv = coord / max(uniforms.layer_size, vec2<f32>(1.0));
    let distance = abs(uv.x - 0.5) * 2.0;
    let frequency = clamp(distance * 63.0, 0.0, 63.0);
    let lower = i32(frequency);
    let amplitude = mix(band(lower), band(min(lower + 1, 63)), smoothstep(0.0, 1.0, fract(frequency)));
    let center = 1.0 - smoothstep(0.0, 1.0, distance);
    let height = 1.0 - uv.y;
    let power = uniforms.power;
    let peak = smoothstep(0.0, 1.0, center);
    let crest = min(0.96, 0.060 + 0.49 * peak * peak * power + amplitude * (0.102 + 0.238 * center + 0.16 * peak * peak));
    let ribbon = 1.0 - smoothstep(0.0, crest, height);
    let haze = (1.0 - smoothstep(0.0, crest + 0.22, height)) * center;
    let alpha = power * (ribbon * (0.12 + center * 0.34 + amplitude * 0.21) + 0.24 * haze);
    let heat = (1.0 - smoothstep(0.0, crest, height)) * (0.18 + 0.68 * center);
    let tint = mix(vec3<f32>(0.99, 0.10, 0.40), vec3<f32>(1.0, 0.31, 0.12), heat);
    let opacity = alpha * mix(1.0, smoothstep(0.0, 0.15, uv.y), peak);
    return vec4<f32>(tint * opacity, opacity);
}
