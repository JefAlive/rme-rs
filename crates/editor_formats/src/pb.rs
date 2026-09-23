//! Decodificador minimalista do protobuf wire format (sem dependências).
//!
//! Porta apenas o necessário para ler o `appearances.dat` do cliente,
//! seguindo `reference-src/source/protobuf/appearances.proto` com a mesma
//! semântica do protobuf-c gerado pelo canary (campos `optional`/`repeated`,
//! presença de campo, encadeamento de payload de sub-mensagens).

pub const WT_VARINT: u8 = 0;
pub const WT_FIXED64: u8 = 1;
pub const WT_LEN: u8 = 2;
pub const WT_FIXED32: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag(pub u32);

impl Tag {
    pub fn field(self) -> u32 {
        self.0 >> 3
    }
    pub fn wire_type(self) -> u8 {
        (self.0 & 0x7) as u8
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub fn is_done(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub fn varint(&mut self) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = *self.buf.get(self.pos)?;
            self.pos += 1;
            if shift == 63 && byte > 1 {
                return None;
            }
            result |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }

    pub fn tag(&mut self) -> Option<Tag> {
        let raw = self.varint()?;
        if raw > u32::MAX as u64 {
            return None;
        }
        Some(Tag(raw as u32))
    }

    pub fn bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let start = self.pos;
        let end = self.pos.checked_add(len)?;
        let slice = self.buf.get(start..end)?;
        self.pos = end;
        Some(slice)
    }

    pub fn len_payload(&mut self) -> Option<&'a [u8]> {
        let len = self.varint()?;
        if len > usize::MAX as u64 {
            return None;
        }
        self.bytes(len as usize)
    }

    pub fn skip(&mut self, wire_type: u8) -> Option<()> {
        match wire_type {
            WT_VARINT => {
                self.varint()?;
                Some(())
            }
            WT_FIXED64 => {
                self.bytes(8)?;
                Some(())
            }
            WT_LEN => {
                self.len_payload()?;
                Some(())
            }
            WT_FIXED32 => {
                self.bytes(4)?;
                Some(())
            }
            _ => None,
        }
    }
}

/// Decodifica um stream de bytes como payload de uma sub-mensagem protobuf,
/// executando `f` (que lê da copia do reader) para cada campo. Campos com
/// número de campo desconhecido são pulados. Retorna `Ok(())` se o trailing
/// decodificou até o fim sem truncar. Usado como alternativa a `parse_*`
/// explícitas quando o consumidor só quer iterar campos conhecidos.
pub fn for_each_field<F>(payload: &[u8], mut f: F) -> Option<()>
where
    F: FnMut(Reader<'_>, u32),
{
    let mut r = Reader::new(payload);
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        f(r, tag.field());
        r.skip(wt)?;
    }
    Some(())
}