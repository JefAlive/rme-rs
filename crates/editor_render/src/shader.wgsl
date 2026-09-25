struct Camera {
    offset: vec2<f32>,
    zoom: f32,
    atlas_columns: u32,
    viewport_size: vec2<f32>,
    floor_alpha: f32,
    sampling_mode: u32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var atlas_tex: texture_2d<f32>;

struct VsIn {
    @location(0) quad_pos: vec2<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) pixel_offset: vec2<f32>,
    @location(3) layer_index: u32,
    @location(4) tint: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) tint: vec4<f32>,
};

const TILE_SIZE: f32 = 32.0;
const CELL_SIZE: i32 = 32;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let world = (in.world_pos * TILE_SIZE + in.pixel_offset + in.quad_pos * TILE_SIZE - camera.offset) * camera.zoom;
    let ndc = vec2<f32>(
        (world.x / camera.viewport_size.x) * 2.0 - 1.0,
        1.0 - (world.y / camera.viewport_size.y) * 2.0,
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = in.quad_pos;
    out.layer = in.layer_index;
    out.tint = in.tint;
    return out;
}

// Busca um texel dentro do sprite atual, sem cruzar para a célula vizinha.
fn sprite_texel(layer: u32, local: vec2<i32>) -> vec4<f32> {
    let columns = max(camera.atlas_columns, 1u);
    let origin = vec2<i32>(
        i32((layer % columns) * 32u),
        i32((layer / columns) * 32u),
    );
    return textureLoad(atlas_tex, origin + clamp(local, vec2<i32>(0), vec2<i32>(CELL_SIZE - 1)), 0);
}

fn nearest_sample(layer: u32, uv: vec2<f32>) -> vec4<f32> {
    return sprite_texel(layer, vec2<i32>(floor(uv * TILE_SIZE)));
}

