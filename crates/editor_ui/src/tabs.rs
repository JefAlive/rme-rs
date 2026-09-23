use editor_core::{MapDocument, position::Position, spatial_map::SpatialMap};
use egui::{RichText, Ui};
use editor_render::assets::SpriteResolver;
use editor_render::atlas::SpriteAtlas;

const PAN_SPEED_TILES_PER_SEC: f32 = 12.0;
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 8.0;

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
pub enum AntiAliasing { Off, Retro, CrtBlend, RoundedEdges }

impl AntiAliasing {
    fn label(self) -> &'static str {
        match self {
            AntiAliasing::Off => "Off",
            AntiAliasing::Retro => "Retro",
            AntiAliasing::CrtBlend => "CRT Blend",
            AntiAliasing::RoundedEdges => "Rounded Edges",
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

    pub current_zoom: u32,
    pub current_floor_display: u8,
    pub hover_info: HoverInfo,
    pub log_lines: Vec<String>,

    pub wgpu: Option<egui_wgpu::RenderState>,
    pub tile_resources: Option<editor_render::pipeline::TileRenderResources>,
    pub offscreen: Option<editor_render::offscreen::OffscreenTarget>,
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
            antialiasing: AntiAliasing::Off,
            current_zoom: 100,
            current_floor_display: editor_core::position::GROUND_FLOOR,
            hover_info: HoverInfo::default(),
            log_lines: Vec::new(),
            wgpu: None,
            tile_resources: None,
            offscreen: None,
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
            let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll_y != 0.0 {
                if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                    let local = hover_pos - rect.min;
                    let old_zoom = self.state.camera_zoom;
                    let new_zoom = (old_zoom * (1.0 + scroll_y * 0.001)).clamp(ZOOM_MIN, ZOOM_MAX);
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
                    let resolver = |type_id: u16| -> u32 {
                        let (Some(resolver), Some(atlas)) = (sprite_resolver.as_mut(), atlas_for_resolve.as_mut()) else {
                            return 0;
                        };
                        resolver.layer_for(device, queue, atlas, type_id)
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

            // Pilha de andares a compor: distância do andar atual define alfa
            // e deslocamento diagonal; o andar atual (distância 0) é desenhado
            // por último, opaco, por cima do contexto semitransparente.
            let mut layers: Vec<editor_render::scene::FloorLayer> = Vec::new();
            {
                let mut z = start_z;
                loop {
                    let distance = (end_z as i32 - z as i32).unsigned_abs() as f32;
                    layers.push(editor_render::scene::FloorLayer {
                        z,
                        alpha: if z == end_z { 1.0 } else { 0.35 },
                        pixel_offset: [distance * 32.0, distance * 32.0],
                    });
                    if z == superend_z { break; }
                    z -= 1;
                }
            }
            layers.sort_by_key(|l| if l.z == end_z { 1 } else { 0 });

            let camera = editor_render::pipeline::CameraUniform {
                offset: [self.state.camera_offset.x, self.state.camera_offset.y],
                zoom: self.state.camera_zoom,
                _pad: 0.0,
                viewport_size: [width as f32, height as f32],
                floor_alpha: 1.0,
                _pad2: 0.0,
            };
            if let (Some(resources), Some(atlas)) = (&self.state.tile_resources, &self.state.atlas) {
                editor_render::scene::render_frame(
                    device, queue, resources, &self.state.chunk_cache, atlas,
                    self.state.offscreen.as_ref().unwrap(), camera, &layers,
                );
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
                for opt in [AntiAliasing::Off, AntiAliasing::Retro, AntiAliasing::CrtBlend, AntiAliasing::RoundedEdges] {
                    ui.selectable_value(&mut self.state.antialiasing, opt, opt.label());
                }
            });
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