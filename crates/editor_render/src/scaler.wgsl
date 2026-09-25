struct Scale {
    source_size: vec2<u32>,
    output_size: vec2<u32>,
    source_cell_size: vec2<f32>,
    output_cell_size: vec2<f32>,
    mode: u32,
    _pad0: u32,
    _pad1: vec2<u32>,
};
@group(0) @binding(0) var source_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> scale: Scale;

struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var out: Out;
    out.position = vec4(positions[index], 0.0, 1.0);
    // O framebuffer wgpu usa origem no topo. O NDC cresce para cima; inverter
    // V aqui mantém a linha superior da cena no topo da imagem final.
    let uv = positions[index] * 0.5 + vec2(0.5);
    out.uv = vec2(uv.x, 1.0 - uv.y);
    return out;
}

fn texel(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(source_tex, clamp(p, vec2<i32>(0), vec2<i32>(scale.source_size) - vec2<i32>(1)), 0);
}
fn nearest(uv: vec2<f32>) -> vec4<f32> { return texel(vec2<i32>(floor(uv * vec2<f32>(scale.source_size)))); }
fn bilinear_at(pixel: vec2<f32>) -> vec4<f32> {
    // Convenção GL: texels têm centros nos inteiros, então desloca meio pixel
    // para alinhar o amostrador (1:1 fica exato, sem borrar a cena).
    let p = pixel - vec2(0.5);
    let base = vec2<i32>(floor(p)); let f = fract(p);
    return mix(mix(texel(base), texel(base + vec2(1, 0)), f.x), mix(texel(base + vec2(0, 1)), texel(base + vec2(1, 1)), f.x), f.y);
}

// Blit (nearest) para o modo Off e zoom-out; bilinear para o modo Retro. Os
// filtros pixel-art (Super 2xSaI, xBRZ) vivem em assets/*.frag compilados com
// naga (GLSL do RME de referência), selecionados no scaler.rs quando o zoom-in
// os exige.
@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    if (scale.mode == 1u) { return bilinear_at(in.uv * vec2<f32>(scale.source_size)); }
    return nearest(in.uv);
}