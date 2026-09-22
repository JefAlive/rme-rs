/// Coordenada global do mundo Tibia. Z=7 é o térreo (0..6 telhados, 8..15 subsolo).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Position {
    pub x: u16,
    pub y: u16,
    pub z: u8,
}

pub const MAP_MAX_Z: u8 = 15;
pub const GROUND_FLOOR: u8 = 7;

pub const CHUNK_SIZE: u16 = 32; // tiles por lado, por andar

/// Coordenada de chunk (não confundir com Position — já dividida por CHUNK_SIZE)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    pub cx: i32,
    pub cy: i32,
    pub z: u8,
}

impl Position {
    #[inline]
    pub fn chunk_coord(self) -> ChunkCoord {
        ChunkCoord {
            cx: (self.x / CHUNK_SIZE) as i32,
            cy: (self.y / CHUNK_SIZE) as i32,
            z: self.z,
        }
    }

    /// Índice local dentro do chunk (0..CHUNK_SIZE*CHUNK_SIZE)
    #[inline]
    pub fn local_index(self) -> usize {
        let lx = (self.x % CHUNK_SIZE) as usize;
        let ly = (self.y % CHUNK_SIZE) as usize;
        ly * CHUNK_SIZE as usize + lx
    }
}