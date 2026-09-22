use crate::{item::Tile, position::Position, spatial_map::SpatialMap};

/// Delta contíguo de um único tile dentro de uma transação.
pub struct TileDelta {
    pub pos: Position,
    pub before: Tile,
    pub after: Tile,
}

/// Uma transação = tudo que aconteceu entre mouse-down e mouse-up.
/// Isso é o "Command" — não precisa de trait object porque só existe
/// um tipo de operação (edição de tile). Operações futuras (ex: mover
/// casa, renomear spawn) entram como variantes do enum abaixo.
pub struct TileEditTransaction {
    pub deltas: Vec<TileDelta>,
    pub label: &'static str, // para UI: "Draw Grass", "Erase", "Fill" etc
}

pub enum EditCommand {
    TileEdit(TileEditTransaction),
    // futuro: HouseRename { id, before, after }, SpawnMove { .. }, etc.
}

impl EditCommand {
    pub fn apply(&self, map: &mut SpatialMap) {
        match self {
            EditCommand::TileEdit(tx) => {
                for d in &tx.deltas {
                    *map.get_tile_mut(d.pos) = d.after.clone();
                }
            }
        }
    }

    pub fn undo(&self, map: &mut SpatialMap) {
        match self {
            EditCommand::TileEdit(tx) => {
                for d in &tx.deltas {
                    *map.get_tile_mut(d.pos) = d.before.clone();
                    map.compact_chunk_if_empty(d.pos.chunk_coord());
                }
            }
        }
    }
}

/// Pilha de histórico. Sem limite de memória "infinito com vazamento":
/// aqui é só Vec<EditCommand> — capacidade de descarte configurável.
pub struct History {
    undo_stack: Vec<EditCommand>,
    redo_stack: Vec<EditCommand>,
    max_depth: usize,
}

impl History {
    pub fn new(max_depth: usize) -> Self {
        Self { undo_stack: Vec::new(), redo_stack: Vec::new(), max_depth }
    }

    pub fn push(&mut self, cmd: EditCommand, map: &mut SpatialMap) {
        cmd.apply(map);
        self.redo_stack.clear();
        self.undo_stack.push(cmd);
        if self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0); // custo aceitável; troque por VecDeque se doer
        }
    }

    pub fn undo(&mut self, map: &mut SpatialMap) -> bool {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.undo(map);
            self.redo_stack.push(cmd);
            true
        } else { false }
    }

    pub fn redo(&mut self, map: &mut SpatialMap) -> bool {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.apply(map);
            self.undo_stack.push(cmd);
            true
        } else { false }
    }
}