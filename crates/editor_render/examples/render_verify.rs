//! PROVE TEMPORÁRIA (regressão de sprites): renderiza o andar 7 GROUND-only
//! via pipeline ATUAL (texture_array atlas + anim table, sem animação) e
//! compara cada célula 32x32 renderizada contra o sprite esperado
//! (`sprite_ids.first()`) decodificado de forma INDEPENDENTE. Igual ao
//! verify_tiles, mas contra a API atual.
//!
//! Uso: cargo run -p editor_render --example render_verify -- reference-assets reference-maps/Dawnport.otbm /tmp/rme-verify
use editor_core::import::{import_otbm, is_subtype_embedded};
use editor_core::position::Position;
use editor_formats::otbm::parse;
use editor_render::anim::AnimTable;
use editor_render::atlas::SpriteAtlas;
use editor_render::offscreen::OffscreenTarget;
use editor_render::pipeline::{CameraUniform, TileRenderResources};
use editor_render::scene::{render_frame, ChunkGpuCache, FloorLayer};

const TILE: u32 = 32;
const WIN: u32 = 96;

fn main() {
    let assets = std::env::args().nth(1).unwrap_or_else(|| "reference-assets".into());
    let map_path = std::env::args().nth(2).unwrap_or_else(|| "reference-maps/Dawnport.otbm".into());
    let out_dir = std::env::args().nth(3).unwrap_or_else(|| "/tmp/rme-verify".into());
    std::fs::create_dir_all(&out_dir).expect("out_dir");

    let mut resolver = editor_render::assets::SpriteResolver::load(&assets).expect("resolver");
    let catalog_str = std::fs::read_to_string(std::path::Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();
    let app_data = std::fs::read(std::path::Path::new(&assets).join(&catalog.appearance_file)).unwrap();
    let table = editor_formats::appearances::load_appearances(&app_data).expect("appearances");

    let otbm = std::fs::read(&map_path).expect("otbm");
    let doc = parse(&otbm, |id| is_subtype_embedded(&table, id)).expect("parse");
    let (map_doc, bounds) = import_otbm(&doc, &table);
    eprintln!("bbox x:{}..{} y:{}..{} z:{}..{}", bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y, bounds.min_z, bounds.max_z);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: None,
    })).expect("adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("rv"),
            required_features: adapter.features(),
            required_limits: adapter.limits().clone(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    )).expect("device");
    eprintln!("adapter layer capacity: {}", device.limits().max_texture_array_layers);

    let atlas = SpriteAtlas::new(&device, &queue);
    let anim_table = AnimTable::new(&device, &queue);
    let resources = TileRenderResources::new(&device, editor_render::offscreen::OFFSCREEN_FORMAT,
        &atlas.bind_group_layout, &anim_table.bind_group_layout);

    let mut cache = ChunkGpuCache::default();
    let (mut atlas, mut resolver, mut anim_table) = (Some(atlas), Some(resolver), Some(anim_table));
    let mut map = map_doc.map;
    {
        let resolve_visual = |type_id: u16| -> editor_render::assets::ItemVisual {
            let (Some(r), Some(a), Some(t)) = (resolver.as_mut(), atlas.as_mut(), anim_table.as_mut())
                else { return editor_render::assets::ItemVisual::default(); };
            r.visual_for(&device, &queue, a, t, type_id)
        };
        cache.sync_for_floor(&device, &mut map, 7, resolve_visual);
    }
    let (atlas, _resolver, anim_table) = (atlas.take().unwrap(), resolver.take().unwrap(), anim_table.take().unwrap());
    eprintln!("atlas layers: {}", atlas.layer_count());

    let (entries, frames) = anim_table.debug_readback(&device, &queue, 256);
    let mut log = String::new();
    for (i, e) in entries.iter().enumerate() {
        log.push_str(&format!("anim[{i}] first_frame={} frame_count={} dur={} mode={} -> frame[{}]={}\n",
            e.first_frame, e.frame_count, e.frame_duration_ms, e.mode,
            e.first_frame, frames.get(e.first_frame as usize).copied().unwrap_or(0)));
    }
    std::fs::write(format!("{out_dir}/anim_dbg.txt"), log).unwrap();
    let layers_n = atlas.debug_sprite_layer().values().copied().max().unwrap_or(0) + 1;
    let atlas_img = readback_array(&device, &queue, atlas.debug_texture(), layers_n);
    let mut alog = String::new();
    for l in 0..layers_n.min(16) {
        let base = (l as usize) * 32 * 32 * 4;
        let px = &atlas_img[base..base + 4];
        let opaque = atlas_img[base..base + 32 * 32 * 4].chunks(4).filter(|p| p[3] != 0).count();
        alog.push_str(&format!("layer[{l}] first_px=({},{},{},{}) opaque_px={}\n", px[0], px[1], px[2], px[3], opaque));
    }
    std::fs::write(format!("{out_dir}/atlas_dbg.txt"), alog).unwrap();
    eprintln!("debug anim+atlas table dumped");

    let size = WIN * TILE;
    let target = make_target(&device, size, size);

    let mut sheet_cache: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    let mut sprite_ref: std::collections::HashMap<u32, Vec<u8>> = std::collections::HashMap::new();
    let mut by_type: std::collections::BTreeMap<u16, (u64, u64)> = std::collections::BTreeMap::new();
    let mut worst: Vec<(u32, u32, u16, u32, u32, u32)> = Vec::new();
    let (mut tested, mut exact) = (0u64, 0u64);
    let mut dbg_cells: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    let mut skipped_with_items: u64 = 0;

    let mut mismatch_decode = 0u64;
    let mut checked_decode = 0u64;
    for (sprite_id, layer) in atlas.debug_sprite_layer() {
        let Some(exp) = get_cell(&assets, &catalog, &mut sheet_cache, &mut sprite_ref, *sprite_id) else { continue };
        checked_decode += 1;
        let base = (*layer as usize) * 32 * 32 * 4;
        let got = &atlas_img[base..base + 32 * 32 * 4];
        if got != exp.as_slice() {
            mismatch_decode += 1;
            if mismatch_decode <= 8 {
                let mut diffs = 0;
                for i in 0..exp.len() {
                    if got[i] != exp[i] { diffs += 1; }
                }
                eprintln!("DECODE MISMATCH sprite={sprite_id} layer={layer} byte_diffs={diffs} expected_first=({},{},{},{}) atlas_first=({},{},{},{})",
                    exp[0], exp[1], exp[2], exp[3], got[0], got[1], got[2], got[3]);
            }
        }
    }
    eprintln!("decode check: {checked_decode} sprites, {mismatch_decode} mismatches vs independent extract");

    let nwx = (bounds.max_x as i64 - bounds.min_x as i64) / WIN as i64 + 1;
    let nwy = (bounds.max_y as i64 - bounds.min_y as i64) / WIN as i64 + 1;
    for wy in 0..nwy {
        for wx in 0..nwx {
            let ox = bounds.min_x as u32 + (wx * WIN as i64) as u32;
            let oy = bounds.min_y as u32 + (wy * WIN as i64) as u32;
            let camera = CameraUniform {
                offset: [ox as f32 * TILE as f32, oy as f32 * TILE as f32],
                zoom: 1.0, time_ms: 0.0,
                viewport_size: [size as f32, size as f32],
                floor_alpha: 1.0, _pad: 0.0,
            };
            let layers = vec![FloorLayer { z: 7, alpha: 1.0, pixel_offset: [0.0, 0.0] }];
            render_frame(&device, &queue, &resources, &cache, &atlas, &anim_table, &target, camera, &layers);
            let img = readback(&device, &queue, &target.texture, size, size);

            for row in 0..WIN {
                for col in 0..WIN {
                    let tx = ox + col;
                    let ty = oy + row;
                    if tx > bounds.max_x as u32 || ty > bounds.max_y as u32 { continue; }
                    let Some(tile) = map.get_tile(Position { x: tx as u16, y: ty as u16, z: 7 }) else { continue };
                    let Some(ground) = &tile.ground else { continue };
                    if !tile.items.is_empty() {
                        skipped_with_items += 1;
                        continue;
                    }
                    let ty_id = ground.type_id;
                    let expected = table.get(ty_id).sprite_ids.first().copied().unwrap_or(0);
                    if expected == 0 { continue; }
                    let cell = cell_rgba(&img, size, col, row);
                    let Some(exp) = get_cell(&assets, &catalog, &mut sheet_cache, &mut sprite_ref, expected) else { continue };
                    tested += 1;
                    let score = similarity(&cell, &exp);
                    if dbg_cells.len() < 3 { dbg_cells.push((tx, ty, cell.clone())); }
                    let e = by_type.entry(ty_id).or_default();
                    e.0 += 1;
                    if score >= 980 { e.1 += 1; exact += 1; }
                    else if worst.len() < 40 {
                        let mut shown = 0;
                        for (sid, se) in sprite_ref.iter() {
                            let s = similarity(&cell, se);
                            let si = *sid;
                            if s > shown { shown = si; }
                        }
                        worst.push((tx, ty, ty_id, expected, shown, score));
                    }
                }
            }
        }
    }

    let mut out = format!(
        "células testadas={tested} exatas={exact} ({:.1}%)\natlas_layers={}\n",
        100.0 * exact as f32 / tested.max(1) as f32, atlas.layer_count()
    );
    for (ty_id, (total, ok)) in &by_type {
        out.push_str(&format!(
            "{ty_id:5}\t{:<22}\ttiles={total:4}\tok={ok:4}\t({:.1}%)\tsprite={}\n",
            table.get(*ty_id).name, 100.0 * *ok as f32 / *total as f32, table.get(*ty_id).sprite_id,
        ));
    }
    std::fs::write(format!("{out_dir}/report.txt"), out).unwrap();
    let mut w = String::new();
    for (px, py, ty, exp, shown, score) in &worst {
        w.push_str(&format!("tile({px},{py}) ground={ty} esperado={exp} exibiu={shown} match={:.3}\n", *score as f32 / 1000.0));
    }
    std::fs::write(format!("{out_dir}/worst.txt"), w).unwrap();

    let mut dbg = String::new();
    for (tx, ty, c) in &dbg_cells {
        let mut sums = [0u64; 4];
        for i in (0..c.len()).step_by(4) {
            for k in 0..4 { sums[k] += c[i + k] as u64; }
        }
        let n = c.len() / 4;
        let mut mins = [255u8; 3];
        let mut maxs = [0u8; 3];
        let mut distinct: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        for i in (0..c.len()).step_by(4) {
            for k in 0..3 {
                mins[k] = mins[k].min(c[i + k]);
                maxs[k] = maxs[k].max(c[i + k]);
            }
            distinct.insert(((c[i] as u32) << 16) | ((c[i + 1] as u32) << 8) | c[i + 2] as u32);
        }
        dbg.push_str(&format!(
            "tile({tx},{ty}) render mean rgba=({:.0},{:.0},{:.0},{:.0}) alpha0x={} range={:?} distinct_px={}\n",
            sums[0] as f32 / n as f32, sums[1] as f32 / n as f32,
            sums[2] as f32 / n as f32, sums[3] as f32 / n as f32,
            c.chunks(4).filter(|px| px[3] == 0).count(),
            (mins, maxs), distinct.len(),
        ));
    }
    std::fs::write(format!("{out_dir}/dbg.txt"), dbg).unwrap();

    eprintln!("OK: {} (tested={tested} exact={exact} layers={})", out_dir, atlas.layer_count());
}

