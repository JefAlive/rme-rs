pub mod position;
pub mod item;
pub mod chunk;
pub mod spatial_map;
pub mod history;

use position::Position;
use spatial_map::SpatialMap;
use history::{History, EditCommand, TileEditTransaction, TileDelta};

/// Um mapa aberto (corresponde a uma aba de tab-bar no seu layout ImRAD:
/// "Global.otbm", "YurOTs.otbm" etc.)
pub struct MapDocument {
    pub name: String,
    pub map: SpatialMap,
    pub history: History,
    pub current_floor: u8,
    dirty: bool,
}

impl MapDocument {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            map: SpatialMap::default(),
            history: History::new(500),
            current_floor: position::GROUND_FLOOR,
            dirty: false,
        }
    }

    /// Builder de transação — usado pelas ferramentas de pincel (ver editor_ui).
    pub fn begin_transaction(&self, label: &'static str) -> TransactionBuilder<'_> {
        TransactionBuilder { doc: self, deltas: Vec::new(), label }
    }
}

pub struct TransactionBuilder<'a> {
    doc: &'a MapDocument,
    deltas: Vec<TileDelta>,
    label: &'static str,
}

impl<'a> TransactionBuilder<'a> {
    /// Registra o "antes" de um tile antes de mutá-lo externamente.
    pub fn record_before(&mut self, pos: Position) {
        let before = self.doc.map.get_tile(pos).cloned().unwrap_or_default();
        let after = before.clone();
        self.deltas.push(TileDelta { pos, before, after });
    }

    pub fn set_after(&mut self, pos: Position, after: crate::item::Tile) {
        if let Some(d) = self.deltas.iter_mut().find(|d| d.pos == pos) {
            d.after = after;
        }
    }

    pub fn commit(self, doc: &mut MapDocument) {
        if self.deltas.is_empty() { return; }
        doc.history.push(
            EditCommand::TileEdit(TileEditTransaction { deltas: self.deltas, label: self.label }),
            &mut doc.map,
        );
        doc.dirty = true;
    }
}