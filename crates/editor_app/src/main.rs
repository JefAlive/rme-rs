use editor_core::{
    item::Item,
    position::{Position, GROUND_FLOOR},
    MapDocument,
};
use editor_formats::spr::SpriteCatalog;
use editor_render::atlas::SpriteAtlas;
use editor_ui::tabs::{AppState, EditorTab, EditorTabViewer};
use egui_dock::{DockArea, DockState, NodeIndex, Style};

const SPR_PATH: &str = "C:/Caminho/Para/Tibia.spr";
const SPRITE_LOAD_COUNT: u32 = 256; // v0: só os primeiros N, rápido de carregar
const SPR_EXTENDED_COUNT: bool = true; // tente `false` se a arte sair corrompida
const SPR_HAS_ALPHA: bool = true;      // tente `false` se as cores saírem erradas

struct RmeApp {
    dock_state: DockState<EditorTab>,
    state: AppState,
}

impl RmeApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // --- Diagnóstico: confirma se o backend wgpu realmente inicializou ---
        eprintln!("wgpu_render_state presente? {}", cc.wgpu_render_state.is_some());
        if let Some(rs) = &cc.wgpu_render_state {
            let info = rs.adapter.get_info();
            eprintln!("backend em uso: {:?}", info.backend);
            eprintln!("adapter: {}", info.name);
        }

        let mut state = AppState::default();
        state.wgpu = cc.wgpu_render_state.clone();

        let mut atlas_opt = None;
        if let Some(rs) = &state.wgpu {
            match SpriteCatalog::load(SPR_PATH, SPR_EXTENDED_COUNT, SPR_HAS_ALPHA) {
                Ok(mut catalog) => {
                    let n = catalog.sprite_count().min(SPRITE_LOAD_COUNT as usize).max(1);
                    let sprites: Vec<_> = (1..=n as u32).map(|id| catalog.decode(id).unwrap_or([0u8; 32*32*4])).collect();
                    eprintln!("[spr] {} sprites carregados de {}", sprites.len(), SPR_PATH);
                    atlas_opt = Some(SpriteAtlas::new(&rs.device, &rs.queue, &sprites));
                }
                Err(e) => {
                    eprintln!("[spr] falha ao carregar '{}': {:?} — usando atlas placeholder", SPR_PATH, e);
                    atlas_opt = Some(SpriteAtlas::new(&rs.device, &rs.queue, &[[0u8; 32*32*4]]));
                }
            }
        }
        state.atlas = atlas_opt;

        let atlas_layers = state.atlas.as_ref().map(|a| a.layer_count).unwrap_or(1);
        let mut doc = MapDocument::new("Global.otbm");
        for y in 0..16u16 {
            for x in 0..16u16 {
                let pos = Position { x, y, z: GROUND_FLOOR };
                let sprite_id = 1 + ((x as u32 + y as u32 * 7) % atlas_layers.max(1));
                doc.map.get_tile_mut(pos).ground = Some(Item::new(sprite_id as u16));
            }
        }
        state.documents.push(doc);

        // Layout do dock: espelha o protótipo ImRAD.
        let mut dock_state = DockState::new(vec![EditorTab::Viewport { doc_index: 0 }]);
        let surface = dock_state.main_surface_mut();

        let [center, left] = surface.split_left(
            NodeIndex::root(),
            0.22,
            vec![
                EditorTab::Objects,
                EditorTab::Towns,
                EditorTab::Houses,
                EditorTab::Zones,
                EditorTab::Project,
            ],
        );

        let [_, right] = surface.split_right(
            center,
            0.78,
            vec![EditorTab::Inspector, EditorTab::World],
        );
        surface.split_below(right, 0.55, vec![EditorTab::Minimap]);
        surface.split_below(left, 0.75, vec![EditorTab::Console]);

        Self { dock_state, state }
    }
}

impl eframe::App for RmeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() { /* TODO */ }
                    if ui.button("Open").clicked() { /* TODO */ }
                    if ui.button("Save").clicked() { /* TODO */ }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        let doc = &mut self.state.documents[self.state.active_doc];
                        doc.history.undo(&mut doc.map);
                    }
                    if ui.button("Redo").clicked() {
                        let doc = &mut self.state.documents[self.state.active_doc];
                        doc.history.redo(&mut doc.map);
                    }
                });
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(0.0))
            .show(ctx, |ui| {
                DockArea::new(&mut self.dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show_inside(ui, &mut EditorTabViewer { state: &mut self.state });
            });
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 860.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "RME-rs — AAA Map Editor Foundation",
        native_options,
        Box::new(|cc| Ok(Box::new(RmeApp::new(cc)))),
    )
}