fn make_target(device: &wgpu::Device, w: u32, h: u32) -> OffscreenTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rv_target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: editor_render::offscreen::OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    OffscreenTarget { texture, view, id: egui::TextureId::default(), width: w, height: h }
}

fn readback(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    let bpr = w * 4;
    let aligned = (bpr + 255) / 256 * 256;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rv_readback"),
        size: aligned as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer { buffer: &staging, layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(aligned), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let mut img = vec![0u8; (w * h * 4) as usize];
    for row in 0..h as usize {
        let src = &data[row * aligned as usize..row * aligned as usize + bpr as usize];
        img[row * bpr as usize..(row + 1) * bpr as usize].copy_from_slice(src);
    }
    drop(data);
    img
}

fn readback_array(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, layers: u32) -> Vec<u8> {
    let w = 32u32;
    let h = 32u32;
    let bpr = w * 4;
    let aligned = (bpr + 255) / 256 * 256;
    let total_rows = h * layers;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rv_atlas_readback"),
        size: aligned as u64 * total_rows as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &staging,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(aligned), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers },
    );
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let mut img = vec![0u8; (h * layers * bpr) as usize];
    for row in 0..total_rows as usize {
        let src = &data[row * aligned as usize..row * aligned as usize + bpr as usize];
        img[row * bpr as usize..(row + 1) * bpr as usize].copy_from_slice(src);
    }
    drop(data);
    img
}

