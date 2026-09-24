//! PROVA pixel-a-pixel em grade: renderiza só o GROUND de janelas 96x96 que
//! cobrem TODO o mapa (por andar), lê de volta e compara CADA célula 32x32
//! com o sprite esperado (decode fresco/independente via decode_sheet). Para
//! células divergentes, procura qual sprite do mapa realmente apareceu.
//!
//! Uso: verify_tiles <assets> <map.otbm> <out_dir> [floors_csv] [win_tiles]
use editor_core::import::{import_otbm, is_subtype_embedded};
use editor_core::position::Position;
use editor_formats::otbm::parse;
use editor_formats::sprite::decode_sheet;
use editor_render::anim::AnimTable;
use editor_render::atlas::SpriteAtlas;
use editor_render::offscreen::OffscreenTarget;
use editor_render::pipeline::{CameraUniform, TileRenderResources};
use editor_render::scene::{render_frame, ChunkGpuCache, FloorLayer};

const TILE: u32 = 32;
const ATLAS4096: u32 = 4096;

fn main() {
    let assets = std::env::args().nth(1).unwrap_or_else(|| "reference-assets".into());
    let map_path = std::env::args().nth(2).unwrap_or_else(|| "reference-maps/Dawnport.otbm".into());
    let out_dir = std::env::args().nth(3).unwrap_or_else(|| "/tmp/rme-out".into());
    let floors: Vec<u8> = std::env::args().nth(4)
        .unwrap_or_else(|| "7".into())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let win_tiles: u32 = std::env::args().nth(5).unwrap_or_else(|| "96".into()).parse().unwrap();
    std::fs::create_dir_all(&out_dir).expect("cria out_dir");

    let mut resolver = editor_render::assets::SpriteResolver::load(&assets).expect("SpriteResolver::load");
    let catalog_str = std::fs::read_to_string(std::path::Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();
    let app_data = std::fs::read(std::path::Path::new(&assets).join(&catalog.appearance_file)).unwrap();
    let table = editor_formats::appearances::load_appearances(&app_data).expect("appearances");

    let otbm = std::fs::read(&map_path).expect("lê otbm");
    let doc = parse(&otbm, |id| is_subtype_embedded(&table, id)).expect("parse otbm");
    let (map_doc, bounds) = import_otbm(&doc, &table);
    eprintln!("bbox x:{}..{} y:{}..{} z:{}..{}", bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y, bounds.min_z, bounds.max_z);
    if let Some(t) = doc.towns.first() {
        eprintln!("town temple: x={} y={} z={}", t.temple.x, t.temple.y, t.temple.z);
    }

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
            label: Some("verify"),
            required_features: adapter.features(),
            required_limits: adapter.limits().clone(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    )).expect("request_device");

    let atlas = SpriteAtlas::new(&device);
    let anim_table = AnimTable::new(&device, &queue);
    let resources = TileRenderResources::new(
        &device, editor_render::offscreen::OFFSCREEN_FORMAT,
        &atlas.bind_group_layout, &anim_table.bind_group_layout,
    );

    // Sync GROUND-ONLY de TODOS os andares (map completo).
    let mut cache = ChunkGpuCache::default();
    let (mut atlas, mut resolver, mut anim_table) = (Some(atlas), Some(resolver), Some(anim_table));
    let mut map = map_doc.map;
    let mut last_anim_1128: Option<u32> = None;
    {
        let mut z = 10u8;
        loop {
            let resolve_visual = |type_id: u16| -> editor_render::assets::ItemVisual {
                let (Some(r), Some(a), Some(t)) = (resolver.as_mut(), atlas.as_mut(), anim_table.as_mut())
                    else { return editor_render::assets::ItemVisual::default(); };
                let v = r.visual_for(&device, &queue, a, t, type_id);
                if type_id == 1128 {
                    last_anim_1128 = Some(v.anim_id);
                }
                v
            };
            cache.sync_ground_floor(&device, &mut map, z, resolve_visual);
            if z == 0 { break; }
            z -= 1;
        }
    }
    let (atlas, _resolver, anim_table) = (atlas.take().unwrap(), resolver.take().unwrap(), anim_table.take().unwrap());
    eprintln!("atlas: {} sprites", atlas.layer_count());

    let target = make_target(&device, win_tiles * TILE, win_tiles * TILE);
    let size = win_tiles * TILE;
    let atlas_img = readback(&device, &queue, atlas.texture(), ATLAS4096, ATLAS4096);

    let mut sheet_cache: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    let mut sprite_ref: std::collections::HashMap<u32, Vec<u8>> = std::collections::HashMap::new();
    let mut by_type: std::collections::BTreeMap<u16, (u32, u32)> = std::collections::BTreeMap::new();
    let mut worst: Vec<((u32, u32), u8, u16, u32, u32, u32)> = Vec::new(); // (x,y), z, type, exp, shown, score
    let mut tested = 0u64; let mut exact = 0u64;
    let mut debug_count = 0u32;

    let nwx = (bounds.max_x - bounds.min_x) as u32 / win_tiles + 1;
    let nwy = (bounds.max_y - bounds.min_y) as u32 / win_tiles + 1;
    let raw_diag = std::env::var("RME_RAW").is_ok();
    for z in &floors {
        for wy in 0..nwy {
            for wx in 0..nwx {
                let ox = bounds.min_x as u32 + wx * win_tiles;
                let oy = bounds.min_y as u32 + wy * win_tiles;
                let camera = CameraUniform {
                    offset: [ox as f32 * TILE as f32, oy as f32 * TILE as f32],
                    zoom: 1.0,
                    time_ms: 0.0,
                    viewport_size: [size as f32, size as f32],
                    floor_alpha: 1.0,
                    _pad: if raw_diag { 1.0 } else { 0.0 },
                };
                let layers = vec![FloorLayer { z: *z, alpha: 1.0, pixel_offset: [0.0, 0.0] }];
                render_frame(&device, &queue, &resources, &cache, &atlas, &anim_table, &target, camera, &layers);
                let img = readback(&device, &queue, &target.texture, size, size);

                for row in 0..win_tiles {
                    for col in 0..win_tiles {
                        let tx = ox + col;
                        let ty = oy + row;
                        if tx > bounds.max_x as u32 || ty > bounds.max_y as u32 { continue; }
                        let pos = Position { x: tx as u16, y: ty as u16, z: *z };
                        let Some(tile) = map.get_tile(pos) else { continue };
                        let Some(ground) = &tile.ground else { continue };
                        let ty_id = ground.type_id;
                        let expected = table.get(ty_id).sprite_ids.first().copied().unwrap_or(0);
                        if expected == 0 { continue; }
                        let cell = cell_rgba(&img, size, col, row);
                        let Some(exp) = get_cell(&assets, &catalog, &mut sheet_cache, &mut sprite_ref, expected) else { continue };
                        tested += 1;
                        let score = similarity(&cell, &exp);
                        let e = by_type.entry(ty_id).or_insert((0, 0));
                        e.0 += 1;
                        if score >= 980 {
                            e.1 += 1;
                            exact += 1;
                        } else if ty_id == 1128 && debug_count < 3 {
                            // diagnóstico do ground 2x2
                            debug_count += 1;
                            let topleft = top_left_crop(&assets, &catalog, &mut sheet_cache, &mut sprite_ref, expected);
                            let mean = mean_rgb(&cell);
                            if let Some(id) = last_anim_1128 {
                                let entry = anim_table.cpu_entries.get(id as usize).copied();
                                let epr = entry.map(|e| (e.first_frame, e.frame_count, e.frame_duration_ms, e.mode));
                                eprintln!("  anim1128={id:?} entry={epr:?}");
                                if let Some(e) = entry {
                                    let frames = &anim_table.cpu_frames
                                        [e.first_frame as usize..(e.first_frame + e.frame_count) as usize];
                                    eprintln!("  frames={frames:?}");
                                }
                            }
                            let slot = atlas.get_slot(expected);
                            let atlas_cell = slot.map(|s| {
                                let col = s % 128;
                                let row = s / 128;
                                cell_rgba(&atlas_img, ATLAS4096, col, row)
                            });
                            if debug_count == 1 {
                                std::fs::write("/tmp/rme-out/cell_rend.bin", &cell).unwrap();
                                std::fs::write("/tmp/rme-out/cell_exp.bin", sprite_ref.get(&expected).unwrap()).unwrap();
                                if let Some(a) = &atlas_cell {
                                    std::fs::write("/tmp/rme-out/cell_atlas.bin", a).unwrap();
                                }
                                std::fs::write("/tmp/rme-out/windbg.bin", &img).unwrap();
                            }
                            eprintln!(
                                "dbg tile({tx},{ty}) z={z} slot={slot:?} score_vs_rescale={score} \
                                 score_vs_topleft={:?} mean_rgb={mean:?} alpha_mean={}",
                                topleft.map(|t| similarity(&cell, &t)),
                                mean_alpha(&cell),
                            );
                        } else {
                            let mut best: Option<(u32, u32)> = None;
                            for (sid, se) in sprite_ref.iter() {
                                let s = similarity(&cell, se);
                                if best.map_or(0, |(_, b)| b) < s {
                                    best = Some((*sid, s));
                                }
                            }
                            if let Some((shown, sc)) = best {
                                worst.push(((tx, ty), *z, ty_id, expected, shown, sc));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut out = String::new();
    out.push_str(&format!("células testadas={tested} exatas={exact} ({:.1}%)\n", 100.0 * exact as f32 / tested.max(1) as f32));
    for (ty_id, (total, ok)) in &by_type {
        out.push_str(&format!(
            "{ty_id:5}\t{:<22}\ttiles={total:3}\tok={ok:3}\t({:.0}%)\t sprite={}\n",
            table.get(*ty_id).name, 100.0 * *ok as f32 / *total as f32, table.get(*ty_id).sprite_id,
        ));
    }
    std::fs::write(format!("{out_dir}/verify.txt"), out).unwrap();

    worst.sort_by(|a, b| b.5.cmp(&a.5));
    let mut w = String::new();
    for (pos, z, ty, exp, shown, score) in worst.iter().take(120) {
        w.push_str(&format!(
            "tile({},{}) z={z} ground={ty} esp={exp} EXIBIU={shown} match={:.3}\n",
            pos.0, pos.1, *score as f32 / 1000.0,
        ));
    }
    std::fs::write(format!("{out_dir}/worst.txt"), w).unwrap();
    eprintln!("OK: verify.txt worst.txt (tested={tested} exact={exact})");
}

fn top_left_crop(
    assets: &str,
    catalog: &editor_formats::catalog::Catalog,
    sheet_cache: &mut std::collections::HashMap<String, Vec<u8>>,
    sprite_ref: &mut std::collections::HashMap<u32, Vec<u8>>,
    sprite_id: u32,
) -> Option<Vec<u8>> {
    let sheet_info = catalog.sheet_for_sprite(sprite_id)?;
    let sheet = if let Some(s) = sheet_cache.get(&sheet_info.file) {
        s.clone()
    } else {
        let data = std::fs::read(std::path::Path::new(assets).join(&sheet_info.file)).ok()?;
        let dec = decode_sheet(&data).ok()?;
        sheet_cache.insert(sheet_info.file.clone(), dec);
        sheet_cache.get(&sheet_info.file).unwrap().clone()
    };
    let idx = sprite_id - sheet_info.first_id;
    let cols = 384 / sheet_info.sprite_type.sprite_size().0;
    let col = idx % cols;
    let row = idx / cols;
    let x0 = col * sheet_info.sprite_type.sprite_size().0;
    let y0 = row * sheet_info.sprite_type.sprite_size().1;
    let mut out = Vec::with_capacity(32 * 32 * 4);
    for dy in 0..32u32 {
        let base = ((y0 + dy) * 384 + x0) as usize * 4;
        out.extend_from_slice(&sheet[base..base + 32 * 4]);
    }
    let _ = sprite_ref;
    Some(out)
}

fn mean_rgb(cell: &[u8]) -> (u8, u8, u8) {
    let mut s = [0u64; 3];
    let mut n = 0u64;
    for i in (0..cell.len()).step_by(4) {
        if cell[i + 3] != 0 {
            s[0] += cell[i] as u64; s[1] += cell[i + 1] as u64; s[2] += cell[i + 2] as u64; n += 1;
        }
    }
    if n == 0 { return (0, 0, 0); }
    ((s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8)
}

fn mean_alpha(cell: &[u8]) -> u8 {
    let mut s = 0u64;
    for i in (3..cell.len()).step_by(4) {
        s += cell[i] as u64;
    }
    (s / (cell.len() / 4) as u64) as u8
}

fn get_cell(
    assets: &str,
    catalog: &editor_formats::catalog::Catalog,
    sheet_cache: &mut std::collections::HashMap<String, Vec<u8>>,
    sprite_ref: &mut std::collections::HashMap<u32, Vec<u8>>,
    sprite_id: u32,
) -> Option<Vec<u8>> {
    if let Some(r) = sprite_ref.get(&sprite_id) { return Some(r.clone()); }
    let sheet_info = catalog.sheet_for_sprite(sprite_id)?;
    let sheet = if let Some(s) = sheet_cache.get(&sheet_info.file) {
        s.clone()
    } else {
        let data = std::fs::read(std::path::Path::new(assets).join(&sheet_info.file)).ok()?;
        let dec = decode_sheet(&data).ok()?;
        sheet_cache.insert(sheet_info.file.clone(), dec);
        sheet_cache.get(&sheet_info.file).unwrap().clone()
    };
    let idx = sprite_id - sheet_info.first_id;
    let cell = extract_cell_bgra(&sheet, idx, sheet_info.sprite_type);
    let mut px = Vec::with_capacity(32 * 32 * 4);
    for i in (0..cell.len()).step_by(4) {
        px.push(cell[i + 2]); px.push(cell[i + 1]); px.push(cell[i]); px.push(cell[i + 3]);
    }
    sprite_ref.insert(sprite_id, px.clone());
    Some(px)
}

fn extract_cell_bgra(sheet: &[u8], idx: u32, layout: editor_formats::catalog::SpriteLayout) -> Vec<u8> {
    use editor_formats::catalog::SpriteLayout as L;
    let (sw, sh) = layout.sprite_size();
    let cols = 384 / sw;
    let col = idx % cols;
    let row = idx / cols;
    let x0 = col * sw;
    let y0 = row * sh;
    let (sx, sy) = match layout {
        L::TwoByOne => (x0 + 32, y0),
        L::OneByTwo => (x0, y0 + 32),
        _ => (x0, y0),
    };
    let mut out = Vec::with_capacity(32 * 32 * 4);
    if layout == L::TwoByTwo {
        for dy in 0..32u32 {
            for dx in 0..32u32 {
                let dst = (dy * 32 + dx) as usize * 4;
                let mut sum = [0u64; 4];
                for yy in 0..2 {
                    for xx in 0..2 {
                        let src = ((y0 + dy * 2 + yy) * 384 + x0 + dx * 2 + xx) as usize * 4;
                        for c in 0..4 {
                            sum[c] += sheet[src + c] as u64;
                        }
                    }
                }
                for c in 0..4 {
                    out.push((sum[c] / 4) as u8);
                }
            }
        }
    } else {
        for dy in 0..32u32 {
            let base = ((sy + dy) * 384 + sx) as usize * 4;
            out.extend_from_slice(&sheet[base..base + 32 * 4]);
        }
    }
    out
}

fn cell_rgba(img: &[u8], size: u32, col: u32, row: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        let base = ((row * 32 + y) * size + col * 32) as usize * 4;
        out.extend_from_slice(&img[base..base + 32 * 4]);
    }
    out
}

fn similarity(a: &[u8], b: &[u8]) -> u32 {
    // Saltar 'saltos': são células com alpha 0 no esperado → o render mostra o
    // clear color (8,13,8,255). O contador é por PIXEL.
    let mut same = 0u64;
    for i in (0..a.len()).step_by(4) {
        let (ar, ag, ab, aa) = (a[i], a[i + 1], a[i + 2], a[i + 3]);
        let (br, bg, bb, ba) = (b[i], b[i + 1], b[i + 2], b[i + 3]);
        let ok = if ba == 0 {
            aa >= 253 && ar.abs_diff(8) <= 3 && ag.abs_diff(13) <= 3 && ab.abs_diff(8) <= 3
        } else {
            aa == ba && ar.abs_diff(br) <= 1 && ag.abs_diff(bg) <= 1 && ab.abs_diff(bb) <= 1
        };
        if ok {
            same += 1;
        }
    }
    (1000 * same / (a.len() / 4) as u64) as u32
}

fn make_target(device: &wgpu::Device, width: u32, height: u32) -> OffscreenTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("verify_target"),
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

fn readback(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
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
    img
}