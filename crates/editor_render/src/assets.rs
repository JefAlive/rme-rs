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
    /// Cache por `type_id` do bloco contíguo de fases de itens animados
    /// (todas as fases × padrões × layers, em ordem de `sprite_index`).
    anim_block: AHashMap<u16, AnimBlockInfo>,
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
    /// O item emissor tem sprite animado (fases) → a luz dele flickera.
    pub is_animated: bool,
    /// Número de fases da animação; 0 = estático (layer_index fixo).
    pub anim_frames: u32,
    /// Avanço em células do atlas entre fases consecutivas (sprites por frame
    /// × blocos 32×32 do sprite). As fases são pré-decodificadas contíguas.
    pub anim_step: u32,
    /// Duração efetiva por frame em ms (média/fixo; 0 → 500 padrão).
    pub anim_dur_ms: u32,
    /// Item async → seed por tile dessincroniza a animação (estilo RME);
    /// sincronizado usa fase global de todos os exemplares.
    pub anim_async: bool,
}

#[derive(Copy, Clone)]
struct CachedSprite {
    base_layer: u32,
    width: u8,
    height: u8,
}

/// Bloco contíguo de um tipo animado no atlas (todas as fases, em ordem de
/// `sprite_index`: frame é a dimensão mais externa).
#[derive(Copy, Clone)]
struct AnimBlockInfo {
    /// 0 = falhou (sprite ausente/atlas cheio) → renderiza frame 0 estático.
    frames: u32,
    /// Célula do sprite de `sprite_index == 0`.
    strip_base: u32,
    /// Sprites por frame = pattern_stride (fz*fy*fx*layers).
    sprites_per_frame: u32,
    /// Blocos 32×32 que cada sprite ocupa (width*height).
    cells_per_sprite: u32,
    width: u8,
    height: u8,
    dur_ms: u32,
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
            anim_block: AHashMap::new(),
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

        // Copia os dados para locais (enum) antes de qualquer borrow mutável
        // de `self` abaixo; `item_type` vive só até aqui.
        let has_light = item_type.has_light();
        let light_color = item_type.sprite.light_color;
        let light_intensity = item_type.sprite.light_intensity;
        let is_animated = item_type.is_animated();
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
            is_animated,
            anim_frames: 0,
            anim_step: 0,
            anim_dur_ms: 0,
            anim_async: item_type.async_animation,
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

        // Itens animados: pré-decodifica o bloco contíguo de TODAS as fases em
        // ordem de `sprite_index` (frame é a dimensão mais externa), garantindo
        // no atlas a contiguidade que o shader usa com `frame * step_cells`.
        if is_animated {
            if self.anim_block.get(&type_id).is_none() {
                let ids = item_type.sprite_ids.clone();
                let phases = item_type.animation_phases.clone();
                let stride = pattern_stride(item_type);
                let info = self.build_anim_block(device, queue, atlas, ids, phases, stride);
                self.anim_block.insert(type_id, info);
            }
            if let Some(block) = self.anim_block.get(&type_id) && block.frames > 0 {
                let step = block.sprites_per_frame * block.cells_per_sprite;
                if step <= 0xffff {
                    return ItemVisual {
                        layer_index: block.strip_base + (sprite_index as u32) * block.cells_per_sprite,
                        width: block.width,
                        height: block.height,
                        anim_frames: block.frames,
                        anim_step: step,
                        anim_dur_ms: block.dur_ms,
                        ..visual
                    };
                }
                if self.diagnosed_missing_types.insert(type_id) {
                    eprintln!("sprite resolver: animação de type_id={type_id} fora do step_cells 16 bits ({step}) — estático");
                }
            }
        }

