//! Porta em Rust de `reference-src/source/filehandle.{h,cpp}`.
//!
//! O formato "TreeNode" usado pelo OTBM (e outros arquivos do RME):
//!   - `0xFE` inicia um nó (o byte seguinte é o byte de tipo do nó);
//!   - `0xFF` fecha o nó corrente;
//!   - `0xFD` escapa o próximo byte (o payload pode conter qualquer byte,
//!     inclusive `0xFE`/`0xFF`/`0xFD`, sempre precedidos de `0xFD`).
//!
//! O payload de um nó vem ANTES dos filhos. O C++ faz leitura lazy
//! (`BinaryNode::advance()` + `load()`); aqui construímos a árvore num passe
//! recursivo, o que preserva exatamente a mesma semântica: o `data` de um nó
//! congela no primeiro filho (nós válidos nunca têm bytes soltos após filhos).

pub const NODE_START: u8 = 0xFE;
pub const NODE_END: u8 = 0xFF;
pub const ESCAPE_CHAR: u8 = 0xFD;

/// Um nó da árvore binária. `data` é o payload **nao-escaped** (começa pelo
/// byte de tipo do nó, igual ao `BinaryNode::data` do RME).
#[derive(Debug, Clone, Default)]
pub struct TreeNode {
    pub data: Vec<u8>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// Primeiro byte do payload = tipo do nó (`OTBM_*` no contexto do OTBM).
    pub fn node_type(&self) -> Option<u8> {
        self.data.first().copied()
    }

    /// Leitor sequencial sobre o payload deste nó (corresponde aos
    /// `getU8/getU16/getString/...` do `BinaryNode`).
    pub fn read(&self) -> BinaryCursor<'_> {
        BinaryCursor::new(&self.data)
    }
}

/// Leitor posicional sobre uma fatia (equivalente aos getters do `BinaryNode`).
/// Nada aqui valida tamanho além de `None` no fim dos dados.
#[derive(Debug, Clone)]
pub struct BinaryCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> BinaryCursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    pub fn is_exhausted(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    /// Little-endian, como o RME (`getU16`).
    pub fn read_u16(&mut self) -> Option<u16> {
        let b = self.read_u8()? as u16;
        let a = self.read_u8()? as u16;
        Some(a << 8 | b)
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        let b = self.read_u16()? as u32;
        let a = self.read_u16()? as u32;
        Some(a << 16 | b)
    }

    pub fn read_u64(&mut self) -> Option<u64> {
        let b = self.read_u32()? as u64;
        let a = self.read_u32()? as u64;
        Some(a << 32 | b)
    }

    /// String com prefixo **u16** (`getString`).
    pub fn read_string(&mut self) -> Option<String> {
        let len = self.read_u16()? as usize;
        self.read_raw(len).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// String com prefixo **u32** (`getLongString`).
    pub fn read_long_string(&mut self) -> Option<String> {
        let len = self.read_u32()? as usize;
        self.read_raw(len).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn read_raw(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.pos + len > self.bytes.len() {
            self.pos = self.bytes.len();
            return None;
        }
        let slice = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Some(slice)
    }

    /// Pula `n` bytes; falha (e zera o cursor) se não houver bytes suficientes —
    /// igual ao `skip` do `BinaryNode`.
    pub fn skip(&mut self, n: usize) -> bool {
        self.read_raw(n).is_some()
    }
}

/// Constrói a árvore de nós a partir de `bytes` (o payload inteiro do arquivo,
/// a partir do ponto onde o primeiro `NODE_START` já foi consumido).
pub fn build_tree(bytes: &[u8]) -> TreeNode {
    let mut cursor = BinaryCursor::new(bytes);
    read_node(&mut cursor)
}

fn read_node(cursor: &mut BinaryCursor) -> TreeNode {
    let mut node = TreeNode::default();
    let mut data_locked = false;
    loop {
        match cursor.read_u8() {
            Some(ESCAPE_CHAR) => {
                if let Some(b) = cursor.read_u8() {
                    if !data_locked {
                        node.data.push(b);
                    }
                } else {
                    break;
                }
            }
            Some(NODE_START) => {
                node.children.push(read_node(cursor));
                data_locked = true;
            }
            Some(NODE_END) | None => break,
            Some(b) => {
                if !data_locked {
                    node.data.push(b);
                }
            }
        }
    }
    node
}