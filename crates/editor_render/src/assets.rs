use std::path::{Path, PathBuf};
use std::sync::Arc;

use ahash::AHashMap;
use editor_formats::appearances::{ItemTypeTable, load_appearances};
use editor_formats::catalog::{Catalog, parse_catalog, SpriteSheetInfo};
use editor_formats::sprite::{decode_sheet, SHEET_SIZE};

use crate::anim::AnimTable;
use crate::atlas::SpriteAtlas;

struct DecodedSheet {
    pixels_rgba: Vec<u8>,
    layout: editor_formats::catalog::SpriteLayout,
    first_id: u32,
}

struct DecodedSpriteCell {
    rgba: [u8; 32 * 32 * 4],
    sheet_file: String,
    layout: editor_formats::catalog::SpriteLayout,
    cell_x: usize,
    cell_y: usize,
}

/// Dados visuais resolvidos de um item, prontos para virar `TileInstance`:
/// `anim_id` aponta pra `AnimTable` (estático = 1 frame, animado = N),
/// `draw_offset` é o "shift" fixo do sprite (appearances.dat) e `elevation`
/// é quanto este item eleva os itens desenhados acima dele na pilha.
#[derive(Copy, Clone)]
pub struct ItemVisual {
    pub anim_id: u32,
    pub draw_offset: [f32; 2],
    pub elevation: f32,
}

impl Default for ItemVisual {
    fn default() -> Self {
        Self { anim_id: 0, draw_offset: [0.0, 0.0], elevation: 0.0 }
    }
}

pub struct SpriteResolver {
    table: ItemTypeTable,
    catalog: Catalog,
    assets_dir: PathBuf,
    sheet_cache: AHashMap<u32, Arc<DecodedSheet>>,
    sprite_layer: AHashMap<u32, u32>,
    anim_cache: AHashMap<u16, u32>,
    resolved: u64,
    cache_hits: u64,
    decode_failures: u64,
    atlas_full: u32,
}

impl SpriteResolver {
    pub fn load<P: AsRef<Path>>(assets_dir: P) -> Result<Self, String> {
        let assets_dir = assets_dir.as_ref().to_path_buf();

        let catalog_path = assets_dir.join("catalog-content.json");
        let catalog_str = std::fs::read_to_string(&catalog_path)
            .map_err(|e| format!("falha lendo catalog-content.json: {e}"))?;
        let catalog = parse_catalog(&catalog_str)?;

        let appearance_file = catalog.appearance_file.clone();
        let appearances_path = assets_dir.join(&appearance_file);
        let appearances_data = std::fs::read(&appearances_path)
            .map_err(|e| format!("falha lendo {appearance_file}: {e}"))?;
        let table = load_appearances(&appearances_data)
            .ok_or_else(|| format!("falha parseando {appearance_file}"))?;

        Ok(Self {
            table, catalog, assets_dir,
            sheet_cache: AHashMap::new(),
            sprite_layer: AHashMap::new(),
            anim_cache: AHashMap::new(),
            resolved: 0, cache_hits: 0, decode_failures: 0, atlas_full: 0,
        })
    }

