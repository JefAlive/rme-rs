struct Camera {
    offset: vec2<f32>,
    zoom: f32,
    _pad: f32,
    viewport_size: vec2<f32>,
    _pad2: vec2<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;

struct VsIn {
    @location(0) quad_pos: vec2<f32>,   // unit quad: (0,0)(1,0)(0,1)(1,1)
    @location(1) world_pos: vec2<f32>,  // posição do tile em SQMs
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

const TILE_SIZE: f32 = 32.0;

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let world = (in.world_pos * TILE_SIZE + in.quad_pos * TILE_SIZE - camera.offset) * camera.zoom;
    let ndc = vec2<f32>(
        (world.x / camera.viewport_size.x) * 2.0 - 1.0,
        1.0 - (world.y / camera.viewport_size.y) * 2.0,
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}