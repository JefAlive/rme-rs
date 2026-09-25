use std::path::{Path, PathBuf};
use std::sync::Arc;

use ahash::{AHashMap, AHashSet};
use editor_formats::appearances::{ItemTypeTable, load_appearances};
use editor_formats::catalog::{Catalog, parse_catalog, SpriteSheetInfo};
use editor_formats::sprite::{decode_sheet, SHEET_SIZE};
use editor_core::position::Position;

use crate::atlas::SpriteAtlas;

/// Sheet decodificada (RGBA, já flipada verticalmente) + metadados.
struct DecodedSheet {
    pixels_rgba: Vec<u8>, // SHEET_SIZE * SHEET_SIZE * 4
}

/// Célula decodificada de um sprite junto com metadados para logs.
struct DecodedSpriteCell {
    cells: Vec<[u8; 32 * 32 * 4]>,
    width: u8,
    height: u8,
    sheet_file: String,
    layout: editor_formats::catalog::SpriteLayout,
    cell_x: usize,
    cell_y: usize,
}

/// Resolvedor `type_id → sprite_id → célula RGBA → layer do atlas`.
pub struct SpriteResolver {
    table: ItemTypeTable,
    catalog: Catalog,
    assets_dir: PathBuf,
    /// Cache de sheets decodificadas por `first_id` do SpriteSheetInfo.
    sheet_cache: AHashMap<u32, Arc<DecodedSheet>>,
    /// Cache `sprite_id → blocos 32×32 contíguos` no atlas.
    sprite_layer: AHashMap<u32, CachedSprite>,
    resolved: u64,
    cache_hits: u64,
    decode_failures: u64,
    atlas_full: u32,
    diagnosed_missing_types: AHashSet<u16>,
    diagnosed_missing_sprites: AHashSet<u32>,
}

#[derive(Copy, Clone, Default)]
pub struct ItemVisual {
    pub layer_index: u32,
    pub width: u8,
    pub height: u8,
    pub draw_offset: [f32; 2],
    pub elevation: f32,
    pub has_light: bool,
    pub light_color: u32,
    pub light_intensity: u32,
}

#[derive(Copy, Clone)]
struct CachedSprite {
    base_layer: u32,
    width: u8,
    height: u8,
}