    /// Resolve um `type_id` completo (anim_id + offset + elevação) numa
    /// única chamada — é o que `sync_for_floor` chama por item do tile.
    pub fn visual_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas,
        anim_table: &mut AnimTable,
        type_id: u16,
    ) -> ItemVisual {
        if type_id == 0 {
            return ItemVisual::default();
        }

        let anim_id = self.anim_id_for(device, queue, atlas, anim_table, type_id);

        let Some(item_type) = self.table.get_opt(type_id) else {
            return ItemVisual { anim_id, ..Default::default() };
        };

        let (ox, oy) = item_type.draw_offset();
        let draw_offset = [ox as f32, oy as f32];

        let elevation = if item_type.has_elevation {
            item_type.draw_height() as f32
        } else {
            0.0
        };

        ItemVisual { anim_id, draw_offset, elevation }
    }

    fn anim_id_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas,
        anim_table: &mut AnimTable,
        type_id: u16,
    ) -> u32 {
        if let Some(&id) = self.anim_cache.get(&type_id) {
            return id;
        }
        let Some(item_type) = self.table.get_opt(type_id) else { return 0 };

        let id = if item_type.animation_phases.len() > 1 {
            let mut frame_layers = Vec::with_capacity(item_type.animation_phases.len());
            for phase in 0..item_type.animation_phases.len() as u32 {
                let idx = item_type.sprite_index(phase, 0, 0, 0, 0);
                let sprite_id = item_type.sprite_ids.get(idx).copied().unwrap_or(0);
                frame_layers.push(self.resolve_sprite_layer(device, queue, atlas, sprite_id));
            }
            let (min, max) = item_type.animation_phases[0];
            let duration_ms = ((min + max) / 2).max(1);
            anim_table.push_animated(device, queue, &frame_layers, duration_ms, item_type.async_animation)
        } else {
            let sprite_id = item_type.sprite_ids.first().copied().unwrap_or(0);
            let layer = self.resolve_sprite_layer(device, queue, atlas, sprite_id);
            anim_table.push_static(device, queue, layer)
        };

        self.anim_cache.insert(type_id, id);
        id
    }

    fn resolve_sprite_layer(
        &mut self, device: &wgpu::Device, queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas, sprite_id: u32,
    ) -> u32 {
        if sprite_id == 0 { return 0; }
        if let Some(&layer) = self.sprite_layer.get(&sprite_id) {
            self.cache_hits += 1;
            return layer;
        }
        let Some(cell) = self.decode_sprite_cell(sprite_id) else {
            self.decode_failures += 1;
            return 0;
        };
        let layer = atlas.append(device, queue, &cell.rgba);
        if layer == 0 {
            self.atlas_full += 1;
            eprintln!("sprite resolver: atlas cheio (max_layers={}) sprite_id={sprite_id}", atlas.max_layers());
            return 0;
        }
        self.sprite_layer.insert(sprite_id, layer);
        self.resolved += 1;
        eprintln!(
            "sprite resolver: sprite_id={sprite_id} sheet={} layout={:?} cell=({},{}) layer={layer}",
            cell.sheet_file, cell.layout, cell.cell_x, cell.cell_y,
        );
        layer
    }

    fn decode_sprite_cell(&mut self, sprite_id: u32) -> Option<DecodedSpriteCell> {
        let sheet_info = self.catalog.sheet_for_sprite(sprite_id)?.clone();
        let sheet = self.get_or_decode_sheet(&sheet_info)?;

        let (sw, sh) = sheet_info.sprite_type.sprite_size();
        let cols = SHEET_SIZE / sw;
        let offset = sprite_id - sheet_info.first_id;
        let col = offset % cols;
        let row = offset / cols;

        let x0 = (col * sw) as usize;
        let y0 = (row * sh) as usize;

        let (sx, sy) = match sheet_info.sprite_type {
            editor_formats::catalog::SpriteLayout::TwoByOne => (x0 + 32, y0),
            editor_formats::catalog::SpriteLayout::OneByTwo => (x0, y0 + 32),
            _ => (x0, y0),
        };

        let mut cell = [0u8; 32 * 32 * 4];
        let sheet_w = SHEET_SIZE as usize;
        for dy in 0..32 {
            let src_row = (sy + dy) * sheet_w * 4 + sx * 4;
            let dst_row = dy * 32 * 4;
            cell[dst_row..dst_row + 128].copy_from_slice(&sheet.pixels_rgba[src_row..src_row + 128]);
        }

        Some(DecodedSpriteCell {
            rgba: cell, sheet_file: sheet_info.file, layout: sheet_info.sprite_type,
            cell_x: sx, cell_y: sy,
        })
    }

    fn get_or_decode_sheet(&mut self, info: &SpriteSheetInfo) -> Option<Arc<DecodedSheet>> {
        if let Some(s) = self.sheet_cache.get(&info.first_id) {
            return Some(Arc::clone(s));
        }
        let path = self.assets_dir.join(&info.file);
        let data = std::fs::read(&path).ok()?;
        let bgra = decode_sheet(&data).ok()?;
        let mut rgba = vec![0u8; bgra.len()];
        for chunk in bgra.chunks_exact(4) {
            let i = chunk.as_ptr() as usize - bgra.as_ptr() as usize;
            let idx = i / 4 * 4;
            rgba[idx] = chunk[2];
            rgba[idx + 1] = chunk[1];
            rgba[idx + 2] = chunk[0];
            rgba[idx + 3] = chunk[3];
        }
        let sheet = Arc::new(DecodedSheet { pixels_rgba: rgba, layout: info.sprite_type, first_id: info.first_id });
        self.sheet_cache.insert(info.first_id, Arc::clone(&sheet));
        Some(sheet)
    }
}

pub use editor_formats::catalog::SpriteLayout;