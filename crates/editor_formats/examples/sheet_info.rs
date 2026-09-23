//! Lê o `catalog-content.json`, decodifica sheets e valida a extração de um
//! sprite pelo seu id.
//!
//! Uso:
//!   cargo run -p editor_formats --example sheet_info -- reference-assets

use editor_formats::catalog::{parse_catalog, SpriteSheetInfo};
use editor_formats::sprite::{decode_sheet, SHEET_SIZE};
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct SheetCache {
    path: PathBuf,
    info: SpriteSheetInfo,
}

fn main() {
    let assets = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("reference-assets"));

    let catalog_path = assets.join("catalog-content.json");
    let catalog_str = std::fs::read_to_string(&catalog_path)
        .unwrap_or_else(|_| panic!("não foi possível ler {}", catalog_path.display()));
    let t0 = Instant::now();
    let catalog = parse_catalog(&catalog_str).expect("parse do catálogo falhou");
    println!("catálogo: {} sheets | sprites (max id): {} | appearances: {}",
        catalog.sheets.len(), catalog.sprites_count, catalog.appearance_file);
    println!("parse catálogo: {:?}", t0.elapsed());

    // Distribuição de layouts de sprite
    let mut layouts = [0u32; 4];
    for s in &catalog.sheets {
        layouts[s.sprite_type as usize] += 1;
    }
    println!("layouts: 1x1={} 1x2={} 2x1={} 2x2={}", layouts[0], layouts[1], layouts[2], layouts[3]);

    let first = &catalog.sheets[0];
    println!();
    println!("== sheet de exemplo: id {}-{} ({}) ==",
        first.first_id, first.last_id, first.file);

    let t = Instant::now();
    let sheet_path = assets.join(&first.file);
    let raw = std::fs::read(&sheet_path).expect("arquivo de sheet não encontrado");
    eprintln!("   lido {} bytes em {:?}", raw.len(), t.elapsed());

    let t = Instant::now();
    let pixels = decode_sheet(&raw).expect("decode da sheet falhou");
    println!("   decode: {:?} ({} bytes de pixels)", t.elapsed(), pixels.len());
    assert_eq!(pixels.len(), (SHEET_SIZE * SHEET_SIZE * 4) as usize);

    // O sprite 0 deve ser o canto superior esquerdo; verificar 4 cantos p/ alpha
    let checker = |x: usize, y: usize| {
        let i = (y * SHEET_SIZE as usize + x) * 4;
        (pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3])
    };
    println!("   pixel(0,0) BGRA={:?}", checker(0, 0));
    println!("   pixel(32,32) BGRA={:?}", checker(32, 32));
    println!("   pixel(383,383) BGRA={:?}", checker(383, 383));

    // que sheet contém o sprite 197909 (ground id=100 do appearances)?
    let target = 197909u32;
    if let Some(sheet) = catalog.sheet_for_sprite(target) {
        println!();
        println!("== sprite {} pertence à sheet {}-{} ({}) ==",
            target, sheet.first_id, sheet.last_id, sheet.file);
        let sheet_path = assets.join(&sheet.file);
        let raw = std::fs::read(&sheet_path).expect("sheet não encontrada");
        let pixels = decode_sheet(&raw).expect("decode falhou");
        let (sw, sh) = sheet.sprite_type.sprite_size();
        let cols = SHEET_SIZE / sw;
        let offset = target - sheet.first_id;
        let col = offset % cols;
        let row = offset / cols;
        let x0 = col * sw;
        let y0 = row * sh;
        println!("   cell {}x{}  uv region ({}, {}) a ({}, {})", col, row, x0, y0, x0 + sw, y0 + sh);
        let i = ((y0 + sh / 2) as usize * SHEET_SIZE as usize + x0 as usize) * 4;
        println!("   pixel centro: BGRA={:?}", (pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]));
    }

    // benchmark de decodificação de uma amostra de sheets
    let n = env::var("SHEETS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let sheets: Vec<_> = catalog
        .sheets
        .iter()
        .map(|s| SheetCache {
            path: assets.join(&s.file),
            info: s.clone(),
        })
        .collect();

    let mut total = Duration::ZERO;
    let mut ok = 0usize;
    for sheet in &sheets[..sheets.len().min(n as usize)] {
        let raw = std::fs::read(&sheet.path).unwrap();
        let t = Instant::now();
        match decode_sheet(&raw) {
            Ok(p) => {
                ok += 1;
                total += t.elapsed();
                let _ = p;
            }
            Err(e) => eprintln!("  falha em {}: {}", sheet.info.file, e),
        }
    }
    println!();
    println!("== benchmark: {} sheets decodificadas (amostra {}) ==", ok, n);
    println!("tempo total: {:?} | média por sheet: {:?}", total, total / ok.max(1) as u32);
    let total_bytes: u64 = SHEET_SIZE as u64 * SHEET_SIZE as u64 * 4;
    let total_data = total_bytes * ok as u64;
    let secs = total.as_secs_f64();
    if secs > 0.0 {
        println!("throughput: {:.1} MB/s", total_data as f64 / 1e6 / secs);
    }
    let _ = Path::new("");
}