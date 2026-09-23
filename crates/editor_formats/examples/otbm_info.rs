//! Inspeciona um arquivo `.otbm`:
//!   cargo run -p editor_formats --example otbm_info -- <arquivo>
//!
//! Valida o parser contra um mapa real e imprime estatísticas (versão, bbox
//! por andar, contagens, warnings) sem depender do `appearances.dat`.

use std::collections::BTreeMap;
use std::time::Instant;

use editor_formats::otbm;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("uso: otbm_info <arquivo.otbm>");
        std::process::exit(2);
    });
    let data = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("erro ao ler {path}: {e}");
        std::process::exit(2);
    });

    let start = Instant::now();
    // Sem tabela de itens: para OTBM v1 o count embutido não será interpretado.
    let doc = match otbm::parse(&data, |_id| false) {
        Ok(doc) => doc,
        Err(e) => {
            eprintln!("falha no parse de {path}: {e}");
            std::process::exit(1);
        }
    };
    let elapsed = start.elapsed();

    println!("arquivo : {path} ({} bytes)", data.len());
    println!("versão  : {} (v{})", doc.version, match doc.version {
        otbm::MAP_OTBM_1 => "1",
        otbm::MAP_OTBM_2 => "2",
        otbm::MAP_OTBM_3 => "3",
        otbm::MAP_OTBM_4 => "4",
        otbm::MAP_OTBM_5 => "5",
        otbm::MAP_OTBM_6 => "6",
        _ => "?",
    });
    println!("tamanho : {} x {} tiles", doc.width, doc.height);
    println!("parsed em {:.1} ms", elapsed.as_secs_f64() * 1000.0);

    if !doc.description.is_empty() {
        println!("descrição: {}", doc.description);
    }
    for (label, opt) in [
        ("monster", &doc.spawn_monster_file),
        ("house", &doc.house_file),
        ("zone", &doc.zone_file),
        ("npc", &doc.spawn_npc_file),
    ] {
        if let Some(file) = opt {
            println!("[{label}] {file}");
        }
    }

    // Bbox e contagens por andar — ordem crescente de profundidade.
    let mut per_floor: BTreeMap<u8, FloorStats> = BTreeMap::new();
    for tile in &doc.tiles {
        let stats = per_floor.entry(tile.z).or_default();
        stats.count += 1;
        stats.items += tile.items.len() as u64;
        stats.min_x = stats.min_x.min(tile.x);
        stats.min_y = stats.min_y.min(tile.y);
        stats.max_x = stats.max_x.max(tile.x);
        stats.max_y = stats.max_y.max(tile.y);
        if tile.house_id != 0 {
            stats.house_tiles += 1;
        }
    }

    println!("\nandares ({})", per_floor.len());
    for (z, s) in &per_floor {
        println!(
            "  z={z:>2}: {} tiles, {} itens, x[{}-{}] y[{}-{}], {} house-tiles",
            s.count, s.items, s.min_x, s.max_x, s.min_y, s.max_y, s.house_tiles
        );
    }

    println!("\ntowns: {}", doc.towns.len());
    for town in &doc.towns {
        println!("  #{} {} — templo em {}:{:?}", town.id, town.name, pos(&town.temple), town.temple);
    }
    println!("waypoints: {}", doc.waypoints.len());

    println!("\nwarnings: {} (mostrando as 10 primeiras)", doc.warnings.len());
    for w in doc.warnings.iter().take(10) {
        println!("  ! {w}");
    }
}

fn pos(p: &otbm::OtmPos) -> String {
    format!("{},{}", p.x, p.y)
}

struct FloorStats {
    count: u64,
    items: u64,
    house_tiles: u64,
    min_x: u16,
    min_y: u16,
    max_x: u16,
    max_y: u16,
}

impl Default for FloorStats {
    fn default() -> Self {
        Self { count: 0, items: 0, house_tiles: 0, min_x: u16::MAX, min_y: u16::MAX, max_x: 0, max_y: 0 }
    }
}