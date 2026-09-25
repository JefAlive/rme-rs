struct Scale {
    source_size: vec2<u32>,
    output_size: vec2<u32>,
    source_pixel_scale: f32,
    mode: u32,
    _padding: vec2<u32>,
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
    let base = vec2<i32>(floor(pixel)); let f = fract(pixel);
    return mix(mix(texel(base), texel(base + vec2(1, 0)), f.x), mix(texel(base + vec2(0, 1)), texel(base + vec2(1, 1)), f.x), f.y);
}
fn sharp_bilinear(uv: vec2<f32>) -> vec4<f32> {
    let p = uv * vec2<f32>(scale.source_size); let cell = floor(p); let frac = fract(p);
    let factor = max(scale.source_pixel_scale, 1.0);
    let edge = max(0.0, 0.5 - 0.5 / factor);
    let f = (frac - vec2(0.5) - clamp(frac - vec2(0.5), vec2(-edge), vec2(edge))) * factor + vec2(0.5);
    return bilinear_at(cell + f - vec2(0.5));
}
fn equal(a: vec3<f32>, b: vec3<f32>) -> bool { return all(abs(a - b) < vec3(0.003)); }

// A cena já está plana, então os vizinhos abaixo pertencem ao ground e aos
// itens compostos juntos — exatamente a entrada esperada pelo Super 2xSaI.
fn super_2xsai(uv: vec2<f32>) -> vec4<f32> {
    let p = uv * vec2<f32>(scale.source_size); let base = vec2<i32>(floor(p)); let f = fract(p);
    let c = texel(base); let r = texel(base + vec2(1, 0)); let d = texel(base + vec2(0, 1)); let q = texel(base + vec2(1, 1));
    let ul = texel(base + vec2(-1, -1)); let u = texel(base + vec2(0, -1)); let l = texel(base + vec2(-1, 0));
    var a = c.rgb; var b = 0.5 * (c.rgb + r.rgb); var e = 0.5 * (c.rgb + d.rgb); var z = 0.25 * (c.rgb + r.rgb + d.rgb + q.rgb);
    if (equal(c.rgb, q.rgb) && !equal(r.rgb, d.rgb)) { z = c.rgb; }
    if (equal(r.rgb, d.rgb) && !equal(c.rgb, q.rgb)) { z = r.rgb; }
    if (equal(c.rgb, r.rgb) && equal(c.rgb, u.rgb)) { b = c.rgb; }
    if (equal(c.rgb, d.rgb) && equal(c.rgb, l.rgb)) { e = c.rgb; }
    let top = select(a, b, f.x >= 0.5); let bottom = select(e, z, f.x >= 0.5);
    return vec4(select(top, bottom, f.y >= 0.5), c.a);
}
fn distance(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let w = vec3(0.2627, 0.6780, 0.0593); let diff = a - b; let y = dot(diff, w);
    let cb = (0.5 / (1.0 - w.b)) * (diff.b - y); let cr = (0.5 / (1.0 - w.r)) * (diff.r - y);
    return sqrt(y*y + cb*cb + cr*cr);
}
fn xbrz_4x(uv: vec2<f32>) -> vec4<f32> {
    let p = uv * vec2<f32>(scale.source_size); let base = vec2<i32>(floor(p)); let f = fract(p);
    let c = texel(base); let r = texel(base + vec2(1, 0)); let d = texel(base + vec2(0, 1)); let q = texel(base + vec2(1, 1));
    let l = texel(base + vec2(-1, 0)); let u = texel(base + vec2(0, -1)); var color = c.rgb;
    let diagonal = distance(d.rgb, r.rgb) + 4.0 * distance(c.rgb, q.rgb);
    let orthogonal = distance(l.rgb, d.rgb) + distance(u.rgb, r.rgb);
    if (!equal(c.rgb, r.rgb) && !equal(c.rgb, d.rgb) && diagonal < orthogonal) {
        let edge = select(r.rgb, d.rgb, distance(c.rgb, d.rgb) < distance(c.rgb, r.rgb));
        color = mix(color, edge, smoothstep(0.45, 1.0, max(f.x, f.y)) * 0.75);
    }
    return vec4(color, c.a);
}
// Média por área adaptativa: em 25%, quatro por quatro texels da cena já
// composta convergem para cada pixel final, incluindo itens transparentes.
fn area(uv: vec2<f32>) -> vec4<f32> {
    let footprint = min(10.0, 1.0 / max(scale.source_pixel_scale, 0.1));
    let n = i32(ceil(footprint)); let begin = uv * vec2<f32>(scale.source_size) - vec2(0.5 + footprint * 0.5);
    var sum = vec4(0.0); var count = 0.0;
    for (var y = 0; y < 10; y += 1) { if (y >= n) { break; }
        for (var x = 0; x < 10; x += 1) { if (x >= n) { break; }
            let p = begin + vec2((f32(x)+0.5)*footprint/f32(n), (f32(y)+0.5)*footprint/f32(n));
            sum += texel(vec2<i32>(floor(p))); count += 1.0;
        }
    }
    return sum / max(count, 1.0);
}
@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    if (scale.source_pixel_scale < 1.0) { return area(in.uv); }
    if (scale.mode == 1u) { return sharp_bilinear(in.uv); }
    if (scale.mode == 2u && scale.source_pixel_scale > 1.0) { return super_2xsai(in.uv); }
    if (scale.mode == 3u && scale.source_pixel_scale > 1.0) { return xbrz_4x(in.uv); }
    return nearest(in.uv);
}
