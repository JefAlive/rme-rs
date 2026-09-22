use editor_core::MapDocument;
use editor_ui::tabs::{AppState, EditorTab, EditorTabViewer};
use egui_dock::{DockArea, DockState, NodeIndex, Style};

struct RmeApp {
    dock_state: DockState<EditorTab>,
    state: AppState,
}

impl RmeApp {
    fn new() -> Self {
        let mut state = AppState::default();
        state.documents.push(MapDocument::new("Global.otbm"));
        state.log_lines.push("[core] editor_core inicializado sem UI/GPU.".into());

        let mut dock_state = DockState::new(vec![EditorTab::Viewport { doc_index: 0 }]);
        let surface = dock_state.main_surface_mut();

        // Objects/Towns/Houses/Zones/Project como abas reais no mesmo node —
        // egui_dock renderiza a tab-bar nativa automaticamente.
        let [center, left] = surface.split_left(
            NodeIndex::root(), 0.22,
            vec![EditorTab::Objects, EditorTab::Towns, EditorTab::Houses, EditorTab::Zones, EditorTab::Project],
        );

        // Inspector e World compartilhando o painel direito como abas.
        let [_, right] = surface.split_right(
            center, 0.78, vec![EditorTab::Inspector, EditorTab::World],
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
        ..Default::default()
    };
    eframe::run_native(
        "RME-rs — AAA Map Editor Foundation",
        native_options,
        Box::new(|_cc| Ok(Box::new(RmeApp::new()))),
    )
}