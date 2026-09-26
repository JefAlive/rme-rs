use editor_core::{MapDocument, position::Position, spatial_map::SpatialMap};
use egui::{RichText, Ui};

const PAN_SPEED_TILES_PER_SEC: f32 = 12.0;
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 8.0;

/// Piso do ambiente (World Light) em percentual. OTClient não tem piso no
/// "Ambient Light" (default 0) — com luz global 0 fica preto puro; aqui
/// mantemos 8% só para o editor não virar abismo no slider 0.
const MIN_AMBIENT_PCT: u8 = 8;

/// Força fixa do lens mist (sem slider): névoa de lente à noite.
const LENS_MIST_STRENGTH: f32 = 0.250;

/// Força fixa do halation de fósforo do CRT Bloom (sem slider), de dia.
const CRT_BLOOM_STRENGTH: f32 = 0.200;

/// Flicker de luzes de itens ANIMADOS, contínuo por cor/tamanho:
/// - SEM animação → luz estática (o `is_animated` vem no TileLight).
/// - MENOR → caos moderado, rápido, raio sutil; MAIOR → ainda mais sutil e
///   lento (nunca zera: sempre há um piso de amplitude).
/// - QUENTE (vermelho dominante): caos normal + raio normal.
/// - FRIO: menos caótico (só bandas lenta/média, sem pop) e raio ×0.5.
const FIRE_SLOW_AMP: f32 = 0.015;
const FIRE_MID_AMP: f32 = 0.035;
const FIRE_FAST_AMP: f32 = 0.032;
const FIRE_POP_AMP: f32 = 0.030;
const FIRE_RADIUS: f32 = 0.012;
const FIRE_COLD_FADE: f32 = 0.55;
const FIRE_COLD_RADIUS: f32 = 0.5;
/// Piso de amplitude para luzes grandes (sutileza, nunca zero).
const FIRE_LARGE_MIN: f32 = 0.25;
/// Piso do raio para luzes grandes.
const FIRE_LARGE_RADIUS: f32 = 0.15;

/// Smoothstep 0→1 entre `e0` e `e1`, clamado.
fn smoothstep01(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Aplica o flicker contínuo a emissoras de item ANIMADO (luz estática não
/// mexe; chão nunca flickera — gate fora daqui). `seed` (0..1) é a fase do
/// COMPONENTE: luzes adjacentes do mesmo tipo compartilham seed → movem juntas
/// (coesas); `damp` (0..1) amortece por tamanho do componente (isolada = 1).
/// Tamanho é uma régua: pequena = caótica/rápida, grande = sutil/lenta. Cor
/// define o caráter: quente caótica e fria calma. Nunca apaga a luz.
fn flicker_light(mut light: editor_render::scene::TileLight, t: f32, seed: f32, damp: f32) -> editor_render::scene::TileLight {
    // Item emissor não-animado → luz estática (sem flicker).
    if light.is_animated == 0 {
        return light;
    }
    let r = light.color[0];
    let g = light.color[1];
    let b = light.color[2];
    let warm = r > g && r >= b;
    let size = light.intensity;

    // "Domesticação" contínua pelo tamanho: pequena/média (≤5) → vivid 1 (caos
    // cheio); grande (≥10) → vivid 0 (sutil, lento). Nunca desliga.
    let tame = smoothstep01(5.0, 10.0, size);
    let vivid = 1.0 - tame;

    // Cor: frias metade do caos e metade do raio.
    let chaos = if warm { 1.0 } else { FIRE_COLD_FADE };
    let rad_f = if warm { 1.0 } else { FIRE_COLD_RADIUS };

    // Amplitudes (vivid + damp)
    let amp = (FIRE_LARGE_MIN + (1.0 - FIRE_LARGE_MIN) * vivid) * chaos * damp;
    let rad_amp = FIRE_RADIUS * (FIRE_LARGE_RADIUS + (1.0 - FIRE_LARGE_RADIUS) * vivid) * rad_f * damp;

    // Componente grande = bem mais suave: `calm` (0 = isolada, →1 = área
    // cheia) reduz velocidade e, principalmente, as bandas rápidas/pop que
    // causam o "caos". A área toda respira junto, lenta e tranquila.
    let calm = 1.0 - damp;
    let speed = (1.4 - 0.85 * tame) * (1.0 - 0.5 * calm);

    let tau = std::f32::consts::TAU;
    let phi = 1.618_034;
    let st = t * speed;

    // Frequências base w e potências de φ (razões irracionais → batimentos
    // que nunca repetem; "chocam" em vez de marcar o ritmo).
    let w = 1.9;
    let s1 = f32::sin(st * w * phi + seed * tau);                           // ~0.49 Hz × speed
    let s2 = f32::sin(st * w + seed * tau * 1.43);                          // ~0.30 Hz (razão ~φ)
    let slow = 0.5 * s1 + 0.5 * s2;                                         // batimento irregular
    let s3 = f32::sin(st * w * phi * phi + seed * tau * 2.37 + 1.4 * s1);       // ~1.28 Hz × speed
    let s4 = f32::sin(st * w * phi * phi * phi + seed * tau * 3.13 + 2.1 * s3); // ~3.28 Hz × speed

    // Hard edges ocasionais (só quentes): valor quantizado por célula, muda de
    // salto e é diferente em cada chama/componente.
    let cell = (st * 4.5).floor();
    let r0 = (f32::sin(cell * 1.7 + seed * 91.7) * 43758.545).fract();
    let pop = r0 - 0.5;

    let d = if warm {
        amp * (FIRE_SLOW_AMP * slow
            + FIRE_MID_AMP * (1.0 - 0.25 * calm) * s3
            + FIRE_FAST_AMP * (1.0 - 0.8 * calm) * s4)
            + FIRE_POP_AMP * amp * (1.0 - 0.7 * calm) * pop
    } else {
        // Frio: só bandas lenta/média (sem fast, sem pop) → menos caótico.
        amp * (FIRE_SLOW_AMP * slow + FIRE_MID_AMP * 0.8 * (1.0 - 0.25 * calm) * s3)
    };
    let rf = 1.0 + rad_amp * (0.6 * s3 + 0.4 * s4);

    let mult = (1.0 + d).max(0.5);
    for c in light.color.iter_mut() {
        *c = (*c * mult).min(1.0);
    }
    light.intensity = (light.intensity * rf.max(0.5)).max(0.5);
    light
}

/// Hash determinístico 0..1 a partir de duas coordenadas (tiles fracionários)
/// e um eixo — fase comum de um componente de luzes.
fn hash_unit(x: f32, y: f32, z: f32) -> f32 {
    (f32::sin(x * 12.9898 + y * 78.233 + z * 37.719) * 43758.545).fract()
}

/// Union-find de componentes de luz (feito por frame; n = nº de luzes
/// visíveis, ordem de grandeza irrelevante num editor).
struct ComponentUnion {
    parent: Vec<usize>,
    size: Vec<usize>,
}
impl ComponentUnion {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect(), size: vec![1; n] }
    }
    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let mut ra = self.find(a);
        let mut rb = self.find(b);
        if ra == rb {
            return;
        }
        if self.size[ra] < self.size[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        self.size[ra] += self.size[rb];
    }
}