fn cell_rgba(img: &[u8], size: u32, col: u32, row: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        let base = ((row * 32 + y) * size + col * 32) as usize * 4;
        out.extend_from_slice(&img[base..base + 32 * 4]);
    }
    out
}

fn get_cell(
    assets: &str,
    catalog: &editor_formats::catalog::Catalog,
    sheet_cache: &mut std::collections::HashMap<String, Vec<u8>>,
    sprite_ref: &mut std::collections::HashMap<u32, Vec<u8>>,
    sprite_id: u32,
) -> Option<Vec<u8>> {
    if let Some(r) = sprite_ref.get(&sprite_id) { return Some(r.clone()); }
    let si = catalog.sheet_for_sprite(sprite_id)?;
    let sheet = if let Some(s) = sheet_cache.get(&si.file) {
        s.clone()
    } else {
        let data = std::fs::read(std::path::Path::new(assets).join(&si.file)).ok()?;
        let bgra = editor_formats::sprite::decode_sheet(&data).ok()?;
        let mut rgba = vec![0u8; bgra.len()];
        for (i, chunk) in bgra.chunks_exact(4).enumerate() {
            let idx = i * 4;
            rgba[idx] = chunk[2];
            rgba[idx + 1] = chunk[1];
            rgba[idx + 2] = chunk[0];
            rgba[idx + 3] = chunk[3];
        }
        sheet_cache.insert(si.file.clone(), rgba);
        sheet_cache.get(&si.file).unwrap().clone()
    };
    let idx = sprite_id - si.first_id;
    let (sw, sh) = si.sprite_type.sprite_size();
    let cols = 384 / sw;
    let col = idx % cols;
    let row = idx / cols;
    let x0 = col * sw;
    let y0 = row * sh;
    let (sx, sy) = match si.sprite_type {
        editor_formats::catalog::SpriteLayout::TwoByOne => (x0 + 32, y0),
        editor_formats::catalog::SpriteLayout::OneByTwo => (x0, y0 + 32),
        _ => (x0, y0),
    };
    let mut px = Vec::with_capacity(32 * 32 * 4);
    for dy in 0..32u32 {
        let base = ((sy + dy) * 384 + sx) as usize * 4;
        for c in 0..32 * 4 {
            px.push(sheet[base + c as usize]);
        }
    }
    sprite_ref.insert(sprite_id, px.clone());
    Some(px)
}

fn srgb_to_linear(c: u8) -> u8 {
    let f = c as f32 / 255.0;
    let l = if f <= 0.04045 { f / 12.92 } else { ((f + 0.055) / 1.055).powf(2.4) };
    (l * 255.0).round() as u8
}

fn similarity(a: &[u8], b: &[u8]) -> u32 {
    let mut same = 0u64;
    for i in (0..a.len()).step_by(4) {
        let (ar, ag, ab, aa) = (a[i], a[i + 1], a[i + 2], a[i + 3]);
        let (br, bg, bb, ba) = (b[i], b[i + 1], b[i + 2], b[i + 3]);
        if ba == 0 {
            if aa >= 253 && ar.abs_diff(8) <= 3 && ag.abs_diff(13) <= 3 && ab.abs_diff(8) <= 3 {
                same += 1;
            }
        } else {
            let br = srgb_to_linear(br);
            let bg = srgb_to_linear(bg);
            let bb = srgb_to_linear(bb);
            if aa == ba && ar.abs_diff(br) <= 2 && ag.abs_diff(bg) <= 2 && ab.abs_diff(bb) <= 2 {
                same += 1;
            }
        }
    }
    (1000 * same / (a.len() / 4) as u64) as u32
}