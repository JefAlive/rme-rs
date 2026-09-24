struct Camera {
    offset: vec2<f32>,
    zoom: f32,
    atlas_columns: u32,
    viewport_size: vec2<f32>,
    floor_alpha: f32,
    _pad2: f32,
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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let columns = max(camera.atlas_columns, 1u);
    let slot_origin = vec2<i32>(
        i32((in.layer % columns) * 32u),
        i32((in.layer / columns) * 32u),
    );
    let local_pixel = clamp(vec2<i32>(in.uv * 32.0), vec2<i32>(0), vec2<i32>(31));
    let sampled = textureLoad(atlas_tex, slot_origin + local_pixel, 0) * in.tint;
    return vec4<f32>(sampled.rgb, sampled.a * camera.floor_alpha);
}
