use ahash::AHashMap;
use wgpu::util::DeviceExt;
use editor_core::{position::{ChunkCoord, Position, CHUNK_SIZE}, spatial_map::SpatialMap};
use crate::{instance::TileInstance, pipeline::{CameraUniform, TileRenderResources}};

#[derive(Default)]
pub struct ChunkGpuCache {
    floors: AHashMap<u8, Option<(wgpu::Buffer, u32)>>,
}

impl ChunkGpuCache {
    /// Recria o buffer do andar se algum chunk mudou. O buffer é global por
    /// andar para preservar a ordem isométrica tile-a-tile do RME.
    pub fn sync_for_floor(
        &mut self,
        device: &wgpu::Device,
        map: &mut SpatialMap,
        floor: u8,
        mut resolve_visual: impl FnMut(u16, Position) -> crate::assets::ItemVisual,
    ) {
        let dirty: Vec<ChunkCoord> = map.iter_dirty_chunks()
            .filter(|(c, _)| c.z == floor)
            .map(|(c, _)| *c)
            .collect();
        if dirty.is_empty() { return; }

        let mut positions = Vec::new();
        for (coord, chunk) in map.iter_chunk_coords().filter(|(coord, _)| coord.z == floor) {
            for local_idx in 0..(CHUNK_SIZE as usize * CHUNK_SIZE as usize) {
                let lx = (local_idx % CHUNK_SIZE as usize) as u16;
                let ly = (local_idx / CHUNK_SIZE as usize) as u16;
                let pos = Position {
                    x: coord.cx as u16 * CHUNK_SIZE + lx,
                    y: coord.cy as u16 * CHUNK_SIZE + ly,
                    z: coord.z,
                };
                if !chunk.tile(local_idx).is_empty() {
                    positions.push(pos);
                }
            }
        }

        // RME visita folhas 4×4 em x/y e tiles dentro de cada folha também
        // em x/y. Cada tile desenha ground e sua pilha antes do próximo tile.
        positions.sort_unstable_by_key(|p| (p.x / 4, p.y / 4, p.x % 4, p.y % 4));

        let mut instances = Vec::with_capacity(positions.len() * 2);
        for pos in positions {
            let Some(tile) = map.get_tile(pos) else { continue };
            let mut elevation = 0.0;
            if let Some(ground) = &tile.ground {
                let visual = resolve_visual(ground.type_id, pos);
                append_visual_instances(&mut instances, pos, visual, elevation);
                elevation += visual.elevation;
            }
            for item in &tile.items {
                let visual = resolve_visual(item.type_id, pos);
                append_visual_instances(&mut instances, pos, visual, elevation);
                elevation += visual.elevation;
            }
        }

        self.floors.insert(floor, upload_instances(device, "floor_instances", &instances));
        for coord in dirty {
            map.clear_dirty(&coord);
        }
    }

    /// Desenha o buffer ordenado tile-a-tile do andar `z`.
    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, z: u8) {
        if let Some(Some((buf, count))) = self.floors.get(&z) {
            pass.set_vertex_buffer(1, buf.slice(..));
            pass.draw(0..4, 0..*count);
        }
    }
}

fn append_visual_instances(
    instances: &mut Vec<TileInstance>,
    pos: Position,
    visual: crate::assets::ItemVisual,
    elevation: f32,
) {
    let width = visual.width.max(1) as u32;
    let height = visual.height.max(1) as u32;
    for part_y in 0..height {
        for part_x in 0..width {
            let x_offset = part_x as i32 - (width as i32 - 1);
            let y_offset = part_y as i32 - (height as i32 - 1);
            instances.push(TileInstance {
                world_pos: [pos.x as f32 + x_offset as f32, pos.y as f32 + y_offset as f32],
                // BlitItem do RME desloca a pilha em ambos os eixos da tela:
                // draw_x -= drawHeight; draw_y -= drawHeight.
                pixel_offset: [
                    -visual.draw_offset[0] - elevation,
                    -visual.draw_offset[1] - elevation,
                ],
                layer_index: visual.layer_index + part_y * width + part_x,
                tint: [1.0, 1.0, 1.0, 1.0],
            });
        }
    }
}

fn upload_instances(
    device: &wgpu::Device,
    label: &'static str,
    instances: &[TileInstance],
) -> Option<(wgpu::Buffer, u32)> {
    if instances.is_empty() { return None; }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(instances),
        usage: wgpu::BufferUsages::VERTEX,
    });
    Some((buffer, instances.len() as u32))
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
    target: &wgpu::TextureView,
    base_camera: CameraUniform,
    layers: &[FloorLayer],
) {
    let stride = resources.camera_stride as u64;
    for (i, layer) in layers.iter().enumerate().take(crate::pipeline::MAX_FLOOR_LAYERS) {
        let mut cam = base_camera;
        cam.offset[0] += layer.pixel_offset[0];
        cam.offset[1] += layer.pixel_offset[1];
        cam.atlas_columns = atlas.columns();
        cam.floor_alpha = layer.alpha;
        queue.write_buffer(&resources.camera_buf, i as u64 * stride, bytemuck::cast_slice(&[cam]));
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport_encoder") });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewport_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
