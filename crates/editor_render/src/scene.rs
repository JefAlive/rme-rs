use ahash::AHashMap;
use wgpu::util::DeviceExt;
use editor_core::{position::{ChunkCoord, Position, CHUNK_SIZE}, spatial_map::SpatialMap};
use crate::{
    anim::AnimTable,
    assets::ItemVisual,
    instance::TileInstance,
    offscreen::OffscreenTarget,
    pipeline::{CameraUniform, TileRenderResources},
};

#[derive(Default)]
pub struct ChunkGpuCache {
    buffers: AHashMap<ChunkCoord, (wgpu::Buffer, u32)>,
}

impl ChunkGpuCache {
    /// Só retesselado os chunks marcados dirty. Desenha TODOS os itens do
    /// tile (chão + pilha inteira), acumulando elevação: cada item com
    /// `has_elevation` empurra os itens desenhados acima dele na pilha.
    pub fn sync_for_floor(
        &mut self,
        device: &wgpu::Device,
        map: &mut SpatialMap,
        floor: u8,
        mut resolve_visual: impl FnMut(u16) -> ItemVisual,
    ) {
        let dirty: Vec<ChunkCoord> = map.iter_dirty_chunks()
            .filter(|(c, _)| c.z == floor)
            .map(|(c, _)| *c)
            .collect();
        for coord in dirty {
            let mut instances = Vec::with_capacity(512);
            for local_idx in 0..(CHUNK_SIZE as usize * CHUNK_SIZE as usize) {
                let lx = (local_idx % CHUNK_SIZE as usize) as u16;
                let ly = (local_idx / CHUNK_SIZE as usize) as u16;
                let pos = Position {
                    x: coord.cx as u16 * CHUNK_SIZE + lx,
                    y: coord.cy as u16 * CHUNK_SIZE + ly,
                    z: coord.z,
                };
                let Some(tile) = map.get_tile(pos) else { continue };

                let mut elevation: f32 = 0.0;
                let world_pos = [pos.x as f32, pos.y as f32];

                if let Some(ground) = &tile.ground {
                    let visual = resolve_visual(ground.type_id);
                    instances.push(TileInstance {
                        world_pos,
                        pixel_offset: [visual.draw_offset[0], visual.draw_offset[1] - elevation],
                        anim_id: visual.anim_id,
                        tint: [1.0, 1.0, 1.0, 1.0],
                    });
                    elevation += visual.elevation;
                }

                // TESTE TEMPORÁRIO (itens REATIVADOS para teste de regressão).
                for item in &tile.items {
                    let visual = resolve_visual(item.type_id);
                    instances.push(TileInstance {
                        world_pos,
                        pixel_offset: [visual.draw_offset[0], visual.draw_offset[1] - elevation],
                        anim_id: visual.anim_id,
                        tint: [1.0, 1.0, 1.0, 1.0],
                    });
                    elevation += visual.elevation;
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

    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, z: u8) {
        for (coord, (buf, count)) in self.buffers.iter() {
            if coord.z != z || *count == 0 { continue; }
            pass.set_vertex_buffer(1, buf.slice(..));
            pass.draw(0..4, 0..*count);
        }
    }
}

/// Um andar a compor no frame. `pixel_offset` usa distância COM SINAL do
/// andar atual: negativo para cima (x-1,y-1 por andar), positivo para baixo
/// (x+1,y+1 por andar) — é isso que corrige o alinhamento diagonal.
#[derive(Copy, Clone)]
pub struct FloorLayer {
    pub z: u8,
    pub alpha: f32,
    pub pixel_offset: [f32; 2],
}

pub fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    resources: &TileRenderResources,
    cache: &ChunkGpuCache,
    atlas: &crate::atlas::SpriteAtlas,
    anim_table: &AnimTable,
    target: &OffscreenTarget,
    base_camera: CameraUniform,
    layers: &[FloorLayer],
) {
    let stride = resources.camera_stride as u64;
    for (i, layer) in layers.iter().enumerate().take(crate::pipeline::MAX_FLOOR_LAYERS) {
        let mut cam = base_camera;
        cam.offset[0] += layer.pixel_offset[0];
        cam.offset[1] += layer.pixel_offset[1];
        cam.floor_alpha = layer.alpha;
        queue.write_buffer(&resources.camera_buf, i as u64 * stride, bytemuck::cast_slice(&[cam]));
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport_encoder") });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.03, g: 0.05, b: 0.03, a: 1.0 }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&resources.pipeline);
        pass.set_bind_group(1, &atlas.bind_group, &[]);
        pass.set_bind_group(2, &anim_table.bind_group, &[]);
        pass.set_vertex_buffer(0, resources.quad_vbuf.slice(..));

        for (i, layer) in layers.iter().enumerate().take(crate::pipeline::MAX_FLOOR_LAYERS) {
            pass.set_bind_group(0, &resources.camera_bind_group, &[(i as u64 * stride) as u32]);
            cache.draw_floor(&mut pass, layer.z);
        }
    }
    queue.submit(Some(encoder.finish()));
}