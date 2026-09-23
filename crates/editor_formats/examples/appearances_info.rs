//! Inspeciona o `appearances.dat` do cliente e a tabela `ItemType` resultante.
//!
//! Uso:
//!   cargo run -p editor_formats --example appearances_info -- \
//!       reference-assets/appearances-*.dat

use editor_formats::appearances;
use std::env;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let path = env::args_os().nth(1).unwrap_or_else(|| {
        eprintln!("uso: appearances_info <appearances.dat>");
        std::process::exit(2);
    });
    let path = PathBuf::from(path);

    let data = std::fs::read(&path).expect("não foi possível ler o arquivo");
    eprintln!("arquivo: {} ({} bytes)", path.display(), data.len());

    let t0 = Instant::now();
    let table = appearances::load_appearances(&data).expect("parse de appearances.dat falhou");
    let elapsed = t0.elapsed();

    let parsed = appearances::parse_appearances(&data).unwrap();
    let mut max_obj_id = 0u32;

    println!("== appearances.dat ==");
    println!("objetos: {}", parsed.objects.len());
    println!("item ids máximos: {} (tabela {} entradas)", table.max_item_id, table.items.len());
    println!("parse + build: {:?}", elapsed);

    let mut grounds = 0u32;
    let mut containers = 0u32;
    let mut fluids = 0u32;
    let mut splashes = 0u32;
    let mut no_flags = 0u32;
    let mut animated = 0u32;
    let mut stacked_anim: u64 = 0;
    let mut sprites_total: u64 = 0;
    let mut with_light = 0u32;

    for item in table.items.iter().flatten() {
        match item.group {
            appearances::ItemGroup::Ground => grounds += 1,
            appearances::ItemGroup::Container => containers += 1,
            appearances::ItemGroup::Fluid => fluids += 1,
            appearances::ItemGroup::Splash => splashes += 1,
            appearances::ItemGroup::None_ => {}
        }
        if !item.animation_phases.is_empty() {
            animated += 1;
            stacked_anim += item.animation_phases.len() as u64;
        }
        if item.has_light() {
            with_light += 1;
        }
        sprites_total += item.sprite_ids.len() as u64;
    }

    for obj in &parsed.objects {
        if obj.flags.is_none() {
            no_flags += 1;
        }
        if let Some(id) = obj.id {
            max_obj_id = max_obj_id.max(id);
        }
    }

    println!("maior id de objeto: {}", max_obj_id);
    println!("objects sem flags (ignorados pelo loader): {}", no_flags);

    println!("== ItemType ==");
    println!("ground: {} | container: {} | fluid: {} | splash: {}",
        grounds, containers, fluids, splashes);
    println!("animated: {} ({} fases somadas) | com luz: {}",
        animated, stacked_anim, with_light);
    println!("sprite ids somados: {}", sprites_total);

    // amostra do primeiro ground definido
    if let Some(item) = table.items.iter().flatten().find(|i| i.is_ground_tile()) {
        println!();
        println!("amostra ground: id={} nome={:?}", item.id, item.name);
        println!("  pattern={}x{}x{} layers={} sprite_id={} sprite_ids[0]={:?}",
            item.pattern_width, item.pattern_height, item.pattern_depth, item.layers,
            item.sprite_id,
            item.sprite_ids.first().copied().unwrap_or(0));
        println!("  draw_height={} draw_offset={:?} luz={:?}",
            item.draw_height(), item.draw_offset(), (item.sprite.light_color, item.sprite.light_intensity));
        println!("  amostras sprite_index(0,0,0,0,px):");
        for px in 0..4 {
            let idx = item.sprite_index(0, px, 0, 0, 0);
            println!("    x={} -> índice {} {}", px, idx, item.sprite_ids.get(idx).copied().unwrap_or(0));
        }
    }
}