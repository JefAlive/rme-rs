// Light vertex shader (WGSL) — gera um quad por fonte de luz com raio =
// intensity * RADIUS_EXT tiles (TriangleStrip, 4 vértices por instância).
// O fragment calcula o falloff OTClient; aqui só posiciona o quad.
// RADIUS_EXT precisa bater com o light_fragment (o corte da cauda acontece
// em dist = intensity * RADIUS_EXT; quad menor que isso corta a luz).
const RADIUS_EXT: f32 = 1.75;
// A instância (TileLight) entra como vertex buffer de instância (padrão do
// scene; o backend GL aqui tem limite 0 de storage buffers por shader).
// Layout do camera uniform é o CameraUniform (48 bytes), idêntico ao vs_main
// da cena (mesma matemática NDC).

struct Camera {
    offset: vec2<f32>,
    zoom: vec2<f32>,
    atlas_columns: u32,
    _align_pad: u32,
    viewport_size: vec2<f32>,
    floor_alpha: f32,
    sampling_mode: u32,
    light: f32,
    _pad_light: u32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

struct VsIn {
    @location(0) world_pos: vec2<f32>,
    @location(1) intensity: f32,
    @location(2) color: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) light_pos: vec2<f32>,
    @location(1) frag_world_pos: vec2<f32>,
    @location(2) intensity: f32,
    @location(3) color: vec3<f32>,
};

const TILE_SIZE: f32 = 32.0;

@vertex
fn vs_main(in: VsIn, @builtin(vertex_index) vertex_index: u32) -> VsOut {
    let light_radius = max(in.intensity * RADIUS_EXT, 0.0);

    // Cantos do quad em tiles relativos ao centro (TriangleStrip 0..4).
    // x: índice par → -1, ímpar → +1; y: 0..1 → -1, 2..3 → +1.
    let corner = vec2<f32>(
        select(1.0, -1.0, (vertex_index & 1u) == 0u),
        select(1.0, -1.0, vertex_index < 2u),
    );

    let vertex_world_pos = in.world_pos + corner * light_radius;
    let world = (vertex_world_pos * TILE_SIZE - camera.offset) * camera.zoom;
    let ndc = vec2<f32>(
        (world.x / camera.viewport_size.x) * 2.0 - 1.0,
        1.0 - (world.y / camera.viewport_size.y) * 2.0,
    );

    var out: VsOut;
    out.clip_pos = vec4<f32>(ndc, 0.0, 1.0);
    out.light_pos = in.world_pos;
    out.frag_world_pos = vertex_world_pos;
    out.intensity = in.intensity;
    out.color = in.color;
    return out;
}