/// Fator de extensão do raio da luz — precisa bater com o `RADIUS_EXT` dos
/// shaders (`light_vertex.wgsl` / `light_fragment.wgsl`), onde o falloff zera
/// em `dist = intensity * RADIUS_EXT`.
const LIGHT_RADIUS_EXT: f32 = 1.75;

/// Para cada luz, `Some(seed, damp)` se ela é participante de flicker (item
/// animado); `None` (estática) para chão e inanimadas.
/// Participantes do MESMO andar com círculos de luz SOBREPOSTOS (distância
/// entre centros ≤ (r1+r2)×1.05) — de qualquer cor — são unidos num
/// componente que compartilha UM seed (movem juntos = coesos) e `damp` cai
/// com o tamanho do componente (área grande → calma; isolada → viva).
fn flicker_component_seed_damp(lights: &[(u8, editor_render::scene::TileLight)]) -> Vec<Option<(f32, f32)>> {
    struct Part { z: u8, x: f32, y: f32, r: f32 }
    let mut parts: Vec<Option<Part>> = Vec::with_capacity(lights.len());
    for (z, l) in lights {
        if l.is_ground != 0 || l.is_animated == 0 {
            parts.push(None);
            continue;
        }
        parts.push(Some(Part {
            z: *z,
            x: l.world_pos[0],
            y: l.world_pos[1],
            r: l.intensity * LIGHT_RADIUS_EXT,
        }));
    }
    let particip: Vec<(usize, &Part)> = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref().map(|p| (i, p)))
        .collect();

    let mut uf = ComponentUnion::new(parts.len());
    // Agrupa por andar (luzes de z diferentes nunca se conectam) e checa
    // sobreposição par a par: círculos se cruzam se a distância entre centros
    // ≤ (r1 + r2) * 0.9. Abaixo de 1.0 exige sobreposição DE VERDADE (quase-
    // toque e bordas afastadas ficam de fora) — unir menos evita que áreas
    // distantes colapsem num componente só.
    let mut by_z: std::collections::HashMap<u8, Vec<usize>> = std::collections::HashMap::new();
    for (i, _) in &particip {
        by_z.entry(parts[*i].as_ref().unwrap().z).or_default().push(*i);
    }
    let ov = 0.9_f32;
    for idxs in by_z.values() {
        for a in 0..idxs.len() {
            let p = parts[idxs[a]].as_ref().unwrap();
            for b in (a + 1)..idxs.len() {
                let q = parts[idxs[b]].as_ref().unwrap();
                let d2 = (p.x - q.x) * (p.x - q.x) + (p.y - q.y) * (p.y - q.y);
                let thresh = (p.r + q.r) * ov;
                if d2 <= thresh * thresh {
                    uf.union(idxs[a], idxs[b]);
                }
            }
        }
    }

    // Âncora (menor x, depois y) e tamanho de cada componente.
    let mut anchors: std::collections::HashMap<usize, (f32, f32, u8)> = std::collections::HashMap::new();
    let mut sizes: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, p) in &particip {
        let root = uf.find(*i);
        let cur = anchors.entry(root).or_insert((p.x, p.y, p.z));
        if p.x < cur.0 || (p.x == cur.0 && p.y < cur.1) {
            *cur = (p.x, p.y, p.z);
        }
        *sizes.entry(root).or_default() += 1;
    }

    let mut out: Vec<Option<(f32, f32)>> = vec![None; parts.len()];
    for (i, p) in &particip {
        let root = uf.find(*i);
        let (ax, ay, az) = anchors.get(&root).copied().unwrap_or((p.x, p.y, p.z));
        let sz = sizes.get(&root).copied().unwrap_or(1).max(1);
        let seed = hash_unit(ax, ay, az as f32);
        let damp = (1.0 / (1.0 + 0.12 * (sz - 1) as f32)).max(0.06);
        out[*i] = Some((seed, damp));
    }
    out
}

