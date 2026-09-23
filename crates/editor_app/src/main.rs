use editor_core::{
    item::Item,
    position::{Position, GROUND_FLOOR},
    MapDocument,
};
use editor_render::atlas::SpriteAtlas;
use editor_ui::tabs::{AppState, EditorTab, EditorTabViewer};
use egui_dock::{DockArea, DockState, NodeIndex, Style};

/// Fase 0->4: atlas placeholder com alguns quadrados coloridos.
/// A partir da Fase 4 o atlas é alimentado pelos sprites reais
/// (appearances.dat + catalog-content.json + sheets LZMA).
fn placeholder_sprites() -> Vec<[u8; 32 * 32 * 4]> {
    let colors: [[u8; 4]; 4] = [
        [220, 60, 60, 255],
        [60, 200, 90, 255],
        [70, 120, 230, 255],
        [235, 215, 90, 255],
    ];
    colors
        .into_iter()
        .map(|c| {
            let mut rgba = [0u8; 32 * 32 * 4];
            for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
                let (cx, cy) = ((i % 32) as u8, (i / 32) as u8);
                let light = ((cx / 8 + cy / 8) % 2 == 1) as u8;
                let shade = if light == 1 { 0 } else { 90 };
                px[0] = c[0].saturating_sub(shade);
                px[1] = c[1].saturating_sub(shade);
                px[2] = c[2].saturating_sub(shade);
                px[3] = 255;
            }
            rgba
        })
        .collect()
}

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

        let mut state = AppState {
            wgpu: cc.wgpu_render_state.clone(),
            ..Default::default()
        };

        let mut atlas_opt = None;
        if let Some(rs) = &state.wgpu {
            let _sprites = placeholder_sprites();
            eprintln!("atlas placeholder: {} camadas (não usado — atlas cresce sob demanda)", _sprites.len());
            atlas_opt = Some(SpriteAtlas::new(&rs.device));
        }
        state.atlas = atlas_opt;

        let mut doc = MapDocument::new("Global.otbm");
        for y in 0..16u16 {
            for x in 0..16u16 {
                let pos = Position { x, y, z: GROUND_FLOOR };
                let type_id = 1 + ((x + y * 7) % 4);
                doc.map.get_tile_mut(pos).ground = Some(Item::new(type_id));
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