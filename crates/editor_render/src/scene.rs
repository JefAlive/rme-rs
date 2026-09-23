use ahash::AHashMap;
use wgpu::util::DeviceExt;
use editor_core::{position::{ChunkCoord, Position, CHUNK_SIZE}, spatial_map::SpatialMap};
use crate::{instance::TileInstance, offscreen::OffscreenTarget, pipeline::{CameraUniform, TileRenderResources}};

#[derive(Default)]
pub struct ChunkGpuCache {
    buffers: AHashMap<ChunkCoord, (wgpu::Buffer, u32)>,
}

impl ChunkGpuCache {
    /// Só retesselado os chunks marcados dirty — o resto do mapa não custa nada.
    pub fn sync(&mut self, device: &wgpu::Device, map: &mut SpatialMap) {
        let dirty: Vec<ChunkCoord> = map.iter_dirty_chunks().map(|(c, _)| *c).collect();
        for coord in dirty {
            let mut instances = Vec::with_capacity(256);
            for local_idx in 0..(CHUNK_SIZE as usize * CHUNK_SIZE as usize) {
                let lx = (local_idx % CHUNK_SIZE as usize) as u16;
                let ly = (local_idx / CHUNK_SIZE as usize) as u16;
                let pos = Position {
                    x: coord.cx as u16 * CHUNK_SIZE + lx,
                    y: coord.cy as u16 * CHUNK_SIZE + ly,
                    z: coord.z,
                };
                if let Some(tile) = map.get_tile(pos) {
                    if let Some(ground) = &tile.ground {
                        instances.push(TileInstance {
                            world_pos: [pos.x as f32, pos.y as f32],
                            color: color_from_type_id(ground.type_id),
                        });
                    }
                }
            }
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk_instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
            self.buffers.insert(coord, (buffer, instances.len() as u32));
            map.clear_dirty(&coord);
        }
    }

    fn draw_all<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for (buf, count) in self.buffers.values() {
            if *count == 0 { continue; }
            pass.set_vertex_buffer(1, buf.slice(..));
            pass.draw(0..4, 0..*count);
        }
    }
}

fn color_from_type_id(id: u16) -> [f32; 4] {
    let h = (id as u32).wrapping_mul(2654435761);
    [((h >> 16) & 0xFF) as f32 / 255.0, ((h >> 8) & 0xFF) as f32 / 255.0, (h & 0xFF) as f32 / 255.0, 1.0]
}

/// Isto é o "render-to-texture" propriamente dito: encoder próprio,
/// render pass mirando a OffscreenTarget, submit próprio.
pub fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    resources: &TileRenderResources,
    cache: &ChunkGpuCache,
    target: &OffscreenTarget,
    camera: CameraUniform,
) {
    queue.write_buffer(&resources.camera_buf, 0, bytemuck::cast_slice(&[camera]));

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("viewport_encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.03, g: 0.05, b: 0.03, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&resources.pipeline);
        pass.set_bind_group(0, &resources.camera_bind_group, &[]);
        pass.set_vertex_buffer(0, resources.quad_vbuf.slice(..));
        cache.draw_all(&mut pass);
    }
    queue.submit(Some(encoder.finish()));
}