/// Bounding box dos tiles com chão no andar informado (para a câmera).
fn compute_map_bounds(map: &SpatialMap, floor: u8) -> Option<(u16, u16, u16, u16)> {
    let mut min_x = u16::MAX;
    let mut min_y = u16::MAX;
    let mut max_x = u16::MIN;
    let mut max_y = u16::MIN;
    let mut found = false;

    for (coord, _chunk) in map.iter_chunk_coords() {
        if coord.z != floor {
            continue;
        }
        for local_idx in 0..(editor_core::position::CHUNK_SIZE as usize * editor_core::position::CHUNK_SIZE as usize) {
            let lx = (local_idx % editor_core::position::CHUNK_SIZE as usize) as u16;
            let ly = (local_idx / editor_core::position::CHUNK_SIZE as usize) as u16;
            let pos = Position {
                x: coord.cx as u16 * editor_core::position::CHUNK_SIZE + lx,
                y: coord.cy as u16 * editor_core::position::CHUNK_SIZE + ly,
                z: coord.z,
            };
            if let Some(tile) = map.get_tile(pos) {
                if tile.ground.is_some() {
                    found = true;
                    min_x = min_x.min(pos.x);
                    min_y = min_y.min(pos.y);
                    max_x = max_x.max(pos.x);
                    max_y = max_y.max(pos.y);
                }
            }
        }
    }

    if found {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Espelha a regra de composição multi-andar do RME (SetupVars/DrawMap):
/// no térreo ou acima (floor <= 7), a pilha vai do térreo (7) até o telhado
/// mais alto (0); no subsolo (floor > 7), desenha até 2 andares abaixo do
/// atual. Retorna (start_z, end_z = floor, superend_z).
fn compute_floor_stack(current_floor: u8) -> (u8, u8, u8) {
    let ground = editor_core::position::GROUND_FLOOR;
    let max_z = editor_core::position::MAP_MAX_Z;
    let start_z = if current_floor < 8 {
        ground
    } else {
        (current_floor + 2).min(max_z)
    };
    let superend_z = if current_floor > ground { 8 } else { 0 };
    (start_z, current_floor, superend_z)
}

pub enum EditorTab {
    Viewport { doc_index: usize },
    Objects,
    Towns,
    Houses,
    Zones,
    Project,
    World,
    Inspector,
    Minimap,
    Console,
}

// ---------- Grupos de seleção independentes (cada um é seu próprio "radio group") ----------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionBrush { SingleSelect, Eraser, Border }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ZoneBrush { ProtectionZone, NoPvp, Pvp, NoLogout }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DoorBrush { Normal, Quest, Locked, Magic }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowBrush { Normal, Hatched }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BrushShape { Circle, Square }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AntiAliasing { Off, Retro, Blurry, RoundedEdges }

impl AntiAliasing {
    fn label(self) -> &'static str {
        match self {
            AntiAliasing::Off => "Off",
            AntiAliasing::Retro => "Retro (Sharp Bilinear)",
            AntiAliasing::Blurry => "Blurry (Super 2xSaI)",
            AntiAliasing::RoundedEdges => "Rounded Edges (4xBRZ)",
        }
    }

    /// Valor consumido diretamente pelo shader de sprites.
    fn shader_mode(self) -> u32 {
        match self {
            AntiAliasing::Off => 0,
            AntiAliasing::Retro => 1,
            AntiAliasing::Blurry => 2,
            AntiAliasing::RoundedEdges => 3,
        }
    }
}

/// Seção "Shaders" da aba World (abaixo de Anti-aliasing). Todos vêm ligados
/// por padrão:
/// - checkerboard dithering: MDAPT (Sp00kyFox) rodado na cena nativa (1x)
///   antes do upscaling, para fundir os padrões de transparência do Tibia;
/// - crt bloom: CRT Bloom, um único toggle que combina o lens mist (névoa
///   difusa de lente, ativa de noite — World Light < 50%) e o halation de
///   fósforo (ativo de dia — World Light > 50%), forças fixas;
/// - crt colors: gama de fósforo SMPTE-C/Rec.601 aplicada ao RGB final
///   (vermelho leve-dessaturado pro laranja, azul pro ciano, branco ok).
#[derive(Clone, Copy)]
pub struct ShaderOptions {
    pub checkerboard_dither: bool,
    pub crt_bloom: bool,
    pub crt_color: bool,
}

impl Default for ShaderOptions {
    fn default() -> Self {
        Self {
            checkerboard_dither: true,
            crt_bloom: true,
            crt_color: true,
        }
    }
}

/// Estado global compartilhado entre abas.
pub struct AppState {
    pub documents: Vec<MapDocument>,
    pub active_doc: usize,

    pub selection_brush: Option<SelectionBrush>,
    pub zone_brush: Option<ZoneBrush>,
    pub door_brush: Option<DoorBrush>,
    pub window_brush: Option<WindowBrush>,
    pub brush_shape: BrushShape,
    pub auto_border_active: bool,
    pub brush_thickness: i32,
    pub brush_size: i32,

    pub city_filter: String,
    pub item_name_filter: String,

    pub world_light: u8,
    pub show_tooltips: bool,
    pub show_npcs: bool,
    pub show_monsters: bool,
    pub show_zones: bool,
    pub antialiasing: AntiAliasing,
    pub shaders: ShaderOptions,

    pub current_zoom: u32,
    pub current_floor_display: u8,
    pub hover_info: HoverInfo,
    pub log_lines: Vec<String>,

    pub wgpu: Option<egui_wgpu::RenderState>,
    pub tile_resources: Option<editor_render::pipeline::TileRenderResources>,
    pub offscreen: Option<editor_render::offscreen::OffscreenTarget>,
    pub scene_target: Option<editor_render::offscreen::SceneTarget>,
    pub filter_target: Option<editor_render::offscreen::SceneTarget>,
    pub mdapt_targets: [Option<editor_render::offscreen::SceneTarget>; 4],
    pub post_target: Option<editor_render::offscreen::SceneTarget>,
    /// Alvo da cena iluminada (saída do apply_light; input da cena × light buffer)
    pub light_target: Option<editor_render::offscreen::SceneTarget>,
    pub scaler_resources: Option<editor_render::scaler::ScaleResources>,
    pub chunk_cache: editor_render::scene::ChunkGpuCache,
    pub camera_offset: egui::Vec2,
    pub camera_zoom: f32,
    pub atlas: Option<editor_render::atlas::SpriteAtlas>,
    pub sprite_resolver: Option<editor_render::assets::SpriteResolver>,
    pub camera_fit_pending: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            documents: Vec::new(),
            active_doc: 0,
            selection_brush: Some(SelectionBrush::SingleSelect),
            zone_brush: None,
            door_brush: None,
            window_brush: None,
            brush_shape: BrushShape::Circle,
            auto_border_active: true,
            brush_thickness: 1,
            brush_size: 1,
            city_filter: String::new(),
            item_name_filter: String::new(),
            world_light: 100,
            show_tooltips: true,
            show_npcs: true,
            show_monsters: true,
            show_zones: false,
            antialiasing: AntiAliasing::Retro,
            shaders: ShaderOptions::default(),
            current_zoom: 100,
            current_floor_display: editor_core::position::GROUND_FLOOR,
            hover_info: HoverInfo::default(),
            log_lines: Vec::new(),
            wgpu: None,
            tile_resources: None,
            offscreen: None,
            scene_target: None,
            filter_target: None,
            mdapt_targets: [None, None, None, None],
            post_target: None,
            light_target: None,
            scaler_resources: None,
            chunk_cache: Default::default(),
            camera_offset: egui::Vec2::ZERO,
            camera_zoom: 1.0,
            atlas: None,
            sprite_resolver: None,
            camera_fit_pending: true,
        }
    }
}

