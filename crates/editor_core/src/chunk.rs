use crate::{item::Tile, position::CHUNK_SIZE};

/// Um andar de um chunk 32x32. Alocado sob demanda — só existe se algum
/// tile dentro dele for editado.
pub struct Chunk {
    // Vec plano ao invés de [[Tile; 32]; 32]: melhor localidade,
    // permite swap/mem::take barato em undo/redo.
    tiles: Vec<Tile>,
    pub dirty: bool, // sinaliza render/re-tessellate do chunk (batching de mesh)
}

impl Chunk {
    pub fn new_empty() -> Self {
        Self {
            tiles: vec![Tile::default(); (CHUNK_SIZE as usize) * (CHUNK_SIZE as usize)],
            dirty: true,
        }
    }

    #[inline]
    pub fn tile(&self, local_index: usize) -> &Tile { &self.tiles[local_index] }

    #[inline]
    pub fn tile_mut(&mut self, local_index: usize) -> &mut Tile {
        self.dirty = true;
        &mut self.tiles[local_index]
    }
}