impl SpriteResolver {
    /// Carrega `appearances.dat` e `catalog-content.json` do diretório de assets.
    pub fn load<P: AsRef<Path>>(assets_dir: P) -> Result<Self, String> {
        let assets_dir = assets_dir.as_ref().to_path_buf();

        // appearances.dat
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
            table,
            catalog,
            assets_dir,
            sheet_cache: AHashMap::new(),
            sprite_layer: AHashMap::new(),
            resolved: 0,
            cache_hits: 0,
            decode_failures: 0,
            atlas_full: 0,
            diagnosed_missing_types: AHashSet::new(),
            diagnosed_missing_sprites: AHashSet::new(),
        })
    }

    /// Resolve `type_id` para o índice da layer no atlas, decodificando a célula
    /// sob demanda e fazendo upload via `atlas.append`.
    pub fn visual_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas,
        type_id: u16,
        position: Position,
    ) -> ItemVisual {
        if type_id == 0 {
            return ItemVisual::default();
        }

        let Some(item_type) = self.table.get_opt(type_id) else {
            if self.diagnosed_missing_types.insert(type_id) {
                eprintln!("sprite resolver: type_id={type_id} não existe em appearances.dat");
            }
            return ItemVisual::default();
        };

        // Copia os dados da luz para locais (enum) antes de qualquer borrow
        // mutável de `self` abaixo; `item_type` vive só até aqui.
        let has_light = item_type.has_light();
        let light_color = item_type.sprite.light_color;
        let light_intensity = item_type.sprite.light_intensity;

        let (offset_x, offset_y) = item_type.draw_offset();
        let visual = ItemVisual {
            layer_index: 0,
            width: 1,
            height: 1,
            draw_offset: [offset_x as f32, offset_y as f32],
            elevation: if item_type.has_elevation { item_type.draw_height() as f32 } else { 0.0 },
            has_light,
            light_color,
            light_intensity,
        };

        // Em itens com padrões (paredes, portas, bordas etc.), o RME escolhe
        // o sprite conforme a posição do tile. Usar sempre sprite_ids[0]
        // fazia várias dessas aparências apontarem para a célula errada.
        let sprite_index = item_type.sprite_index(
            0,
            position.x as u32,
            position.y as u32,
            position.z as u32,
            0,
        );
        let Some(&sprite_id) = item_type
            .sprite_ids
            .get(sprite_index)
            .or_else(|| item_type.sprite_ids.first())
        else {
            if self.diagnosed_missing_types.insert(type_id) {
                eprintln!("sprite resolver: type_id={type_id} sem sprite_ids em appearances.dat");
            }
            return visual;
        };

        if sprite_id == 0 {
            if self.diagnosed_missing_types.insert(type_id) {
                eprintln!("sprite resolver: type_id={type_id} resolve para sprite_id=0");
            }
            return visual;
        }

        // Cache hit?
        if let Some(&sprite) = self.sprite_layer.get(&sprite_id) {
            self.cache_hits += 1;
            return ItemVisual {
                layer_index: sprite.base_layer,
                width: sprite.width,
                height: sprite.height,
                has_light,
                light_color,
                light_intensity,
                ..visual
            };
        }

        // Decodificar sheet → célula RGBA 32×32
        let Some(cell) = self.decode_sprite_cell(sprite_id) else {
            self.decode_failures += 1;
            if self.diagnosed_missing_sprites.insert(sprite_id) {
                eprintln!("sprite resolver: falha ao decodificar sprite_id={sprite_id} para type_id={type_id}");
            }
            return visual;
        };

        // Os blocos 32×32 são adicionados em ordem de linha para que o shader
        // calcule o slot de cada parte a partir do índice-base.
        let mut base_layer = 0;
        for (index, rgba) in cell.cells.iter().enumerate() {
            let layer = atlas.append(device, queue, rgba);
            if layer == 0 {
                self.atlas_full += 1;
                eprintln!("sprite resolver: atlas cheio (max_slots={}) sprite_id={sprite_id}", atlas.max_layers());
                return visual;
            }
            if index == 0 { base_layer = layer; }
        }

        let cached = CachedSprite {
            base_layer,
            width: cell.width,
            height: cell.height,
        };
        self.sprite_layer.insert(sprite_id, cached);
        self.resolved += 1;
        eprintln!(
            "sprite resolver: type_id={type_id} sprite_id={sprite_id} sheet={} layout={:?} cell=({},{}) size={}x{} base_layer={base_layer}",
            cell.sheet_file, cell.layout, cell.cell_x, cell.cell_y, cell.width, cell.height,
        );
        ItemVisual { layer_index: base_layer, width: cell.width, height: cell.height, has_light, light_color, light_intensity, ..visual }
    }

    fn decode_sprite_cell(&mut self, sprite_id: u32) -> Option<DecodedSpriteCell> {
        // Localizar sheet (clone para evitar borrow duplo)
        let sheet_info = self.catalog.sheet_for_sprite(sprite_id)?.clone();

        // Obter (ou decodificar) sheet
        let sheet = self.get_or_decode_sheet(&sheet_info)?;

        // Calcular célula na sheet
        let (sw, sh) = sheet_info.sprite_type.sprite_size();
        let cols = SHEET_SIZE / sw;
        let offset = sprite_id - sheet_info.first_id;
        let col = offset % cols;
        let row = offset / cols;

        // Coordenadas do canto superior-esquerdo da célula na sheet decodificada.
        let x0 = (col * sw) as usize;
        let y0 = (row * sh) as usize;
        let width = (sw / 32) as u8;
        let height = (sh / 32) as u8;
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        let sheet_w = SHEET_SIZE as usize;
        for part_y in 0..height as usize {
            for part_x in 0..width as usize {
                let sx = x0 + part_x * 32;
                let sy = y0 + part_y * 32;
                let mut cell = [0u8; 32 * 32 * 4];
                for dy in 0..32 {
                    let src_row = (sy + dy) * sheet_w * 4 + sx * 4;
                    let dst_row = dy * 32 * 4;
                    cell[dst_row..dst_row + 128]
                        .copy_from_slice(&sheet.pixels_rgba[src_row..src_row + 128]);
                }
                cells.push(cell);
            }
        }

        Some(DecodedSpriteCell {
            cells,
            width,
            height,
            sheet_file: sheet_info.file,
            layout: sheet_info.sprite_type,
            cell_x: x0,
            cell_y: y0,
        })
    }

    fn get_or_decode_sheet(&mut self, info: &SpriteSheetInfo) -> Option<Arc<DecodedSheet>> {
        if let Some(s) = self.sheet_cache.get(&info.first_id) {
            return Some(Arc::clone(s));
        }

        let path = self.assets_dir.join(&info.file);
        let data = std::fs::read(&path).ok()?;

        // decode_sheet devolve BGRA flipada; converter para RGBA
        let bgra = decode_sheet(&data).ok()?;
        let mut rgba = vec![0u8; bgra.len()];
        for chunk in bgra.chunks_exact(4) {
            let i = chunk.as_ptr() as usize - bgra.as_ptr() as usize;
            let idx = i / 4 * 4;
            rgba[idx] = chunk[2];     // R = B
            rgba[idx + 1] = chunk[1]; // G = G
            rgba[idx + 2] = chunk[0]; // B = R
            rgba[idx + 3] = chunk[3]; // A = A
        }

        let sheet = Arc::new(DecodedSheet {
            pixels_rgba: rgba,
        });
        self.sheet_cache.insert(info.first_id, Arc::clone(&sheet));
        Some(sheet)
    }
}

// Re-export para conveniência no tabs.rs
pub use editor_formats::catalog::SpriteLayout;