#[derive(Default, Clone)]
pub struct HoverInfo {
    pub x: i32, pub y: i32, pub z: i32,
    pub item_id: u32,
    pub item_name: String,
}

fn radio_button<T: PartialEq + Copy>(ui: &mut Ui, current: &mut Option<T>, value: T, label: &str) {
    let selected = *current == Some(value);
    if ui.selectable_label(selected, label).clicked() {
        *current = Some(value);
    }
}

fn radio_button_req<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, value: T, label: &str) {
    let selected = *current == value;
    if ui.selectable_label(selected, label).clicked() {
        *current = value;
    }
}

pub struct EditorTabViewer<'a> {
    pub state: &'a mut AppState,
}

impl<'a> egui_dock::TabViewer for EditorTabViewer<'a> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            EditorTab::Viewport { doc_index } => {
                self.state.documents.get(*doc_index)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| "Viewport".into())
                    .into()
            }
            EditorTab::Objects   => "Objects".into(),
            EditorTab::Towns     => "Towns".into(),
            EditorTab::Houses    => "Houses".into(),
            EditorTab::Zones     => "Zones".into(),
            EditorTab::Project   => "Project".into(),
            EditorTab::World     => "World".into(),
            EditorTab::Inspector => "Inspector".into(),
            EditorTab::Minimap   => "Minimap".into(),
            EditorTab::Console   => "Console".into(),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            EditorTab::Viewport { doc_index } => self.ui_viewport(ui, *doc_index),
            EditorTab::Objects   => self.ui_objects(ui),
            EditorTab::Towns     => self.ui_placeholder(ui, "Towns"),
            EditorTab::Houses    => self.ui_placeholder(ui, "Houses"),
            EditorTab::Zones     => self.ui_placeholder(ui, "Zones"),
            EditorTab::Project   => self.ui_placeholder(ui, "Project"),
            EditorTab::World     => self.ui_world(ui),
            EditorTab::Inspector => self.ui_inspector(ui),
            EditorTab::Minimap   => self.ui_minimap(ui),
            EditorTab::Console   => self.ui_console(ui),
        }
    }
}

