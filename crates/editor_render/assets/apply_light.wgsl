// Apply light buffer to scene via multiplication (CompositionMode_Light).
// Entrada: cena (linear) + light buffer (linear) — mesma resolução nativa da cena.
// Saída: cena * light (linear).

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var light_tex: texture_2d<f32>;
@group(0) @binding(3) var light_sampler: sampler;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    // Fullscreen triangle (triângulo que cobre a tela com folga).
    let pos = vec2<f32>(
        select(-1.0, 3.0, vertex_index == 1u),
        select(-1.0, 3.0, vertex_index == 2u),
    );
    let uv = vec2<f32>(
        select(0.0, 2.0, vertex_index == 1u),
        select(1.0, -1.0, vertex_index == 2u),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let scene_color = textureSample(scene_tex, scene_sampler, in.uv);
    let light_color = textureSample(light_tex, light_sampler, in.uv);
    let result = scene_color.rgb * light_color.rgb;
    return vec4<f32>(result, scene_color.a);
}