//! Harness headless de debug: reproduz EXATAMENTE o pipeline do editor
//! (SpriteResolver -> atlas -> ChunkGpuCache -> render_frame) e grava o
//! resultado em PNG, sem precisar de janela/eframe.
//!
//! Uso:
//!   cargo run -p editor_render --example render_map -- reference-assets reference-maps/Dawnport.otbm /tmp/rme-out
//!
//! Saída em OUT_DIR:
//!   viewport.png     - o que o editor mostraria (mesma camera/stack do tabs.rs)
//!   atlas.png        - dump do atlas 4096x4096 (todos os sprites inseridos)
//!   log.txt          - stats do resolver + mapa type_id -> sprite_id

use editor_core::import::{import_otbm, is_subtype_embedded};
use editor_formats::otbm::parse;
use editor_render::anim::AnimTable;
use editor_render::atlas::SpriteAtlas;
use editor_render::offscreen::OffscreenTarget;
use editor_render::pipeline::{CameraUniform, TileRenderResources};
use editor_render::scene::{render_frame, ChunkGpuCache, FloorLayer};

const FLOOR: u8 = 7;
const ATLAS_SIZE: u32 = 4096;

fn main() {
    let assets = std::env::args().nth(1).unwrap_or_else(|| "reference-assets".into());
    let map_path = std::env::args().nth(2).unwrap_or_else(|| "reference-maps/Dawnport.otbm".into());
    let out_dir = std::env::args().nth(3).unwrap_or_else(|| "/tmp/rme-out".into());
    std::fs::create_dir_all(&out_dir).expect("cria out_dir");

    // 1) Assets: appearances.dat + catalog-content.json + sheets
    let mut resolver = editor_render::assets::SpriteResolver::load(&assets).expect("SpriteResolver::load");
    let catalog_str = std::fs::read_to_string(std::path::Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();
    let app_data = std::fs::read(std::path::Path::new(&assets).join(&catalog.appearance_file)).unwrap();
    let table = editor_formats::appearances::load_appearances(&app_data).expect("appearances");

    // 2) Mapa OTBM
    let otbm = std::fs::read(&map_path).expect("lê otbm");
    let doc = parse(&otbm, |id| is_subtype_embedded(&table, id)).expect("parse otbm");
    let (map_doc, bounds) = import_otbm(&doc, &table);
    eprintln!("mapa: bbox=({}..{}, {}..{}) z={}..{} tiles={}",
        bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y,
        bounds.min_z, bounds.max_z, doc.tiles.len());

    // 3) wgpu headless (lavapipe / software)
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: None,
    })).expect("nenhum adapter wgpu (instale lavapipe/vulkan software)");
    let info = adapter.get_info();
    eprintln!("adapter: {:?} | {:?}", info.backend, info.name);
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("headless"),
            required_features: adapter.features(),
            required_limits: adapter.limits().clone(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    )).expect("request_device");

    // 4) Recursos do renderer, idênticos ao tabs.rs
    let atlas = SpriteAtlas::new(&device);
    let anim_table = AnimTable::new(&device, &queue);
    let resources = TileRenderResources::new(
        &device, editor_render::offscreen::OFFSCREEN_FORMAT,
        &atlas.bind_group_layout, &anim_table.bind_group_layout,
    );

    // 5) Sincroniza floor stack do editor (start=8 .. superend=0 no floor 7)
    let mut cache = ChunkGpuCache::default();
    let (_start, _end, superend) = (8u8, FLOOR, 0u8);
    let (mut atlas, mut resolver, mut anim_table) = (Some(atlas), Some(resolver), Some(anim_table));
    let mut map = map_doc.map;
    {
        let mut z = _start;
        loop {
            let resolve_visual = |type_id: u16| -> editor_render::assets::ItemVisual {
                let (Some(resolver), Some(atlas), Some(anim_table)) =
                    (resolver.as_mut(), atlas.as_mut(), anim_table.as_mut()) else {
                    return editor_render::assets::ItemVisual::default();
                };
                resolver.visual_for(&device, &queue, atlas, anim_table, type_id)
            };
            cache.sync_for_floor(&device, &mut map, z, resolve_visual);
            if z == superend { break; }
            z -= 1;
        }
    }
    let (atlas, resolver, anim_table) = (atlas.take().unwrap(), resolver.take().unwrap(), anim_table.take().unwrap());

    // 6) Dump do atlas + log
    dump_atlas_png(&device, &queue, &atlas, &format!("{out_dir}/atlas.png"));
    let mut log_str = format!(
        "resolver: resolved={} cache_hits={} decode_failures={}\n",
        resolver.resolved(), resolver.cache_hits(), resolver.decode_failures(),
    );
    for (ty_id, name, sprite_id) in distinct_ground_sprites(&map, &table) {
        log_str.push_str(&format!("ground\t{ty_id}\t{name}\t{sprite_id}\n"));
    }
    for sprite_id in distinct_item_sprites(&map, &table) {
        log_str.push_str(&format!("item_sprite\t{sprite_id}\n"));
    }
    std::fs::write(format!("{out_dir}/log.txt"), log_str).unwrap();

    // 7) Janela de render: crop ao redor do template da 1ª town
    let town_pos = doc.towns.first().map(|t| t.temple);
    let (cx, cy) = town_pos.map(|p| (p.x as f32, p.y as f32)).unwrap_or((32000.0, 31800.0));
    let half_tiles = 48.0f32;
    let map_x0 = (cx - half_tiles).max(0.0);
    let map_y0 = (cy - half_tiles).max(0.0);
    let width = (half_tiles * 2.0 * 32.0) as u32;
    let height = (half_tiles * 2.0 * 32.0) as u32;

    // 8) Target de render + camera (idêntico ao shader/camera do editor)
    let target = make_target(&device, width, height);
    let layers = vec![FloorLayer { z: FLOOR, alpha: 1.0, pixel_offset: [0.0, 0.0] }];
    let camera = CameraUniform {
        offset: [map_x0 * 32.0, map_y0 * 32.0],
        zoom: 1.0,
        time_ms: 0.0,
        viewport_size: [width as f32, height as f32],
        floor_alpha: 1.0,
        _pad: 0.0,
    };
    render_frame(&device, &queue, &resources, &cache, &atlas, &anim_table, &target, camera, &layers);

    // 9) Readback -> PNG
    readback_png(&device, &queue, &target.texture, width, height, &format!("{out_dir}/viewport.png"));
    eprintln!("OK: {out_dir}/viewport.png, atlas.png, log.txt");
}

