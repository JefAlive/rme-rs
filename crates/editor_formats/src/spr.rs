use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

pub struct SpriteCatalog {
    file: File,
    offsets: Vec<u32>, // offsets[i] corresponde ao sprite id (i+1)
    has_alpha: bool,
}

#[derive(Debug)]
pub enum SprError {
    Io(io::Error),
}

impl From<io::Error> for SprError {
    fn from(e: io::Error) -> Self { SprError::Io(e) }
}

impl SpriteCatalog {
    pub fn load(path: &str, extended_count: bool, has_alpha: bool) -> Result<Self, SprError> {
        let mut file = File::open(path)?;

        let mut sig = [0u8; 4]; // versão/assinatura do arquivo — não usamos
        file.read_exact(&mut sig)?;

        let count = if extended_count {
            let mut buf = [0u8; 4];
            file.read_exact(&mut buf)?;
            u32::from_le_bytes(buf)
        } else {
            let mut buf = [0u8; 2];
            file.read_exact(&mut buf)?;
            u16::from_le_bytes(buf) as u32
        };

        let mut offsets = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let mut buf = [0u8; 4];
            file.read_exact(&mut buf)?;
            offsets.push(u32::from_le_bytes(buf));
        }

        Ok(Self { file, offsets, has_alpha })
    }

    pub fn sprite_count(&self) -> usize {
        self.offsets.len()
    }

    /// Decodifica o sprite `id` (1-based) para RGBA 32x32 (4096 bytes).
    /// Fora do range ou offset 0 => sprite vazio (transparente).
    pub fn decode(&mut self, id: u32) -> Result<[u8; 32 * 32 * 4], SprError> {
        let mut out = [0u8; 32 * 32 * 4];
        let idx = id as usize;
        if idx == 0 || idx > self.offsets.len() {
            return Ok(out);
        }
        let offset = self.offsets[idx - 1];
        if offset == 0 {
            return Ok(out);
        }

        self.file.seek(SeekFrom::Start(offset as u64))?;

        let mut key = [0u8; 3]; // color-key de transparência — não usado no decode
        self.file.read_exact(&mut key)?;

        let mut len_buf = [0u8; 2];
        self.file.read_exact(&mut len_buf)?;
        let data_len = u16::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; data_len];
        self.file.read_exact(&mut data)?;

        let bpp = if self.has_alpha { 4 } else { 3 };
        let mut cursor = 0usize;
        let mut pixel = 0usize;

        while cursor + 4 <= data.len() && pixel < 1024 {
            let transparent = u16::from_le_bytes([data[cursor], data[cursor + 1]]) as usize;
            let colored = u16::from_le_bytes([data[cursor + 2], data[cursor + 3]]) as usize;
            cursor += 4;
            pixel += transparent;

            for _ in 0..colored {
                if pixel >= 1024 || cursor + bpp > data.len() { break; }
                let (r, g, b) = (data[cursor], data[cursor + 1], data[cursor + 2]);
                let a = if self.has_alpha { data[cursor + 3] } else { 255 };
                cursor += bpp;

                let o = pixel * 4;
                out[o] = r; out[o + 1] = g; out[o + 2] = b; out[o + 3] = a;
                pixel += 1;
            }
        }

        Ok(out)
    }
}