impl<'a> EditorTabViewer<'a> {
    fn ui_viewport(&mut self, ui: &mut Ui, doc_index: usize) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(avail.x, avail.y - 26.0), egui::Sense::click_and_drag(),
        );

        if resp.dragged() {
            self.state.camera_offset -= resp.drag_delta() / self.state.camera_zoom;
        }

        // --- Navegação: zoom no scroll (ancorado no cursor), pan WASD, andar Q/E ---
        if resp.hovered() {
            let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
            if scroll_y != 0.0 {
                if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                    let local = hover_pos - rect.min;
                    let old_zoom = self.state.camera_zoom;
                    // Um pouco mais sensível abaixo de 200% para navegar o zoom
                    // com menos giros de scroll; acima disso mantém o passo.
                    let sensitivity = if old_zoom < 2.0 { 0.0018 } else { 0.001 };
                    let new_zoom = (old_zoom * (1.0 + scroll_y * sensitivity)).clamp(ZOOM_MIN, ZOOM_MAX);
                    if (new_zoom - old_zoom).abs() > f32::EPSILON {
                        // Mantém o ponto do mundo sob o cursor fixo ao zoomar.
                        self.state.camera_offset.x += local.x * (1.0 / old_zoom - 1.0 / new_zoom);
                        self.state.camera_offset.y += local.y * (1.0 / old_zoom - 1.0 / new_zoom);
                        self.state.camera_zoom = new_zoom;
                    }
                }
            }

            let dt = ui.input(|i| i.stable_dt);
            let mut dir = egui::Vec2::ZERO;
            ui.input(|i| {
                if i.key_down(egui::Key::W) { dir.y -= 1.0; }
                if i.key_down(egui::Key::S) { dir.y += 1.0; }
                if i.key_down(egui::Key::A) { dir.x -= 1.0; }
                if i.key_down(egui::Key::D) { dir.x += 1.0; }
            });
            if dir != egui::Vec2::ZERO {
                let world_px_per_sec = PAN_SPEED_TILES_PER_SEC * 32.0;
                self.state.camera_offset += dir.normalized() * world_px_per_sec * dt;
            }

            if ui.input(|i| i.key_pressed(egui::Key::Q)) {
                self.state.current_floor_display =
                    (self.state.current_floor_display + 1).min(editor_core::position::MAP_MAX_Z);
            }
            if ui.input(|i| i.key_pressed(egui::Key::E)) {
                self.state.current_floor_display = self.state.current_floor_display.saturating_sub(1);
            }

            // WASD precisa de repaint contínuo enquanto a tecla está segurada.
            ui.ctx().request_repaint();
        }

        if let Some(wgpu_state) = self.state.wgpu.clone() {
            let device = &wgpu_state.device;
            let queue = &wgpu_state.queue;

            if self.state.tile_resources.is_none() {
                if let Some(atlas) = &self.state.atlas {
                    self.state.tile_resources = Some(
                        editor_render::pipeline::TileRenderResources::new(
                            device, editor_render::offscreen::OFFSCREEN_FORMAT, &atlas.bind_group_layout,
                        )
                    );
                }
            }
            if self.state.scaler_resources.is_none() {
                self.state.scaler_resources = Some(editor_render::scaler::ScaleResources::new(device));
            }

            let width = rect.width().max(1.0) as u32;
            let height = rect.height().max(1.0) as u32;
            {
                let mut renderer = wgpu_state.renderer.write();
                match &mut self.state.offscreen {
                    Some(target) => target.resize_if_needed(device, &mut renderer, width, height),
                    None => self.state.offscreen = Some(
                        editor_render::offscreen::OffscreenTarget::create(device, &mut renderer, width, height)
                    ),
                }
            }
            // Alvo intermediário dos pós (CRT colour / CRT bloom), do mesmo
            // tamanho do output; o resultado volta ao output ao final.
            match &mut self.state.post_target {
                Some(target) => target.resize_if_needed(device, width, height),
                None => self.state.post_target = Some(editor_render::offscreen::SceneTarget::create(device, width, height)),
            }

            let floor = self.state.current_floor_display;
            let (start_z, end_z, superend_z) = compute_floor_stack(floor);

            // Extrai sprite_resolver/atlas do AppState por um instante: assim os
            // closures abaixo não capturam NADA de `self` — elimina qualquer
            // disputa de borrow com `doc` ou `chunk_cache`.
            let mut sprite_resolver = self.state.sprite_resolver.take();
            let mut atlas_for_resolve = self.state.atlas.take();
            {
                let doc = &mut self.state.documents[doc_index];
                let mut z = start_z;
                loop {
                    let resolver = |type_id: u16, position: editor_core::position::Position| -> editor_render::assets::ItemVisual {
                        let (Some(resolver), Some(atlas)) = (sprite_resolver.as_mut(), atlas_for_resolve.as_mut()) else {
                            return editor_render::assets::ItemVisual::default();
                        };
                        resolver.visual_for(device, queue, atlas, type_id, position)
                    };
                    self.state.chunk_cache.sync_for_floor(device, &mut doc.map, z, resolver);
                    if z == superend_z { break; }
                    z -= 1;
                }
            }
            self.state.sprite_resolver = sprite_resolver;
            self.state.atlas = atlas_for_resolve;

            // Camera fit: centraliza no bbox do mapa no primeiro frame.
            if self.state.camera_fit_pending {
                let doc = &self.state.documents[doc_index];
                if let Some((min_x, min_y, max_x, max_y)) = compute_map_bounds(&doc.map, floor) {
                    let map_w = (max_x as f32 - min_x as f32 + 1.0) * 32.0;
                    let map_h = (max_y as f32 - min_y as f32 + 1.0) * 32.0;
                    let center_x = (min_x as f32 + max_x as f32 + 1.0) * 16.0;
                    let center_y = (min_y as f32 + max_y as f32 + 1.0) * 16.0;
                    self.state.camera_offset = egui::Vec2::new(center_x, center_y);
                    let zoom_x = width as f32 / map_w;
                    let zoom_y = height as f32 / map_h;
                    self.state.camera_zoom = zoom_x.min(zoom_y) * 0.9;
                    eprintln!("camera fit: floor={floor} bbox=({min_x}..{max_x} x {min_y}..{max_y}) zoom={:.3}",
                        self.state.camera_zoom);
                    self.state.camera_fit_pending = false;
                }
            }

            // O RME desenha como mapas completos apenas os floors entre o
            // início da pilha e o floor selecionado. Floors abaixo dele
            // (z menor) não fazem parte da composição: incluir esses tiles
            // deixa, por exemplo, o floor 2 visível através de áreas vazias
            // do floor 5. O overlay translúcido de floor-1 é separado e não
            // entra aqui.
            // Alinha os andares segundo getDrawPosition do RME: z=7 é a
            // origem no térreo e acima; no subsolo, usa o floor selecionado.
            let mut layers: Vec<editor_render::scene::FloorLayer> = Vec::new();
            {
                let mut z = start_z as i32;
                loop {
                    let ground_z = editor_core::position::GROUND_FLOOR as i32;
                    let reference_z = if z <= ground_z { ground_z } else { end_z as i32 };
                    let screen_shift = (reference_z - z) as f32 * 32.0;
                    layers.push(editor_render::scene::FloorLayer {
                        z: z as u8,
                        alpha: 1.0,
                        pixel_offset: [screen_shift, screen_shift],
                    });
                    if z as u8 == end_z { break; }
                    z -= 1;
                }
            }
            layers.sort_by_key(|l| if l.z == end_z { 1 } else { 0 });

            // Tamanho do buffer de cena no estilo do RME de referência: em
            // zoom-in o Retro (Smooth) supersampleia a cena em densidade
            // inteira (ceil do zoom, sourceCellSize) para que as bordas dos
            // tiles fiquem alinhadas e o filtro atue sobre células inteiras.
            // Os filtros pixel-art (Super 2xSaI, xBRZ) processam a cena nativa
            // (1 texel por pixel do mapa) e o chain sobe em múltiplos inteiros
            // (2xSaI 2x, xBRZ 4x) para um alvo intermediário; o blit final
            // encaixa o resultado no tamanho da janela, mantendo o grid do
            // filtro inteiro e consistente durante o zoom fracionário. "Off"
            // mantém o buffer do tamanho do painel. Em zoom-out a cena fica
            // nativa (até 2x o painel) e o scaler só reduz. O filtro em si é
            // sempre a última passada, sobre a cena composta inteira.
            let mode = self.state.antialiasing.shader_mode();
            let zoom = self.state.camera_zoom;
            let (req_w, req_h, design_zoom) = if mode == 0 {
                (width.max(1) as f32, height.max(1) as f32, [zoom, zoom])
            } else if zoom > 1.0 {
                if mode >= 2 {
                    // Super 2xSaI / xBRZ: cena nativa; o filtro sobe a 2x/4x.
                    ((width.max(1) as f32 / zoom).ceil(),
                     (height.max(1) as f32 / zoom).ceil(),
                     [1.0, 1.0])
                } else {
                    let density = zoom.ceil();
                    ((width.max(1) as f32 / zoom).ceil() * density,
                     (height.max(1) as f32 / zoom).ceil() * density,
                     [density, density])
                }
            } else if zoom >= 0.5 {
                ((width.max(1) as f32 / zoom).ceil(),
                 (height.max(1) as f32 / zoom).ceil(),
                 [1.0, 1.0])
            } else {
                (width.max(1) as f32 * 2.0,
                 height.max(1) as f32 * 2.0,
                 [zoom * 2.0, zoom * 2.0])
            };
            let max_dimension = device.limits().max_texture_dimension_2d as f32;
            let max_pixels = 32.0 * 1024.0 * 1024.0;
            let cap = (max_dimension / req_w)
                .min(max_dimension / req_h)
                .min((max_pixels / (req_w * req_h)).sqrt())
                .min(1.0);
            let scene_width = (req_w * cap).ceil().max(1.0) as u32;
            let scene_height = (req_h * cap).ceil().max(1.0) as u32;
            match &mut self.state.scene_target {
                Some(target) => target.resize_if_needed(device, scene_width, scene_height),
                None => self.state.scene_target = Some(editor_render::offscreen::SceneTarget::create(device, scene_width, scene_height)),
            }

            // Alvo intermediário do chain pixel-art: filtro em múltiplo
            // inteiro (2xSaI 2x, xBRZ 4x) antes do blit final para a janela.
            let up_ratio = if (mode == 2 || mode == 3) && zoom > 1.0 { Some(if mode == 2 { 2 } else { 4 }) } else { None };
            if let Some(ratio) = up_ratio {
                let fw = (req_w * ratio as f32).ceil().max(1.0);
                let fh = (req_h * ratio as f32).ceil().max(1.0);
                let cap = (max_dimension / fw)
                    .min(max_dimension / fh)
                    .min((max_pixels / (fw * fh)).sqrt())
                    .min(1.0);
                let filter_width = (fw * cap).ceil().max(1.0) as u32;
                let filter_height = (fh * cap).ceil().max(1.0) as u32;
                match &mut self.state.filter_target {
                    Some(target) => target.resize_if_needed(device, filter_width, filter_height),
                    None => self.state.filter_target = Some(editor_render::offscreen::SceneTarget::create(device, filter_width, filter_height)),
                }
            }

            // Zoom exato por eixo; sem arredondamento a célula do filtro fica
            // fracionária (ex: 2.001 em vez de 2) e treme durante o zoom.
            let scene_zoom = [
                design_zoom[0] * scene_width as f32 / req_w.max(1.0),
                design_zoom[1] * scene_height as f32 / req_h.max(1.0),
            ];

            // MDAPT (checkerboard dithering) roda antes do upscaling quando a
            // cena ainda está em 1x (1 texel por pixel do mapa): modos 2/3 em
            // zoom-in e as vistas 1:1 de zoom-out. Em densidades supersampleadas
            // (Retro) ele misturaria o interior do texel, então fica off.
            let native_scene = (scene_zoom[0] - 1.0).abs() < 1e-3 && (scene_zoom[1] - 1.0).abs() < 1e-3;
            let mdapt_active = self.state.shaders.checkerboard_dither && native_scene;
            if mdapt_active {
                for slot in self.state.mdapt_targets.iter_mut() {
                    match slot {
                        Some(target) => target.resize_if_needed(device, scene_width, scene_height),
                        None => *slot = Some(editor_render::offscreen::SceneTarget::create(device, scene_width, scene_height)),
                    }
                }
            }

            // Iluminação (método OTClient): só ativa quando há dimming real
            // (isDark: < 0.99). Em dia pleno (slider 100) não há pass de luz.
            let lights_active = (self.state.world_light as f32 / 100.0) < 0.99;
            if lights_active {
                match &mut self.state.light_target {
                    Some(target) => target.resize_if_needed(device, scene_width, scene_height),
                    None => self.state.light_target = Some(editor_render::offscreen::SceneTarget::create(device, scene_width, scene_height)),
                }
            }

            let camera = editor_render::pipeline::CameraUniform {
                offset: [self.state.camera_offset.x, self.state.camera_offset.y],
                zoom: scene_zoom,
                atlas_columns: 1,
                _align_pad: 0,
                viewport_size: [scene_width as f32, scene_height as f32],
                floor_alpha: 1.0,
                sampling_mode: 0,
                light: if self.state.world_light < MIN_AMBIENT_PCT { MIN_AMBIENT_PCT } else { self.state.world_light } as f32 / 100.0,
                _pad_light: 0,
            };
            if let (Some(resources), Some(atlas), Some(scene), Some(output), Some(scaler)) = (
                &self.state.tile_resources, &self.state.atlas, &self.state.scene_target,
                &self.state.offscreen, self.state.scaler_resources.as_mut(),
            ) {
                editor_render::scene::render_frame(
                    device, queue, resources, &self.state.chunk_cache, atlas,
                    &scene.view, camera, &layers,
                );

                // Pass de luz: light buffer (ambiente + quads das fontes, blend
                // Max por canal) multiplicado pela cena — mesmo método do
                // LightView do OTClient, adaptado para quads por fonte na GPU.
                // O resultado "iluminado" fica num alvo próprio (`light_target`)
                // para não ter feedback de leitura/escrita no mesmo alvo.
                let base_scene: &editor_render::offscreen::SceneTarget = if lights_active {
                    let ambient = if self.state.world_light < MIN_AMBIENT_PCT { MIN_AMBIENT_PCT } else { self.state.world_light } as f32 / 100.0;
                    let ox = camera.offset[0];
                    let oy = camera.offset[1];
                    let vw = camera.viewport_size[0];
                    let vh = camera.viewport_size[1];
                    let zx = camera.zoom[0];
                    let zy = camera.zoom[1];
                    // Corta luzes fora da vista (raio máximo da fonte + folga).
                    let margin = 32.0;
                    let min_tx = ox / 32.0 - margin;
                    let max_tx = (ox + vw / zx) / 32.0 + margin;
                    let min_ty = oy / 32.0 - margin;
                    let max_ty = (oy + vh / zy) / 32.0 + margin;
                    let mut visible_lights: Vec<editor_render::scene::TileLight> = Vec::new();
                    let flicker_t = ui.input(|i| i.time) as f32;
                    let mut raw_lights: Vec<(u8, editor_render::scene::TileLight)> = Vec::new();
                    for layer in &layers {
                        for light in self.state.chunk_cache.lights(layer.z) {
                            if light.intensity > 0.0
                                && light.world_pos[0] >= min_tx && light.world_pos[0] <= max_tx
                                && light.world_pos[1] >= min_ty && light.world_pos[1] <= max_ty {
                                raw_lights.push((layer.z, *light));
                            }
                        }
                    }
                    // Componentes conexos: luzes de item adjacentes do mesmo tipo
                    // compartilham fase (coesas) e amortecem por tamanho; chão e
                    // inanimadas ficam estáticas (`None`).
                    let comps = flicker_component_seed_damp(&raw_lights);
                    for (i, (_z, light)) in raw_lights.iter().enumerate() {
                        match comps[i] {
                            Some((seed, damp)) => visible_lights.push(flicker_light(*light, flicker_t, seed, damp)),
                            None => visible_lights.push(*light),
                        }
                    }
                    scaler.render_lights(device, queue, &camera, &visible_lights, (scene_width, scene_height), [ambient; 3]);
                    let lit = self.state.light_target.as_ref().unwrap();
                    scaler.apply_light(device, queue, &scene.view, &lit.view);
                    lit
                } else {
                    scene
                };

                // Fonte do scaler é o resultado do MDAPT quando ativo (a cena
                // iluminada serviu de entrada das 5 passadas; t[0] tem o merge).
                let scale_source: &editor_render::offscreen::SceneTarget = if mdapt_active {
                    scaler.render_mdapt(device, queue, base_scene, [
                        self.state.mdapt_targets[0].as_ref().unwrap(),
                        self.state.mdapt_targets[1].as_ref().unwrap(),
                        self.state.mdapt_targets[2].as_ref().unwrap(),
                        self.state.mdapt_targets[3].as_ref().unwrap(),
                    ]);
                    self.state.mdapt_targets[0].as_ref().unwrap()
                } else {
                    base_scene
                };
                if let Some(ratio) = up_ratio {
                    if let Some(filter) = &self.state.filter_target {
                        scaler.render_chain(
                            device, queue, scale_source, filter, output, mode, ratio, (scene_zoom, [zoom, zoom]),
                        );
                    }
                } else {
                    scaler.render(
                        device, queue, scale_source, output, mode, (scene_zoom, [zoom, zoom]),
                    );
                }

                // Pós "Shaders" na imagem final (depois do upscaling, extensão acompanha
                // o zoom). O CRT Bloom é um toggle único que combina o lens mist
                // (névoa de lente, ativa de noite — World Light < 50%) e o
                // halation de fósforo (ativo de dia — World Light > 50%), com
                // forças fixas; o CRT Colors (P22) vai por último. Cada passada
                // se auto-gateia pelo World Light (força 0 = identidade). O
                // resultado termina sempre em `output`.
                if self.state.shaders.crt_bloom || self.state.shaders.crt_color {
                    if let Some(post) = &self.state.post_target {
                        let post_size = (output.width, output.height);
                        if self.state.shaders.crt_bloom {
                            scaler.post_mist(device, queue, post_size, &output.view, &post.view, LENS_MIST_STRENGTH, self.state.world_light);
                            scaler.post_bloom(device, queue, post_size, &post.view, &output.view, CRT_BLOOM_STRENGTH, self.state.world_light);
                            if self.state.shaders.crt_color {
                                scaler.post_color(device, queue, &output.view, &post.view);
                                scaler.copy_scene(device, queue, post_size, &post.view, &output.view);
                            }
                        } else if self.state.shaders.crt_color {
                            scaler.post_color(device, queue, &output.view, &post.view);
                            scaler.copy_scene(device, queue, post_size, &post.view, &output.view);
                        }
                    }
                }

                // Conversão final linear → sRGB para apresentação no egui.
                // O pipeline roda todo em linear; esta passagem converte para sRGB antes de exibir.
                if let Some(post) = &self.state.post_target {
                    scaler.post_srgb(device, queue, &output.view, &post.view);
                    scaler.copy_scene(device, queue, (output.width, output.height), &post.view, &output.view);
                }
            }

            let id = self.state.offscreen.as_ref().unwrap().id;
            ui.painter().image(
                id, rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(60, 0, 0));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER,
                "wgpu render state indisponível", egui::FontId::default(), egui::Color32::WHITE);
        }

        if resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let local = pos - rect.min;
                let tile_x = ((local.x / self.state.camera_zoom + self.state.camera_offset.x) / 32.0).floor() as u16;
                let tile_y = ((local.y / self.state.camera_zoom + self.state.camera_offset.y) / 32.0).floor() as u16;
                let world_pos = editor_core::position::Position {
                    x: tile_x, y: tile_y, z: self.state.current_floor_display,
                };
                let doc = &mut self.state.documents[doc_index];
                let mut tx = doc.begin_transaction("Paint");
                tx.record_before(doc, world_pos);
                let mut tile = doc.map.get_tile(world_pos).cloned().unwrap_or_default();
                let sprite_id = self.state.atlas.as_ref().map(|a| 1 + (42 % a.layer_count())).unwrap_or(1);
                tile.ground = Some(editor_core::item::Item::new(sprite_id as u16));
                tx.set_after(world_pos, tile);
                tx.commit(doc);
            }
        }

        ui.horizontal(|ui| {
            let h = &self.state.hover_info;
            ui.label(format!("Position: [{}, {}, {}]", h.x, h.y, h.z));
            ui.separator();
            ui.label(format!("Floor: {}", self.state.current_floor_display));
            ui.separator();
            ui.label(format!("Zoom: {}%", (self.state.camera_zoom * 100.0) as i32));
        });
    }

    fn ui_objects(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Filter: City / Biome").weak());
        egui::ComboBox::from_id_salt("city_filter")
            .selected_text(if self.state.city_filter.is_empty() {
                "[8.0] Svargrond - Ice & Viking Isle"
            } else { &self.state.city_filter })
            .show_ui(ui, |_ui| { /* popular via ItemTypeTable/worlds */ });

        ui.separator();
        ui.label(RichText::new("Filter: Item Name").weak());
        ui.text_edit_singleline(&mut self.state.item_name_filter);

        ui.separator();
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            ui.label("(lista de itens filtrados — sprite atlas aqui)");
        });

        ui.separator();

        ui.label(RichText::new("Selection").weak());
        radio_button(ui, &mut self.state.selection_brush, SelectionBrush::SingleSelect, "Single Select");
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.selection_brush, SelectionBrush::Eraser, "Eraser");
            radio_button(ui, &mut self.state.selection_brush, SelectionBrush::Border, "Border");
        });

        ui.label(RichText::new("Zones").weak());
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::ProtectionZone, "PZ");
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::NoPvp, "NoPvp");
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::Pvp, "Pvp");
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::NoLogout, "BlockLogout");
        });

        ui.label(RichText::new("Doors").weak());
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Normal, "Normal");
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Quest, "Quest");
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Locked, "Locked");
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Magic, "Magic");
        });

        ui.label(RichText::new("Windows").weak());
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.window_brush, WindowBrush::Normal, "Normal");
            radio_button(ui, &mut self.state.window_brush, WindowBrush::Hatched, "Hatched");
        });

        ui.label(RichText::new("Auto-bordering").weak());
        ui.checkbox(&mut self.state.auto_border_active, "Active");

        ui.label(RichText::new("Brush Thickness").weak());
        ui.add(egui::Slider::new(&mut self.state.brush_thickness, 0..=10));

        ui.label(RichText::new("Brush Size").weak());
        ui.add(egui::Slider::new(&mut self.state.brush_size, 0..=10));

        ui.label(RichText::new("Brush Type").weak());
        ui.horizontal(|ui| {
            radio_button_req(ui, &mut self.state.brush_shape, BrushShape::Circle, "Circle");
            radio_button_req(ui, &mut self.state.brush_shape, BrushShape::Square, "Square");
        });
    }

    fn ui_placeholder(&mut self, ui: &mut Ui, name: &str) {
        ui.label(RichText::new(format!("{name} — em construção")).weak());
    }

