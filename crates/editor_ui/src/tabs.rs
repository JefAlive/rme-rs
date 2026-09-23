use editor_core::MapDocument;
use egui::{RichText, Ui};

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

    // Grupos de brush — cada um independente, nenhum interfere no outro.
    pub selection_brush: Option<SelectionBrush>,
    pub zone_brush: Option<ZoneBrush>,
    pub door_brush: Option<DoorBrush>,
    pub window_brush: Option<WindowBrush>,
    pub brush_shape: BrushShape,
    pub auto_border_active: bool,
    pub brush_thickness: i32,
    pub brush_size: i32,

    // Filtros do painel Objects
    pub city_filter: String,
    pub item_name_filter: String,

    // World settings
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
            atlas: None
        }
    }
}

#[derive(Default, Clone)]
pub struct HoverInfo {
    pub x: i32, pub y: i32, pub z: i32,
    pub item_id: u32,
    pub item_name: String,
}

/// Helper genérico: botão que participa de um grupo de seleção única,
/// sem afetar nenhum outro grupo.
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

            let doc = &mut self.state.documents[doc_index];
            // Fase 4+: resolver real (ItemTypeTable -> sprite id -> layer do atlas).
            self.state.chunk_cache.sync(device, &mut doc.map, |type_id| type_id as u32);

            let camera = editor_render::pipeline::CameraUniform {
                offset: [self.state.camera_offset.x, self.state.camera_offset.y],
                zoom: self.state.camera_zoom,
                _pad: 0.0,
                viewport_size: [width as f32, height as f32],
                _pad2: [0.0, 0.0],
            };
            if let (Some(resources), Some(atlas)) = (&self.state.tile_resources, &self.state.atlas) {
                editor_render::scene::render_frame(
                    device, queue, resources, &self.state.chunk_cache, atlas,
                    self.state.offscreen.as_ref().unwrap(), camera,
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

        // clique pinta um tile — mesma lógica de antes, agora convertendo
        // coordenada de tela -> mundo considerando pan/zoom
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
                let sprite_id = self.state.atlas.as_ref().map(|a| 1 + (42 % a.layer_count)).unwrap_or(1);
                tile.ground = Some(editor_core::item::Item::new(sprite_id as u16));
                tx.set_after(world_pos, tile);
                tx.commit(doc);
            }
        }

        // status bar (igual ao que já tínhamos)
        ui.horizontal(|ui| {
            let h = &self.state.hover_info;
            ui.label(format!("Position: [{}, {}, {}]", h.x, h.y, h.z));
            ui.separator();
            ui.label(format!("Zoom: {}%", (self.state.camera_zoom * 100.0) as i32));
        });
    }

    /// Antigo "Palette" — agora é só o conteúdo da aba Objects.
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

        // ---- Selection: grupo independente ----
        ui.label(RichText::new("Selection").weak());
        radio_button(ui, &mut self.state.selection_brush, SelectionBrush::SingleSelect, "Single Select");
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.selection_brush, SelectionBrush::Eraser, "Eraser");
            radio_button(ui, &mut self.state.selection_brush, SelectionBrush::Border, "Border");
        });

        // ---- Zones: grupo independente (não mexe em Selection nem Doors) ----
        ui.label(RichText::new("Zones").weak());
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::ProtectionZone, "PZ");
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::NoPvp, "NoPvp");
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::Pvp, "Pvp");
            radio_button(ui, &mut self.state.zone_brush, ZoneBrush::NoLogout, "BlockLogout");
        });

        // ---- Doors: grupo independente ----
        ui.label(RichText::new("Doors").weak());
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Normal, "Normal");
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Quest, "Quest");
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Locked, "Locked");
            radio_button(ui, &mut self.state.door_brush, DoorBrush::Magic, "Magic");
        });

        // ---- Windows: grupo independente (faltava no protótipo anterior) ----
        ui.label(RichText::new("Windows").weak());
        ui.horizontal(|ui| {
            radio_button(ui, &mut self.state.window_brush, WindowBrush::Normal, "Normal");
            radio_button(ui, &mut self.state.window_brush, WindowBrush::Hatched, "Hatched");
        });

        // ---- Estes já eram independentes de verdade, só corrigindo o binding ----
        ui.label(RichText::new("Auto-bordering").weak());
        ui.checkbox(&mut self.state.auto_border_active, "Active");

        ui.label(RichText::new("Brush Thickness").weak());
        ui.add(egui::Slider::new(&mut self.state.brush_thickness, 0..=10));

        ui.label(RichText::new("Brush Size").weak());
        ui.add(egui::Slider::new(&mut self.state.brush_size, 0..=10));

        // ---- Brush Type: agora é de fato um select (Circle xor Square) ----
        ui.label(RichText::new("Brush Type").weak());
        ui.horizontal(|ui| {
            radio_button_req(ui, &mut self.state.brush_shape, BrushShape::Circle, "Circle");
            radio_button_req(ui, &mut self.state.brush_shape, BrushShape::Square, "Square");
        });
    }

    fn ui_placeholder(&mut self, ui: &mut Ui, name: &str) {
        ui.label(RichText::new(format!("{name} — em construção")).weak());
    }

    /// Nova aba World.
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

    /// Inspector agora só tem Tile/Item properties (World Light saiu daqui).
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
            if ui.button("Move Up").clicked() {}
            if ui.button("Move Down").clicked() {}
            if ui.add(egui::Button::new("Delete").fill(egui::Color32::from_rgb(0xff, 0x67, 0x67))).clicked() {}
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