// crates/editor_core/src/lib.rs

pub mod position;
pub mod item;
pub mod chunk;
pub mod spatial_map;
pub mod history;

use position::Position;
use spatial_map::SpatialMap;
use history::{History, EditCommand, TileEditTransaction, TileDelta};

/// Um mapa aberto (corresponde a uma aba de tab-bar no layout ImRAD:
/// "Global.otbm", "YurOTs.otbm" etc.)
pub struct MapDocument {
    pub name: String,
    pub map: SpatialMap,
    pub history: History,
    pub current_floor: u8,
    pub dirty: bool,
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

    /// Cria um builder de transação totalmente independente — não empresta
    /// `self` de forma alguma, então não conflita com nenhum `&mut` posterior.
    pub fn begin_transaction(&self, label: &'static str) -> TransactionBuilder {
        TransactionBuilder { deltas: Vec::new(), label }
    }
}

/// Uma transação de edição em construção (ex: um gesto de clique/drag do
/// pincel). Não guarda nenhuma referência ao `MapDocument` — cada método
/// que precisa ler o mapa recebe o `&MapDocument` como parâmetro explícito,
/// e o empréstimo termina no fim daquela chamada.
pub struct TransactionBuilder {
    deltas: Vec<TileDelta>,
    label: &'static str,
}

impl TransactionBuilder {
    /// Registra o "antes" de um tile, lendo o estado atual do documento.
    /// O empréstimo imutável de `doc` é transitório: nasce e morre aqui.
    pub fn record_before(&mut self, doc: &MapDocument, pos: Position) {
        let before = doc.map.get_tile(pos).cloned().unwrap_or_default();
        let after = before.clone();
        self.deltas.push(TileDelta { pos, before, after });
    }

    /// Define o estado "depois" de um tile já registrado via `record_before`.
    pub fn set_after(&mut self, pos: Position, after: item::Tile) {
        if let Some(d) = self.deltas.iter_mut().find(|d| d.pos == pos) {
            d.after = after;
        }
    }

    /// Único ponto que exige `&mut MapDocument` — como `self` é consumido
    /// por valor aqui, não há nenhum empréstimo concorrente vivo.
    pub fn commit(self, doc: &mut MapDocument) {
        if self.deltas.is_empty() {
            return;
        }
        doc.history.push(
            EditCommand::TileEdit(TileEditTransaction { deltas: self.deltas, label: self.label }),
            &mut doc.map,
        );
        doc.dirty = true;
    }
}