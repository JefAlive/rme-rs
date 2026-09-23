struct Camera {
    offset: vec2<f32>,
    zoom: f32,
    time_ms: f32,
    viewport_size: vec2<f32>,
    floor_alpha: f32,
    _pad: f32,
};
@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var atlas_tex: texture_2d_array<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct AnimEntry {
    first_frame: u32,
    frame_count: u32,
    frame_duration_ms: u32,
    mode: u32,
};
const MAX_ANIM_ENTRIES: u32 = 8192u;
const MAX_ANIM_FRAMES: u32 = 32768u;

@group(2) @binding(0) var<storage, read> anim_entries: array<AnimEntry, 8192>;
@group(2) @binding(1) var<storage, read> anim_frames: array<u32, 32768>;

struct VsIn {
    @location(0) quad_pos: vec2<f32>,
    @location(1) world_pos: vec2<f32>,
    @location(2) pixel_offset: vec2<f32>,
    @location(3) anim_id: u32,
    @location(4) tint: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) anim_id: u32,
    @location(2) tint: vec4<f32>,
    @location(3) @interpolate(flat) world_pos: vec2<f32>,
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
    out.anim_id = in.anim_id;
    out.tint = in.tint;
    out.world_pos = in.world_pos;
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn resolve_layer(anim_id: u32, seed_pos: vec2<f32>) -> u32 {
    let safe_anim_id = min(anim_id, MAX_ANIM_ENTRIES - 1u);
    let entry = anim_entries[safe_anim_id];
    if (entry.frame_count <= 1u) {
        return anim_frames[min(entry.first_frame, MAX_ANIM_FRAMES - 1u)];
    }
    var t = u32(camera.time_ms);
    if (entry.mode == 1u) {
        let phase = u32(hash(seed_pos) * f32(entry.frame_duration_ms) * f32(entry.frame_count));
        t = t + phase;
    }
    let frame = (t / entry.frame_duration_ms) % entry.frame_count;
    let frame_idx = min(entry.first_frame + frame, MAX_ANIM_FRAMES - 1u);
    return anim_frames[frame_idx];
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let layer = resolve_layer(in.anim_id, in.world_pos);
    let sampled = textureSample(atlas_tex, atlas_sampler, in.uv, i32(layer)) * in.tint;
    return vec4<f32>(sampled.rgb, sampled.a * camera.floor_alpha);
}