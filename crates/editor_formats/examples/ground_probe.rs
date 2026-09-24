//! PROBE TEMPORÁRIA (análise de regressão): grounds do andar 7 — quais têm
//! draw_offset/elevation ≠ 0 e quantos sprites possuem. Rodar:
//!   cargo run -p editor_formats --example ground_probe -- reference-assets reference-maps/Dawnport.otbm
use std::collections::BTreeMap;
use std::path::Path;

fn main() {
    let assets = std::env::args().nth(1).unwrap_or_else(|| "reference-assets".into());
    let map = std::env::args().nth(2).unwrap_or_else(|| "reference-maps/Dawnport.otbm".into());

    let catalog_str = std::fs::read_to_string(Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();
    let data = std::fs::read(Path::new(&assets).join(&catalog.appearance_file)).unwrap();
    let table = editor_formats::appearances::load_appearances(&data).expect("appearances");

    let otbm = std::fs::read(&map).unwrap();
    let doc = editor_formats::otbm::parse(&otbm, |_id| false).expect("parse otbm");

    let mut stats: BTreeMap<u16, (u64, [i32; 2], u16, bool, usize, String)> = BTreeMap::new();
    for t in &doc.tiles {
        if t.z != 7 { continue; }
        for item in &t.items {
            let Some(ty) = table.get_opt(item.id) else { continue };
            if ty.group != editor_formats::appearances::ItemGroup::Ground { continue; }
            let e = stats.entry(item.id).or_insert((0, { let d = ty.draw_offset(); [d.0, d.1] }, ty.draw_height(), ty.has_elevation, ty.sprite_ids.len(), ty.name.clone()));
            e.0 += 1;
        }
    }

    let mut nonzero_offset = 0u64;
    let mut with_height = 0u64;
    for (id, (count, off, h, he, nsp, name)) in &stats {
        if *off != [0, 0] { nonzero_offset += 1; }
        if *he { with_height += 1; }
        println!("ground type={id:5} tiles={count:4} draw_offset=({}, {}) draw_height={h} has_elev={he} sprites={nsp:3} {name}", off[0], off[1]);
    }
    println!("\nresumo: types={} com offset≠0={} com has_elevation={}",
        stats.len(), nonzero_offset, with_height);
    for (id, (_, off, h, he, nsp, _)) in &stats {
        if *off != [0, 0] || *he || *nsp > 1 {
            println!("  ATIVO: type={id} offset={off:?} height={h} has_elev={he} sprites={nsp}");
        }
    }
}