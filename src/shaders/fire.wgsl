struct Uniforms {
    layer_size: vec2<f32>,
    layer_scale: f32,
    phase: f32,
    bass: f32,
    midrange: f32,
    treble: f32,
}

fn effect(coord: vec2<f32>) -> vec4<f32> {
    let uv = coord / max(uniforms.layer_size, vec2<f32>(1.0));
    let x = uv.x;
    let height = 1.0 - uv.y;
    let center = 1.0 - smoothstep(0.0, 0.54, abs(x - 0.5));
    let t = uniforms.phase;
    let bass = uniforms.bass;
    let midrange = uniforms.midrange;
    let treble = uniforms.treble;
    let wave = 0.58 + (0.11 + 0.16 * midrange) * sin(43.0*x - 2.8*t)
                    + (0.04 + 0.15 * midrange) * sin(92.0*x + 4.1*t)
                    + (0.015 + 0.18 * treble) * sin(153.0*x - 5.5*t);
    let crest = 0.048 + center * (0.08 + 0.63 * wave) * (0.20 + 0.73 * bass);
    let flame = 1.0 - smoothstep(0.0, crest, height);
    let haze = (1.0 - smoothstep(0.0, crest + 0.24, height)) * center;
    let edge = 0.15 + center * 0.85;
    let level = clamp(bass * 0.74 + midrange * 0.34 + treble * 0.18, 0.0, 1.0);
    let alpha = edge * (flame * (0.25 + 0.52 * level) + haze * 0.25) * smoothstep(0.0, 0.23, level);
    let heat = (1.0 - smoothstep(0.0, crest * 0.80, height)) * (0.24 + 0.76 * center);
    let tint = mix(vec3<f32>(0.97, 0.035, 0.42), vec3<f32>(1.0, 0.31, 0.09), heat);
    return vec4<f32>(tint * alpha, alpha);
}
