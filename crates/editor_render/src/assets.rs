use std::path::{Path, PathBuf};
use std::sync::Arc;

use ahash::AHashMap;
use editor_formats::appearances::{ItemTypeTable, load_appearances};
use editor_formats::catalog::{Catalog, parse_catalog, SpriteSheetInfo};
use editor_formats::sprite::{decode_sheet, SHEET_SIZE};

use crate::atlas::SpriteAtlas;

/// Sheet decodificada (RGBA, já flipada verticalmente) + metadados.
struct DecodedSheet {
    pixels_rgba: Vec<u8>, // SHEET_SIZE * SHEET_SIZE * 4
}

/// Célula decodificada de um sprite junto com metadados para logs.
struct DecodedSpriteCell {
    rgba: [u8; 32 * 32 * 4],
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
    /// Cache `sprite_id → layer_index` no atlas.
    sprite_layer: AHashMap<u32, u32>,
    resolved: u64,
    cache_hits: u64,
    decode_failures: u64,
    atlas_full: u32,
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
        })
    }

    /// Resolve `type_id` para o índice da layer no atlas, decodificando a célula
    /// sob demanda e fazendo upload via `atlas.append`.
    pub fn layer_for(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas,
        type_id: u16,
    ) -> u32 {
        if type_id == 0 {
            return 0; // layer 0 = transparente
        }

        let Some(item_type) = self.table.get_opt(type_id) else {
            return 0;
        };

        let Some(&sprite_id) = item_type.sprite_ids.first() else {
            return 0;
        };

        if sprite_id == 0 {
            return 0;
        }

        // Cache hit?
        if let Some(&layer) = self.sprite_layer.get(&sprite_id) {
            self.cache_hits += 1;
            return layer;
        }

        // Decodificar sheet → célula RGBA 32×32
        let Some(cell) = self.decode_sprite_cell(sprite_id) else {
            self.decode_failures += 1;
            return 0;
        };

        // Upload no atlas
        let layer = atlas.append(device, queue, &cell.rgba);
        if layer == 0 {
            self.atlas_full += 1;
            eprintln!("sprite resolver: atlas cheio (max_layers={}) sprite_id={sprite_id}", atlas.max_layers());
            return 0;
        }

        self.sprite_layer.insert(sprite_id, layer);
        self.resolved += 1;
        eprintln!(
            "sprite resolver: type_id={type_id} sprite_id={sprite_id} sheet={} layout={:?} cell=({},{}) layer={layer}",
            cell.sheet_file, cell.layout, cell.cell_x, cell.cell_y,
        );
        layer
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

        // Coordenadas do canto superior-esquerdo da célula na sheet decodificada
        let x0 = (col * sw) as usize;
        let y0 = (row * sh) as usize;

        // Região 32×32 a extrair (conforme SpriteLayout do RME):
        // - 1x1 / 2x2: canto sup.-esq. (32×32)
        // - 2x1: metade direita (x0+32, y0)
        // - 1x2: metade inferior (x0, y0+32)
        let (sx, sy) = match sheet_info.sprite_type {
            editor_formats::catalog::SpriteLayout::TwoByOne => (x0 + 32, y0),
            editor_formats::catalog::SpriteLayout::OneByTwo => (x0, y0 + 32),
            _ => (x0, y0),
        };

        // Extrair 32×32 RGBA da sheet (BGRA→RGBA já feito no decode)
        let mut cell = [0u8; 32 * 32 * 4];
        let sheet_w = SHEET_SIZE as usize;
        for dy in 0..32 {
            let src_row = (sy + dy) * sheet_w * 4 + sx * 4;
            let dst_row = dy * 32 * 4;
            cell[dst_row..dst_row + 128].copy_from_slice(&sheet.pixels_rgba[src_row..src_row + 128]);
        }

        Some(DecodedSpriteCell {
            rgba: cell,
            sheet_file: sheet_info.file,
            layout: sheet_info.sprite_type,
            cell_x: sx,
            cell_y: sy,
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