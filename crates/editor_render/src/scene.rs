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
    /// Apenas os chunks do andar `floor` são sincronizados.
    pub fn sync_for_floor(
        &mut self,
        device: &wgpu::Device,
        map: &mut SpatialMap,
        floor: u8,
        mut resolve_layer: impl FnMut(u16) -> u32,
    ) {
        let dirty: Vec<ChunkCoord> = map.iter_dirty_chunks()
            .filter(|(c, _)| c.z == floor)
            .map(|(c, _)| *c)
            .collect();
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
                            layer_index: resolve_layer(ground.type_id),
                            tint: [1.0, 1.0, 1.0, 1.0],
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

    /// Desenha só os chunks do andar `z` — usado para compor a pilha
    /// multi-andar, um draw call por `FloorLayer`.
    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, z: u8) {
        for (coord, (buf, count)) in self.buffers.iter() {
            if coord.z != z || *count == 0 {
                continue;
            }
            pass.set_vertex_buffer(1, buf.slice(..));
            pass.draw(0..4, 0..*count);
        }
    }
}

/// Um andar a compor no frame: `alpha` controla a transparência (1.0 = andar
/// atual, opaco) e `pixel_offset` aplica o deslocamento diagonal — quanto
/// mais distante do andar atual, maior o deslocamento, dando profundidade.
#[derive(Copy, Clone)]
pub struct FloorLayer {
    pub z: u8,
    pub alpha: f32,
    pub pixel_offset: [f32; 2],
}

/// Render-to-texture: um encoder próprio, um draw call por `FloorLayer`,
/// cada um lendo sua própria fatia do uniform buffer via dynamic offset.
pub fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    resources: &TileRenderResources,
    cache: &ChunkGpuCache,
    atlas: &crate::atlas::SpriteAtlas,
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
        pass.set_vertex_buffer(0, resources.quad_vbuf.slice(..));

        for (i, layer) in layers.iter().enumerate().take(crate::pipeline::MAX_FLOOR_LAYERS) {
            pass.set_bind_group(0, &resources.camera_bind_group, &[(i as u64 * stride) as u32]);
            cache.draw_floor(&mut pass, layer.z);
        }
    }
    queue.submit(Some(encoder.finish()));
}