use ahash::AHashMap;
use crate::{chunk::Chunk, item::Tile, position::{ChunkCoord, Position}};

/// O mapa inteiro. Chunks vazios simplesmente não existem no HashMap —
/// custo de memória zero para áreas não editadas do mundo.
#[derive(Default)]
pub struct SpatialMap {
    chunks: AHashMap<ChunkCoord, Chunk>,
}

impl SpatialMap {
    pub fn get_tile(&self, pos: Position) -> Option<&Tile> {
        self.chunks.get(&pos.chunk_coord()).map(|c| c.tile(pos.local_index()))
    }

    /// Cria o chunk sob demanda (copy-on-write conceitual).
    pub fn get_tile_mut(&mut self, pos: Position) -> &mut Tile {
        let chunk = self.chunks.entry(pos.chunk_coord()).or_insert_with(Chunk::new_empty);
        chunk.tile_mut(pos.local_index())
    }

    /// Usado pelo pipeline de undo: se após restaurar o tile ele fica vazio
    /// E foi o último tile ocupado do chunk, o chunk pode ser liberado.
    pub fn compact_chunk_if_empty(&mut self, coord: ChunkCoord) {
        if let Some(chunk) = self.chunks.get(&coord) {
            // custo O(1024) só ocorre no caminho frio (raramente)
            if (0..1024).all(|i| chunk.tile(i).is_empty()) {
                self.chunks.remove(&coord);
            }
        }
    }

    pub fn iter_dirty_chunks(&self) -> impl Iterator<Item = (&ChunkCoord, &Chunk)> {
        self.chunks.iter().filter(|(_, c)| c.dirty)
    }

    pub fn clear_dirty(&mut self, coord: &ChunkCoord) {
        if let Some(c) = self.chunks.get_mut(coord) { c.dirty = false; }
    }
}