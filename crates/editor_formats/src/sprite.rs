//! Decodificação das sheets `sprites-*.bmp.lzma` do cliente.
//!
//! Fonte da verdade: `reference-src/source/sprite_appearances.cpp::loadSpriteSheet`
//! e o comentário de cabeçalho CIP embutido nele:
//!  - 32 bytes de cabeçalho fixo com padding (nulos), a sequência mágica
//!    `70 0A FA 80 24` e o tamanho 7-bit LZMA;
//!  - 1 byte `lclppb` (lc/lp/pb), 4 bytes dict_size (LE), 8 bytes
//!    "cip compressed size", então os dados LZMA1 **raw**;
//!  - o payload descomprimido é um BMP de 384x384 BGRA em bottom-up, que é
//!    virado verticalmente em `loadSpriteSheet`.

use std::io::Cursor;

/// `SPRITE_SHEET_WIDTH`/`HEIGHT` (384 px) e `BYTES_IN_SPRITE_SHEET`.
pub const SHEET_SIZE: u32 = 384;
pub const SHEET_BYTES: usize = (SHEET_SIZE * SHEET_SIZE * 4) as usize;
/// `LZMA_UNCOMPRESSED_SIZE`: BMP + 122 bytes de cabeçalho extra.
const UNCOMPRESSED_SIZE: usize = SHEET_BYTES + 122;

#[derive(Debug)]
pub enum SheetError {
    TruncatedHeader,
    BadMagic,
    Lzma(lzma_rs::error::Error),
    TruncatedOutput,
}

impl std::fmt::Display for SheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SheetError::TruncatedHeader => write!(f, "cabeçalho CIP truncado"),
            SheetError::BadMagic => write!(f, "sequência mágica CIP inválida"),
            SheetError::Lzma(e) => write!(f, "falha no decode LZMA: {e}"),
            SheetError::TruncatedOutput => write!(f, "saída LZMA menor que o esperado"),
        }
    }
}

/// Descomprime e extrai os pixels BGRA de uma sheet (384x384), com a imagem
/// já virada verticalmente (canto superior-esquerdo na origem, como no RME).
///
/// Configuração LZMA: `lc=3 lp=0 pb=2`, dict_size lido do cabeçalho — mesmos
/// valores/referência da `loadSpriteSheet`.
pub fn decode_sheet(data: &[u8]) -> Result<Vec<u8>, SheetError> {
    // --- cabeçalho CIP (sprite_appearances.cpp) ---
    let mut pos = 0usize;
    loop {
        let b = *data.get(pos).ok_or(SheetError::TruncatedHeader)?;
        pos += 1;
        if b != 0 {
            break;
        }
    }
    // pos = 1 + índice da magic sequence [70 0A FA 80 24]... na verdade o loop
    // sai com `pos` logo após o primeiro byte não-nulo (a própria magic). No C++
    // `while (buffer[pos++] == 0)` → pos = índice(primeiro não-nulo) + 1, e em
    // seguida `pos += 4` consome o resto da sequência mágica.
    pos += 4;
    // tamanho 7-bit (valor descartado, igual ao C++) — pos avança até 1 byte
    // depois do último byte do varint (o byte `lclppb`).
    loop {
        let b = *data.get(pos).ok_or(SheetError::TruncatedHeader)?;
        pos += 1;
        if b & 0x80 == 0 {
            break;
        }
    }

    let lclppb = u32::from(*data.get(pos).ok_or(SheetError::TruncatedHeader)?);
    pos += 1;

    let dict_size = u32::from_le_bytes(
        data.get(pos..pos + 4)
            .ok_or(SheetError::TruncatedHeader)?
            .try_into()
            .unwrap(),
    );
    pos += 4;

    // "cip compressed size" (8 bytes) — descartado.
    pos += 8;

    let compressed = data.get(pos..).ok_or(SheetError::TruncatedHeader)?;

    // --- LZMA1 raw ---
    let properties = lzma_rs::decompress::raw::LzmaProperties {
        lc: lclppb % 9,
        lp: (lclppb / 9) % 5,
        pb: (lclppb / 9) / 5,
    };
    let params = lzma_rs::decompress::raw::LzmaParams::new(properties, dict_size, None);

    let mut output = Vec::with_capacity(UNCOMPRESSED_SIZE);
    let mut decoder = lzma_rs::decompress::raw::LzmaDecoder::new(params, None)
        .map_err(SheetError::Lzma)?;
    let mut input = Cursor::new(compressed);
    decoder
        .decompress(&mut input, &mut output)
        .map_err(SheetError::Lzma)?;

    if output.len() < SHEET_BYTES + 11 {
        return Err(SheetError::TruncatedOutput);
    }

    // Pixels a partir do BMP header offset (u32 LE em +10).
    let pixel_offset = u32::from_le_bytes(output[10..14].try_into().unwrap()) as usize;
    let pixels = output
        .get(pixel_offset..pixel_offset + SHEET_BYTES)
        .ok_or(SheetError::TruncatedOutput)?;

    // Flip vertical (BMP é bottom-up; o RME faz o mesmo em loadSpriteSheet).
    let mut sheet = vec![0u8; SHEET_BYTES];
    const ROW: usize = (SHEET_SIZE as usize) * 4;
    for row in 0..SHEET_SIZE as usize {
        let src = &pixels[row * ROW..(row + 1) * ROW];
        let dst = &mut sheet[(SHEET_SIZE as usize - 1 - row) * ROW..][..ROW];
        dst.copy_from_slice(src);
    }

    Ok(sheet)
}