        // Caminho estático (também usado como fallback de bloco falho):
        // resolveso sprite do frame 0.
        let Some(sprite) = self.resolve_sprite_layer(device, queue, atlas, sprite_id) else {
            return visual;
        };
        ItemVisual { layer_index: sprite.base_layer, width: sprite.width, height: sprite.height, has_light, light_color, light_intensity, ..visual }
    }

    /// Resolve um sprite único: cache hit, decode da sheet e upload contíguo
    /// dos blocos 32×32 (ordem de linha) para o atlas.
    fn resolve_sprite_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas,
        sprite_id: u32,
    ) -> Option<CachedSprite> {
        if let Some(&sprite) = self.sprite_layer.get(&sprite_id) {
            self.cache_hits += 1;
            return Some(sprite);
        }

        let Some(cell) = self.decode_sprite_cell(sprite_id) else {
            self.decode_failures += 1;
            if self.diagnosed_missing_sprites.insert(sprite_id) {
                eprintln!("sprite resolver: falha ao decodificar sprite_id={sprite_id}");
            }
            return None;
        };

        // Os blocos 32×32 são adicionados em ordem de linha para que o shader
        // calcule o slot de cada parte a partir do índice-base.
        let mut base_layer = 0;
        for (index, rgba) in cell.cells.iter().enumerate() {
            let layer = atlas.append(device, queue, rgba);
            if layer == 0 {
                self.atlas_full += 1;
                eprintln!("sprite resolver: atlas cheio (max_slots={}) sprite_id={sprite_id}", atlas.max_layers());
                return None;
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
            "sprite resolver: sprite_id={sprite_id} sheet={} layout={:?} cell=({},{}) size={}x{} base_layer={base_layer}",
            cell.sheet_file, cell.layout, cell.cell_x, cell.cell_y, cell.width, cell.height,
        );
        Some(cached)
    }

    /// Pré-decodifica o bloco completo de `sprite_ids` de um tipo animado em
    /// ordem, um sprite após o outro (as fases ficam contíguas no atlas).
    fn build_anim_block(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut SpriteAtlas,
        sprite_ids: Vec<u32>,
        phases: Vec<(u32, u32)>,
        pattern_stride: u32,
    ) -> AnimBlockInfo {
        if sprite_ids.is_empty() {
            return AnimBlockInfo { frames: 0, strip_base: 0, sprites_per_frame: 0, cells_per_sprite: 0, width: 0, height: 0, dur_ms: 0 };
        }
        let mut base = 0u32;
        let mut cells_per_sprite = 0u32;
        let mut sprite_width = 0u8;
        let mut sprite_height = 0u8;
        // Contiguidade: cada sprite deve cair exatamente na célula seguinte à
        // do anterior. Se algum sprite já foi resolvido num bloco de outro tipo
        // (sprites compartilhados entre tipos), a tira quebra e a animação vira
        // estática em vez de exibir células erradas.
        let mut expected = 0u32;
        for (idx, &sid) in sprite_ids.iter().enumerate() {
            let Some(cached) = self.resolve_sprite_layer(device, queue, atlas, sid) else {
                return AnimBlockInfo { frames: 0, strip_base: 0, sprites_per_frame: 0, cells_per_sprite: 0, width: 0, height: 0, dur_ms: 0 };
            };
            if idx == 0 {
                base = cached.base_layer;
                cells_per_sprite = cached.width as u32 * cached.height as u32;
                expected = base + cells_per_sprite;
                sprite_width = cached.width;
                sprite_height = cached.height;
            } else {
                if cached.base_layer != expected {
                    return AnimBlockInfo { frames: 0, strip_base: 0, sprites_per_frame: 0, cells_per_sprite: 0, width: 0, height: 0, dur_ms: 0 };
                }
                expected += cached.width as u32 * cached.height as u32;
            }
        }
        AnimBlockInfo {
            frames: phases.len() as u32,
            strip_base: base,
            sprites_per_frame: pattern_stride.max(1),
            cells_per_sprite,
            width: sprite_width,
            height: sprite_height,
            dur_ms: resolve_dur_ms(&phases),
        }
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

/// Sprites por frame = strides de padrões × layers (`fz*fy*fx*layers`), a
/// distância entre fases consecutivas de um mesmo padrão no `sprite_index`.
fn pattern_stride(item_type: &editor_formats::appearances::ItemType) -> u32 {
    let fx = item_type.pattern_width.max(1);
    let fy = item_type.pattern_height.max(1);
    let fz = item_type.pattern_depth.max(1);
    let layers = item_type.layers.max(1);
    fz * fy * fx * layers
}

/// Duração efetiva por frame (ms) das fases de um tipo animado. Fiel ao
/// OTClient/RME quando as fases têm a mesma duração (caso comum): devolve o
/// valor delas. Fases (0,0) caem para a primeira duração não-zero (fix do
/// OTClient`Animator::unserializeAppearance`). Com durações variadas, a média
/// aproxima deterministicamente a caminhada ponderada por duração do RME.
fn resolve_dur_ms(phases: &[(u32, u32)]) -> u32 {
    if phases.is_empty() {
        return 0;
    }
    let fallback = phases
        .iter()
        .find(|(mn, mx)| *mn > 0 || *mx > 0)
        .map(|(mn, mx)| (((*mn as u64 + *mx as u64) / 2).max(1)) as u32)
        .unwrap_or(1);
    let mut sum = 0u64;
    for (mn, mx) in phases {
        let dur = if *mn == 0 && *mx == 0 {
            fallback
        } else {
            (((*mn as u64 + *mx as u64) / 2).max(1)) as u32
        };
        sum += dur as u64;
    }
    ((sum / phases.len() as u64).max(1)) as u32
}
