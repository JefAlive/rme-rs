//! Verifica campos de alguns type_ids no appearances.dat (names, sprite ids).
use std::path::Path;

fn main() {
    let assets = std::env::args().nth(1).unwrap_or_else(|| "reference-assets".into());
    let catalog_str = std::fs::read_to_string(Path::new(&assets).join("catalog-content.json")).unwrap();
    let catalog = editor_formats::catalog::parse_catalog(&catalog_str).unwrap();
    let data = std::fs::read(Path::new(&assets).join(&catalog.appearance_file)).unwrap();
    let table = editor_formats::appearances::load_appearances(&data).expect("appearances");

    for id in [0u16, 486, 4601, 4602, 4598, 4628, 1, 100, 1020, 1206, 2148, 2810] {
        let it = table.get(id);
        println!(
            "id={id:5} name={:?} group={:?} sprite_id={} sprite_ids.len={} patterns={}x{}x{} layers={} anim_phases={} shift={:?} elev={}",
            it.name, it.group, it.sprite_id, it.sprite_ids.len(),
            it.pattern_width, it.pattern_height, it.pattern_depth, it.layers,
            it.animation_phases.len(), it.sprite.draw_offset, it.sprite.draw_height,
        );
    }
}