fn bilinear_at(layer: u32, pixel: vec2<f32>) -> vec4<f32> {
    let base = vec2<i32>(floor(pixel));
    let f = fract(pixel);
    let a = sprite_texel(layer, base);
    let b = sprite_texel(layer, base + vec2<i32>(1, 0));
    let c = sprite_texel(layer, base + vec2<i32>(0, 1));
    let d = sprite_texel(layer, base + vec2<i32>(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// Sharp bilinear: mantém centros de pixel nítidos e interpola só nas bordas.
fn sharp_bilinear(layer: u32, uv: vec2<f32>) -> vec4<f32> {
    let texel = uv * TILE_SIZE;
    let cell = floor(texel);
    let fractional = fract(texel);
    let scale = max(camera.zoom, 1.0);
    let range = max(0.0, 0.5 - 0.5 / scale);
    let center = fractional - vec2<f32>(0.5);
    let sharp_fraction = (center - clamp(center, vec2<f32>(-range), vec2<f32>(range))) * scale + vec2<f32>(0.5);
    return bilinear_at(layer, cell + sharp_fraction - vec2<f32>(0.5));
}

fn same_color(a: vec3<f32>, b: vec3<f32>) -> bool {
    return all(abs(a - b) < vec3<f32>(0.003));
}

fn sai_result(a: vec3<f32>, b: vec3<f32>, c: vec3<f32>, d: vec3<f32>) -> i32 {
    var x = 0;
    var y = 0;
    if (same_color(a, c)) { x += 1; } else if (same_color(b, c)) { y += 1; }
    if (same_color(a, d)) { x += 1; } else if (same_color(b, d)) { y += 1; }
    if (x <= 1) { return select(-1, 1, y <= 1); }
    return select(1, 0, y == 2);
}

// Um passo Super 2xSaI. Os quatro resultados formam o bloco 2x de cada pixel.
fn super_2xsai(layer: u32, uv: vec2<f32>) -> vec4<f32> {
    let p = uv * TILE_SIZE;
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    let c0 = sprite_texel(layer, base + vec2<i32>(-1, -1));
    let c1 = sprite_texel(layer, base + vec2<i32>( 0, -1));
    let c2 = sprite_texel(layer, base + vec2<i32>( 1, -1));
    let c3 = sprite_texel(layer, base + vec2<i32>(-1,  0));
    let c4 = sprite_texel(layer, base);
    let c5 = sprite_texel(layer, base + vec2<i32>( 1,  0));
    let c6 = sprite_texel(layer, base + vec2<i32>(-1,  1));
    let c7 = sprite_texel(layer, base + vec2<i32>( 0,  1));
    let c8 = sprite_texel(layer, base + vec2<i32>( 1,  1));
    let d0 = sprite_texel(layer, base + vec2<i32>(-1,  2));
    let d1 = sprite_texel(layer, base + vec2<i32>( 0,  2));
    let d2 = sprite_texel(layer, base + vec2<i32>( 1,  2));
    let d3 = sprite_texel(layer, base + vec2<i32>( 2, -1));
    let d4 = sprite_texel(layer, base + vec2<i32>( 2,  0));
    let d5 = sprite_texel(layer, base + vec2<i32>( 2,  1));

    var p00 = c4.rgb;
    var p10 = 0.5 * (c4.rgb + c5.rgb);
    var p01 = 0.5 * (c4.rgb + c7.rgb);
    var p11 = 0.25 * (c4.rgb + c5.rgb + c7.rgb + c8.rgb);

    if (same_color(c4.rgb, c8.rgb) && !same_color(c5.rgb, c7.rgb)) {
        p11 = c4.rgb;
        p10 = select(0.5 * (c4.rgb + c5.rgb), c4.rgb,
            (same_color(c4.rgb, c1.rgb) && same_color(c5.rgb, d5.rgb)) ||
            (same_color(c4.rgb, c7.rgb) && same_color(c4.rgb, c2.rgb) && !same_color(c5.rgb, c1.rgb) && same_color(c5.rgb, d3.rgb)));
        p01 = select(0.5 * (c4.rgb + c7.rgb), c4.rgb,
            (same_color(c4.rgb, c3.rgb) && same_color(c7.rgb, d2.rgb)) ||
            (same_color(c4.rgb, c5.rgb) && same_color(c4.rgb, c6.rgb) && !same_color(c3.rgb, c7.rgb) && same_color(c7.rgb, d0.rgb)));
    } else if (same_color(c5.rgb, c7.rgb) && !same_color(c4.rgb, c8.rgb)) {
        p11 = c5.rgb;
        p10 = select(0.5 * (c4.rgb + c5.rgb), c5.rgb,
            (same_color(c5.rgb, c2.rgb) && same_color(c4.rgb, c6.rgb)) ||
            (same_color(c5.rgb, c1.rgb) && same_color(c5.rgb, c8.rgb) && !same_color(c4.rgb, c2.rgb) && same_color(c4.rgb, c0.rgb)));
        p01 = select(0.5 * (c4.rgb + c7.rgb), c7.rgb,
            (same_color(c7.rgb, c6.rgb) && same_color(c4.rgb, c2.rgb)) ||
            (same_color(c7.rgb, c3.rgb) && same_color(c7.rgb, c8.rgb) && !same_color(c4.rgb, c6.rgb) && same_color(c4.rgb, c0.rgb)));
    } else if (same_color(c4.rgb, c8.rgb) && same_color(c5.rgb, c7.rgb)) {
        if (same_color(c4.rgb, c5.rgb)) {
            p10 = c4.rgb; p01 = c4.rgb; p11 = c4.rgb;
        } else {
            var vote = 0;
            vote += sai_result(c4.rgb, c5.rgb, c3.rgb, c1.rgb);
            vote -= sai_result(c5.rgb, c4.rgb, d4.rgb, c2.rgb);
            vote -= sai_result(c5.rgb, c4.rgb, c6.rgb, d1.rgb);
            vote += sai_result(c4.rgb, c5.rgb, d5.rgb, d2.rgb);
            p11 = select(0.25 * (c4.rgb + c5.rgb + c7.rgb + c8.rgb), c5.rgb, vote < 0);
            p11 = select(p11, c4.rgb, vote > 0);
        }
    }

    if ((same_color(c4.rgb, c8.rgb) && !same_color(c5.rgb, c7.rgb) && same_color(c3.rgb, c4.rgb) && !same_color(c4.rgb, d2.rgb)) ||
        (same_color(c4.rgb, c6.rgb) && same_color(c5.rgb, c4.rgb) && !same_color(c3.rgb, c7.rgb) && !same_color(c4.rgb, d0.rgb))) {
        p00 = 0.5 * (c7.rgb + c4.rgb);
    }
    if ((same_color(c5.rgb, c7.rgb) && !same_color(c4.rgb, c8.rgb) && same_color(c6.rgb, c7.rgb) && !same_color(c7.rgb, c2.rgb)) ||
        (same_color(c3.rgb, c7.rgb) && same_color(c8.rgb, c7.rgb) && !same_color(c6.rgb, c4.rgb) && !same_color(c7.rgb, c0.rgb))) {
        p00 = 0.5 * (c7.rgb + c4.rgb);
    }
    let top = select(p00, p10, f.x >= 0.5);
    let bottom = select(p01, p11, f.x >= 0.5);
    return vec4<f32>(select(top, bottom, f.y >= 0.5), c4.a);
}

fn xbrz_distance(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let w = vec3<f32>(0.2627, 0.6780, 0.0593);
    let diff = a - b;
    let y = dot(diff, w);
    let cb = (0.5 / (1.0 - w.b)) * (diff.b - y);
    let cr = (0.5 / (1.0 - w.r)) * (diff.r - y);
    return sqrt(y * y + cb * cb + cr * cr);
}

// A classificação xBRZ de cantos suaviza apenas diagonais com continuidade de
// cor, preservando os degraus de pixels em regiões planas.
fn xbrz_4x(layer: u32, uv: vec2<f32>) -> vec4<f32> {
    let p = uv * TILE_SIZE;
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    let c = sprite_texel(layer, base);
    let right = sprite_texel(layer, base + vec2<i32>(1, 0));
    let down = sprite_texel(layer, base + vec2<i32>(0, 1));
    let diagonal = sprite_texel(layer, base + vec2<i32>(1, 1));
    let left = sprite_texel(layer, base + vec2<i32>(-1, 0));
    let up = sprite_texel(layer, base + vec2<i32>(0, -1));
    var color = c.rgb;
    let diagonal_cost = xbrz_distance(down.rgb, right.rgb) + 4.0 * xbrz_distance(c.rgb, diagonal.rgb);
    let orthogonal_cost = xbrz_distance(left.rgb, down.rgb) + xbrz_distance(up.rgb, right.rgb);
    if (!same_color(c.rgb, right.rgb) && !same_color(c.rgb, down.rgb) && diagonal_cost < orthogonal_cost) {
        let blend = select(right.rgb, down.rgb, xbrz_distance(c.rgb, down.rgb) < xbrz_distance(c.rgb, right.rgb));
        color = mix(color, blend, smoothstep(0.45, 1.0, max(f.x, f.y)) * 0.75);
    }
    let upper_right = sprite_texel(layer, base + vec2<i32>(1, -1));
    let lower_left = sprite_texel(layer, base + vec2<i32>(-1, 1));
    if (!same_color(c.rgb, right.rgb) && !same_color(c.rgb, up.rgb) && f.x > 0.5 && f.y < 0.5 &&
        xbrz_distance(up.rgb, right.rgb) < xbrz_distance(left.rgb, upper_right.rgb)) {
        color = mix(color, right.rgb, 0.5);
    }
    if (!same_color(c.rgb, left.rgb) && !same_color(c.rgb, down.rgb) && f.x < 0.5 && f.y > 0.5 &&
        xbrz_distance(left.rgb, down.rgb) < xbrz_distance(lower_left.rgb, up.rgb)) {
        color = mix(color, down.rgb, 0.5);
    }
    return vec4<f32>(color, c.a);
}

// Redução por área: em 25% agrega uma grade 4x4 por pixel final, eliminando
// o alias de escolher um único texel no zoom extremo.
fn area_downsample(layer: u32, uv: vec2<f32>) -> vec4<f32> {
    let footprint = min(10.0, 1.0 / max(camera.zoom, 0.1));
    let samples = i32(ceil(footprint));
    let begin = uv * TILE_SIZE - vec2<f32>(0.5 + footprint * 0.5);
    var total = vec4<f32>(0.0);
    var count = 0.0;
    for (var y = 0; y < 10; y += 1) {
        if (y >= samples) { break; }
        for (var x = 0; x < 10; x += 1) {
            if (x >= samples) { break; }
            let position = begin + vec2<f32>((f32(x) + 0.5) * footprint / f32(samples), (f32(y) + 0.5) * footprint / f32(samples));
            total += sprite_texel(layer, vec2<i32>(floor(position)));
            count += 1.0;
        }
    }
    return total / max(count, 1.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // A cena intermediária é sempre formada por pixels originais. O filtro é
    // aplicado depois, sobre o ground e todos os itens já compostos.
    let sampled = nearest_sample(in.layer, in.uv) * in.tint;
    return vec4<f32>(sampled.rgb, sampled.a * camera.floor_alpha);
}