fn make_target(device: &wgpu::Device, width: u32, height: u32) -> OffscreenTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless_target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: editor_render::offscreen::OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    OffscreenTarget { texture, view, id: egui::TextureId::default(), width, height }
}

fn readback_png(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, w: u32, h: u32, path: &str) {
    let bytes_per_row = w * 4;
    let aligned = (bytes_per_row + 255) / 256 * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: aligned as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(aligned), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let mut img = vec![0u8; (w * h * 4) as usize];
    for row in 0..h as usize {
        let src = &data[row * aligned as usize..row * aligned as usize + bytes_per_row as usize];
        img[row * bytes_per_row as usize..(row + 1) * bytes_per_row as usize].copy_from_slice(src);
    }
    drop(data);
    write_png(path, w, h, &img);
}

fn dump_atlas_png(device: &wgpu::Device, queue: &wgpu::Queue, atlas: &SpriteAtlas, path: &str) {
    let size = ATLAS_SIZE;
    let bytes_per_row = size * 4;
    let aligned = (bytes_per_row + 255) / 256 * 256;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("atlas_readback"),
        size: aligned as u64 * size as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture: atlas.texture(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &staging,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(aligned), rows_per_image: Some(size) },
        },
        wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let mut img = vec![0u8; (size * size * 4) as usize];
    for row in 0..size as usize {
        let src = &data[row * aligned as usize..row * aligned as usize + bytes_per_row as usize];
        img[row * bytes_per_row as usize..(row + 1) * bytes_per_row as usize].copy_from_slice(src);
    }
    drop(data);
    write_png(path, size, size, &img);
    eprintln!("atlas: {} sprites únicos em cache", atlas.layer_count());
}

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("cria png");
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
}

/// Tipo_id dos grounds distintos no andar 7 + nome + sprite principal.
fn distinct_ground_sprites(
    map: &editor_core::spatial_map::SpatialMap,
    table: &editor_formats::appearances::ItemTypeTable,
) -> Vec<(u16, String, u32)> {
    use editor_core::position::{Position, CHUNK_SIZE};
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for (coord, _) in map.iter_chunk_coords() {
        if coord.z != 7 { continue; }
        for local_idx in 0..(CHUNK_SIZE as usize * CHUNK_SIZE as usize) {
            let lx = (local_idx % CHUNK_SIZE as usize) as u16;
            let ly = (local_idx / CHUNK_SIZE as usize) as u16;
            let pos = Position { x: coord.cx as u16 * CHUNK_SIZE + lx, y: coord.cy as u16 * CHUNK_SIZE + ly, z: coord.z };
            if let Some(t) = map.get_tile(pos) {
                if let Some(g) = &t.ground {
                    if seen.insert(g.type_id) {
                        let name = table.get(g.type_id).name.clone();
                        let sp = table.get(g.type_id).sprite_id;
                        out.push((g.type_id, name, sp));
                    }
                }
            }
        }
    }
    out
}

/// Sprites principais (sprite_id) de itens não-ground distintos no andar 7.
fn distinct_item_sprites(
    map: &editor_core::spatial_map::SpatialMap,
    table: &editor_formats::appearances::ItemTypeTable,
) -> Vec<u32> {
    use editor_core::position::{Position, CHUNK_SIZE};
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for (coord, _) in map.iter_chunk_coords() {
        if coord.z != 7 { continue; }
        for local_idx in 0..(CHUNK_SIZE as usize * CHUNK_SIZE as usize) {
            let lx = (local_idx % CHUNK_SIZE as usize) as u16;
            let ly = (local_idx / CHUNK_SIZE as usize) as u16;
            let pos = Position { x: coord.cx as u16 * CHUNK_SIZE + lx, y: coord.cy as u16 * CHUNK_SIZE + ly, z: coord.z };
            if let Some(t) = map.get_tile(pos) {
                for it in &t.items {
                    let sp = table.get(it.type_id).sprite_id;
                    if sp != 0 && seen.insert((it.type_id, sp)) {
                        out.push(sp);
                    }
                }
            }
        }
    }
    out
}