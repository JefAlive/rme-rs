//! Rastreia o mapeamento type_id → anim_id → slot → sprite (CIP) para os
//! type_ids denunciados como "sprites trocados" (ex: sand 231 mostrando o
//! sprite do item 3112).
//!
//! Uso: probe_types <assets> <typeid1,typeid2,...>
use std::path::Path;

use editor_render::anim::AnimTable;
use editor_render::assets::SpriteResolver;
use editor_render::atlas::SpriteAtlas;

fn main() {
    let assets = std::env::args().nth(1).expect("assets dir");
    let type_ids: Vec<u16> = std::env::args()
        .nth(2)
        .expect("type ids csv")
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let catalog_str =
        std::fs::read_to_string(Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();
    let data = std::fs::read(Path::new(&assets).join(&catalog.appearance_file)).unwrap();
    let table = editor_formats::appearances::load_appearances(&data).expect("appearances");

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: true,
        compatible_surface: None,
    })).expect("no adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("probe"),
            required_features: adapter.features(),
            required_limits: adapter.limits().clone(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    )).expect("request_device");

    let mut atlas = SpriteAtlas::new(&device);
    let mut anim_table = AnimTable::new(&device, &queue);
    let mut resolver = SpriteResolver::load(&assets).unwrap();

    for ty in type_ids {
        let it = table.get(ty);
        let expected_sprite = it.sprite_ids.first().copied().unwrap_or(0);
        let vis = resolver.visual_for(&device, &queue, &mut atlas, &mut anim_table, ty);
        let slot = anim_table.debug_first_slot(vis.anim_id);
        let slot_sprite = slot.map(|s| atlas.slot_sprite(s));
        let name = if it.name.is_empty() { "(sem nome)" } else { &it.name };
        println!(
            "type={ty:5} {name:<24} expected_sprite={:>7} anim_id={:>6} slot={:>5?} slot_sprite={:>7?} diff={}",
            expected_sprite,
            vis.anim_id,
            slot,
            slot_sprite,
            slot_sprite != Some(expected_sprite),
        );
    }
}