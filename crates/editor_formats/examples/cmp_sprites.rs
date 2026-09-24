//! Comparação de pixels com o decoder de referência C++
//! (ref_sprite_dump.cpp). Usa SÓ editor_formats, sem assets.rs.
//!
//! Uso: cmp_sprites <assets_dir> <ids.txt> <out_dir>
//!   ids.txt: um sprite_id por linha. Escreve out_dir/<id>.rgba (32x32 RGBA).
use std::path::Path;

const SHEET: u32 = 384;

fn main() {
    let assets = std::env::args().nth(1).unwrap();
    let ids_path = std::env::args().nth(2).unwrap();
    let out_dir = std::env::args().nth(3).unwrap();

    let catalog_str = std::fs::read_to_string(Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();

    let ids: Vec<u32> = std::fs::read_to_string(&ids_path)
        .unwrap()
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect();

    for id in &ids {
        let Some(info) = catalog.sheet_for_sprite(*id) else {
            eprintln!("{id}: sem sheet");
            continue;
        };
        let path = Path::new(&assets).join(&info.file);
        let data = std::fs::read(&path).unwrap();
        let bgra = match editor_formats::sprite::decode_sheet(&data) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{id}: decode falhou: {e}");
                continue;
            }
        };

        let mut rgba = vec![0u8; bgra.len()];
        for (i, chunk) in bgra.chunks_exact(4).enumerate() {
            rgba[i * 4 + 0] = chunk[2];
            rgba[i * 4 + 1] = chunk[1];
            rgba[i * 4 + 2] = chunk[0];
            rgba[i * 4 + 3] = chunk[3];
        }

        let (sw, sh) = info.sprite_type.sprite_size();
        let cols = SHEET / sw;
        let offset = id - info.first_id;
        let col = offset % cols;
        let row = offset / cols;
        let x0 = col * sw;
        let y0 = row * sh;
        let (sx, sy) = match info.sprite_type {
            editor_formats::catalog::SpriteLayout::TwoByOne => (x0 + 32, y0),
            editor_formats::catalog::SpriteLayout::OneByTwo => (x0, y0 + 32),
            _ => (x0, y0),
        };

        let mut cell = [0u8; 32 * 32 * 4];
        let sheet_w = SHEET as usize;
        for dy in 0..32u32 {
            let src_row = ((sy + dy) * SHEET * 4 + sx * 4) as usize;
            let dst_row = (dy * 32 * 4) as usize;
            cell[dst_row..dst_row + 128]
                .copy_from_slice(&rgba[src_row..src_row + 128]);
        }

        std::fs::write(format!("{out_dir}/{id}.rgba"), cell).unwrap();
    }
    println!("{}.sprites=done", ids.len());
}