fn ui_world(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("World Light").weak());
        ui.add(egui::Slider::new(&mut self.state.world_light, 0..=100));
        ui.separator();
        ui.checkbox(&mut self.state.show_tooltips, "Show Tooltips");
        ui.checkbox(&mut self.state.show_npcs, "Show NPCs");
        ui.checkbox(&mut self.state.show_monsters, "Show Monsters");
        ui.checkbox(&mut self.state.show_zones, "Show Zones");

        ui.separator();
        ui.label(RichText::new("Anti-aliasing").weak());
        egui::ComboBox::from_id_salt("antialiasing")
            .selected_text(self.state.antialiasing.label())
            .show_ui(ui, |ui| {
                for opt in [AntiAliasing::Off, AntiAliasing::Retro, AntiAliasing::Blurry, AntiAliasing::RoundedEdges] {
                    ui.selectable_value(&mut self.state.antialiasing, opt, opt.label());
                }
            });

        ui.separator();
        ui.label(RichText::new("Shaders").weak());
        ui.checkbox(&mut self.state.shaders.checkerboard_dither, "Checkerboard Dithering")
            .on_hover_text("Merge Dithering and Pseudo Transparency (MDAPT, Sp00kyFox): funde os padrões de dithering/transparência antes do upscaling, quando a cena está em 1x.");
        ui.checkbox(&mut self.state.shaders.crt_bloom, "CRT Bloom")
            .on_hover_text("Lens mist (névoa difusa de lente) à noite, com World Light abaixo de 50% (raio espalha conforme escurece, foco em cores frias), e halation de fósforo de dia, acima de 50% (raio aperta conforme clareia, foco em cores quentes). Sem clarear a noite nem lavar o dia.");
        ui.checkbox(&mut self.state.shaders.crt_color, "CRT Colors")
            .on_hover_text("Gama de fósforo SMPTE-C/Rec.601 (P22 dos CRTs de consumo, grade.glsl/Dogway): vermelho fica levemente dessaturado e esquenta pro laranja, azul puxa pro ciano, branco preservado — sem boost de saturação.");
    }

    fn ui_inspector(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Tile properties").strong());
        ui.separator();
        ui.label(RichText::new("Walking").weak());
        ui.label("Ground Speed: 220");
        ui.label("Block Path: false");
        ui.label(RichText::new("Zones").weak());
        ui.label("No Logout Zone: false");
        ui.label("Protection Zone: false");

        ui.separator();
        ui.label(RichText::new("Item properties").strong());
        ui.horizontal(|ui| {
            ui.button("Move Up").clicked();
            ui.button("Move Down").clicked();
            ui.add(egui::Button::new("Delete").fill(egui::Color32::from_rgb(0xff, 0x67, 0x67))).clicked();
        });
        ui.label("Item ID"); ui.add(egui::DragValue::new(&mut 0));
        ui.label("Text"); ui.text_edit_multiline(&mut String::new());
    }

    fn ui_minimap(&mut self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(10, 12, 10));
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Minimap", egui::FontId::default(), egui::Color32::DARK_GRAY);
    }

    fn ui_console(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for line in &self.state.log_lines {
                ui.label(line);
            }
        });
    }
}
