use editor_core::import::{import_otbm, is_subtype_embedded, Bounds};
use editor_formats::otbm::parse;
use editor_render::atlas::SpriteAtlas;
use editor_render::assets::SpriteResolver;
use editor_ui::tabs::{AppState, EditorTab, EditorTabViewer};
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use std::path::Path;

/// Caminhos de debug para assets e mapa real.
const ASSETS_DIR: &str = "reference-assets";
const MAP_PATH: &str = "reference-maps/Dawnport.otbm";

struct RmeApp {
    dock_state: DockState<EditorTab>,
    state: AppState,
}

impl RmeApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Diagnóstico wgpu
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

        // Atlas + tabela de animação vazios (crescem sob demanda via SpriteResolver)
        if let Some(rs) = &state.wgpu {
            state.atlas = Some(SpriteAtlas::new(&rs.device));
            state.anim_table = Some(editor_render::anim::AnimTable::new(&rs.device, &rs.queue));
        }

        // Carregar assets reais + mapa
        if state.wgpu.is_some() {
            match Self::load_real_assets(&mut state) {
                Ok(bounds) => {
                    eprintln!("mapa carregado: bbox=({}..{} x {}..{}) z={}..{}",
                        bounds.min_x, bounds.max_x, bounds.min_y, bounds.max_y,
                        bounds.min_z, bounds.max_z);
                    // O camera_fit no tabs.rs centraliza no primeiro frame
                    state.camera_fit_pending = true;
                }
                Err(e) => {
                    eprintln!("falha carregando assets/mapa: {e}; usando mapa demo vazio");
                    state.camera_fit_pending = false;
                }
            }
        } else {
            eprintln!("wgpu não disponível — modo headless");
        }

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

impl RmeApp {
    /// Carrega appearances.dat + catalog-content.json + Dawnport.otbm.
    /// Retorna o bbox do mapa para camera fit.
    fn load_real_assets(
        state: &mut AppState,
    ) -> Result<Bounds, String> {
        // 1. Carregar appearances.dat via editor_formats
        let appearances_path = Path::new(ASSETS_DIR).join("appearances-e8a12a674c42b8383b7205a42146efe7976f4beac9bf16353ec96dbdb62891e8.dat");
        let appearances_data = std::fs::read(&appearances_path)
            .map_err(|e| format!("falha lendo appearances.dat: {e}"))?;
        let table = editor_formats::appearances::load_appearances(&appearances_data)
            .ok_or("falha parseando appearances.dat")?;

        // 2. Carregar catalog-content.json + sheets (SpriteResolver)
        let resolver = SpriteResolver::load(ASSETS_DIR)
            .map_err(|e| format!("falha carregando SpriteResolver: {e}"))?;
        state.sprite_resolver = Some(resolver);

        // 3. Parse OTBM
        let otbm_data = std::fs::read(MAP_PATH)
            .map_err(|e| format!("falha lendo {MAP_PATH}: {e}"))?;
        let subtype_embedded = |id| is_subtype_embedded(&table, id);
        let doc = parse(&otbm_data, subtype_embedded)
            .map_err(|e| format!("parse OTBM falhou: {e}"))?;

        // 4. Estatísticas de importação (logs úteis para debug).
        //    OtmTile (saída crua do parser) NÃO sabe o que é "chão" — isso só
        //    é decidido consultando a ItemTypeTable, então replicamos aqui a
        //    mesma checagem que `import_otbm`/`add_item` fazem internamente:
        //    o primeiro item cujo ItemGroup é Ground é considerado o chão.
        let mut per_floor: std::collections::BTreeMap<u8, (u64, u64)> = std::collections::BTreeMap::new();
        let mut grounds_z7: u64 = 0;
        let mut items_z7: u64 = 0;
        for t in &doc.tiles {
            let entry = per_floor.entry(t.z).or_default();
            entry.0 += 1;
            entry.1 += t.items.len() as u64;
            if t.z == 7 {
                let has_ground = t.items.iter().any(|item| {
                    table.get_opt(item.id)
                        .is_some_and(|ty| ty.group == editor_formats::appearances::ItemGroup::Ground)
                });
                if has_ground {
                    grounds_z7 += 1;
                }
                items_z7 += t.items.len() as u64;
            }
        }

        // 5. Importar para MapDocument (com bounds)
        let (doc_map, bounds) = import_otbm(&doc, &table);
        let mut state_ref = std::mem::take(state);
        state_ref.documents.push(doc_map);
        *state = state_ref;

        // 6. Logs de estatísticas
        eprintln!("import OTBM: tiles={} warnings={} floors={}",
            doc.tiles.len(), doc.warnings.len(), per_floor.len());
        for (z, (tiles, items)) in &per_floor {
            eprintln!("import OTBM: z={z} tiles={tiles} items={items}");
        }
        eprintln!("import OTBM: z=7 ground_tiles={grounds_z7} items={items_z7}");

        Ok(bounds)
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