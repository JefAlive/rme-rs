//! Porta de `reference-src/source/iomap_otbm.cpp` (bem como dos
//! `Item::Create_OTBM`/`ItemAttributes`), sem dependência externa.
//!
//! A saída é um documento cru (`OtbmDocument`): tiles + itens + towns,
//! sem ainda decidir *qual item é "chão"* — isso depende da tabela de itens
//! do `appearances.dat` e fica para a Fase 4.

use std::collections::HashSet;
use std::fmt;

use crate::binary::{build_tree, BinaryCursor, TreeNode, NODE_START};

pub const OTBM_ROOTV1: u8 = 1;
pub const OTBM_MAP_DATA: u8 = 2;
pub const OTBM_ITEM_DEF: u8 = 3;
pub const OTBM_TILE_AREA: u8 = 4;
pub const OTBM_TILE: u8 = 5;
pub const OTBM_ITEM: u8 = 6;
pub const OTBM_SPAWNS: u8 = 9;
pub const OTBM_TOWNS: u8 = 12;
pub const OTBM_TOWN: u8 = 13;
pub const OTBM_HOUSETILE: u8 = 14;
pub const OTBM_WAYPOINTS: u8 = 15;
pub const OTBM_WAYPOINT: u8 = 16;
pub const OTBM_TILE_ZONE: u8 = 19;

// OTBM_ItemAttribute (iomap_otbm.h)
pub const OTBM_ATTR_DESCRIPTION: u8 = 1;
pub const OTBM_ATTR_EXT_FILE: u8 = 2;
pub const OTBM_ATTR_TILE_FLAGS: u8 = 3;
pub const OTBM_ATTR_ACTION_ID: u8 = 4;
pub const OTBM_ATTR_UNIQUE_ID: u8 = 5;
pub const OTBM_ATTR_TEXT: u8 = 6;
pub const OTBM_ATTR_DESC: u8 = 7;
pub const OTBM_ATTR_TELE_DEST: u8 = 8;
pub const OTBM_ATTR_ITEM: u8 = 9;
pub const OTBM_ATTR_DEPOT_ID: u8 = 10;
pub const OTBM_ATTR_EXT_SPAWN_MONSTER_FILE: u8 = 11;
pub const OTBM_ATTR_RUNE_CHARGES: u8 = 12;
pub const OTBM_ATTR_EXT_HOUSE_FILE: u8 = 13;
pub const OTBM_ATTR_HOUSEDOORID: u8 = 14;
pub const OTBM_ATTR_COUNT: u8 = 15;
pub const OTBM_ATTR_CHARGES: u8 = 22;
pub const OTBM_ATTR_EXT_SPAWN_NPC_FILE: u8 = 23;
pub const OTBM_ATTR_EXT_ZONE_FILE: u8 = 24;
pub const OTBM_ATTR_ATTRIBUTE_MAP: u8 = 128;

// MapVersionID (client_assets.h) — o u32 que abre o payload da raiz.
pub const MAP_OTBM_1: u32 = 0;
pub const MAP_OTBM_2: u32 = 1;
pub const MAP_OTBM_3: u32 = 2;
pub const MAP_OTBM_4: u32 = 3;
pub const MAP_OTBM_5: u32 = 4;
pub const MAP_OTBM_6: u32 = 5;
pub const MAP_OTBM_LAST_VERSION: u32 = MAP_OTBM_6;

/// TILESTATE_* (tile.h): bits gravados em `OTBM_ATTR_TILE_FLAGS`.
pub const TILESTATE_PROTECTIONZONE: u32 = 0x0001;
pub const TILESTATE_NOPVP: u32 = 0x0004;
pub const TILESTATE_NOLOGOUT: u32 = 0x0008;
pub const TILESTATE_PVPZONE: u32 = 0x0010;
pub const TILESTATE_REFRESH: u32 = 0x0020;

// ItemAttribute::Type (item_attributes.h)
const ATTR_TYPE_STRING: u8 = 1;
const ATTR_TYPE_INTEGER: u8 = 2;
const ATTR_TYPE_FLOAT: u8 = 3;
const ATTR_TYPE_BOOLEAN: u8 = 4;
const ATTR_TYPE_DOUBLE: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OtmPos {
    pub x: u16,
    pub y: u16,
    pub z: u8,
}

#[derive(Debug, Clone, Default)]
pub struct OtmItem {
    pub id: u16,
    /// "subtype" do RME: empilhamento, fluido, splash, charges — sobrescrito
    /// a cada COUNT/CHARGES/RUNE_CHARGES lido (igual ao `setSubtype`).
    pub subtype: u16,
    pub action_id: u16,
    pub unique_id: u16,
    pub text: Option<String>,
    pub desc: Option<String>,
    pub tele_dest: Option<OtmPos>,
    pub depot_id: u16,
    pub door_id: u8,
    pub container: Vec<OtmItem>,
    pub attribute_map: Vec<(String, String)>,
}

impl OtmItem {
    fn new(id: u16) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OtmTile {
    pub x: u16,
    pub y: u16,
    pub z: u8,
    pub house_id: u32,
    pub flags: u32,
    pub items: Vec<OtmItem>,
}

#[derive(Debug, Clone)]
pub struct OtmTown {
    pub id: u32,
    pub name: String,
    pub temple: OtmPos,
}

#[derive(Debug, Clone)]
pub struct OtmWaypoint {
    pub name: String,
    pub pos: OtmPos,
}

#[derive(Debug, Default)]
pub struct OtbmDocument {
    pub version: u32,
    pub width: u16,
    pub height: u16,
    pub description: String,
    pub spawn_monster_file: Option<String>,
    pub house_file: Option<String>,
    pub zone_file: Option<String>,
    pub spawn_npc_file: Option<String>,
    pub tiles: Vec<OtmTile>,
    pub towns: Vec<OtmTown>,
    pub waypoints: Vec<OtmWaypoint>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum OtbmError {
    TooSmall,
    BadMagic,
    BadRootNode,
    MissingMapData,
    Truncated(&'static str),
}

impl fmt::Display for OtbmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OtbmError::TooSmall => write!(f, "arquivo menor que o cabeçalho de 5 bytes"),
            OtbmError::BadMagic => write!(f, "magic não reconhecido (esperado \"OTBM\" ou wildcard)"),
            OtbmError::BadRootNode => write!(f, "não há nó raiz (byte 4 não é NODE_START)"),
            OtbmError::MissingMapData => write!(f, "nada o nó OTBM_MAP_DATA na raiz"),
            OtbmError::Truncated(what) => write!(f, "arquivo truncado ao ler {what}"),
        }
    }
}

impl std::error::Error for OtbmError {}

/// Parseia um arquivo `.otbm` (buffer inteiro). `is_subtype_embedded` decide,
/// apenas para OTBM v1, se um item traz o count logo após o id
/// (stackable/splash/fluid) — depende da tabela de itens, então fica a cargo
/// do chamador; para mapas modernos (>= OTBM v2) é ignorado.
pub fn parse(data: &[u8], is_subtype_embedded: impl Fn(u16) -> bool) -> Result<OtbmDocument, OtbmError> {
    if data.len() < 5 {
        return Err(OtbmError::TooSmall);
    }
    let wildcard = data[..4] == [0; 4];
    if !(wildcard || &data[..4] == b"OTBM") {
        return Err(OtbmError::BadMagic);
    }
    if data[4] != NODE_START {
        return Err(OtbmError::BadRootNode);
    }

    let root = build_tree(&data[5..]);
    let mut load = OtbmLoad {
        version: 0,
        is_subtype_embedded: &is_subtype_embedded,
        warnings: Vec::new(),
    };

    let mut doc = OtbmDocument::default();

    {
        let mut rc = root.read();
        rc.skip(1); // tipo da raiz (OTBM_ROOTV1)
        let version = rc.read_u32().ok_or(OtbmError::Truncated("versão da raiz"))?;
        load.version = version;
        doc.version = version;
        if version > MAP_OTBM_LAST_VERSION {
            load.warn(format!("OTBM versão {version} não suportada — tentando continuar",));
        }
        doc.width = rc.read_u16().ok_or(OtbmError::Truncated("largura do mapa"))?;
        doc.height = rc.read_u16().ok_or(OtbmError::Truncated("altura do mapa"))?;
    }

    let mut seen_map_data = false;
    for child in &root.children {
        match child.node_type() {
            Some(OTBM_MAP_DATA) => {
                seen_map_data = true;
                load_map_data(child, &mut load, &mut doc);
            }
            Some(other) => load.warn(format!("nó raiz ignorado (tipo {other})")),
            None => load.warn("nó raiz sem byte de tipo".to_string()),
        }
    }
    if !seen_map_data {
        return Err(OtbmError::MissingMapData);
    }

    doc.warnings = std::mem::take(&mut load.warnings);
    Ok(doc)
}

struct OtbmLoad<'a> {
    version: u32,
    is_subtype_embedded: &'a dyn Fn(u16) -> bool,
    warnings: Vec<String>,
}

impl OtbmLoad<'_> {
    fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }
}

fn load_map_data(node: &TreeNode, load: &mut OtbmLoad, doc: &mut OtbmDocument) {
    let mut c = node.read();
    c.skip(1); // byte de tipo (OTBM_MAP_DATA)

    while let Some(attr) = c.read_u8() {
        match attr {
            OTBM_ATTR_DESCRIPTION => match c.read_string() {
                Some(s) => doc.description = s,
                None => load.warn("descrição do mapa inválida"),
            },
            OTBM_ATTR_EXT_SPAWN_MONSTER_FILE => doc.spawn_monster_file = c.read_string(),
            OTBM_ATTR_EXT_HOUSE_FILE => doc.house_file = c.read_string(),
            OTBM_ATTR_EXT_ZONE_FILE => doc.zone_file = c.read_string(),
            OTBM_ATTR_EXT_SPAWN_NPC_FILE => doc.spawn_npc_file = c.read_string(),
            _ => load.warn(format!("atributo de cabeçalho desconhecido ({attr})")),
        }
    }

    let mut seen_positions = HashSet::new();
    for child in &node.children {
        let Some(t) = child.node_type() else {
            load.warn("nó filho do MAP_DATA sem tipo");
            continue;
        };
        match t {
            OTBM_TILE_AREA => load_tile_area(child, &mut seen_positions, load, doc),
            OTBM_TOWNS => load_towns(child, load, doc),
            OTBM_WAYPOINTS => load_waypoints(child, load, doc),
            OTBM_ITEM_DEF => load.warn("OTBM_ITEM_DEF ignorado (definições globais de item)".to_string()),
            OTBM_SPAWNS => load.warn("spawns embarcados ignorados (o arquivo usa XML separado)".to_string()),
            other => load.warn(format!("tipo de nó de mapa desconhecido ({other})")),
        }
    }
}

fn load_tile_area(node: &TreeNode, seen: &mut HashSet<OtmPos>, load: &mut OtbmLoad, doc: &mut OtbmDocument) {
    let mut c = node.read();
    c.skip(1); // byte de tipo (OTBM_TILE_AREA), consumido pelo RME via getByte
    let (Some(base_x), Some(base_y), Some(base_z)) = (c.read_u16(), c.read_u16(), c.read_u8()) else {
        load.warn("TILE_AREA sem coordenada base");
        return;
    };

    for child in &node.children {
        let Some(t) = child.node_type() else {
            load.warn("nó filho da área sem tipo");
            continue;
        };
        if t != OTBM_TILE && t != OTBM_HOUSETILE {
            load.warn(format!("tipo de nó de tile desconhecido ({t})"));
            continue;
        }

        let mut tc = child.read();
        tc.skip(1); // byte de tipo (OTBM_TILE / OTBM_HOUSETILE), consumido via getByte
        let (Some(x_off), Some(y_off)) = (tc.read_u8(), tc.read_u8()) else {
            load.warn("tile sem posição na área");
            continue;
        };

        let pos = OtmPos { x: base_x + x_off as u16, y: base_y + y_off as u16, z: base_z };

        let mut house_id = 0u32;
        if t == OTBM_HOUSETILE {
            match tc.read_u32() {
                Some(0) => load.warn(format!("house id inválido em {pos:?}")),
                Some(id) => house_id = id,
                None => {
                    load.warn(format!("house tile sem house data em {pos:?}"));
                    continue;
                }
            }
        }

        if !seen.insert(pos) {
            load.warn(format!("tile duplicado em {pos:?}, descartado"));
            continue;
        }

        let mut tile = OtmTile { x: pos.x, y: pos.y, z: pos.z, house_id, ..Default::default() };

        while let Some(attr) = tc.read_u8() {
            match attr {
                OTBM_ATTR_TILE_FLAGS => match tc.read_u32() {
                    Some(flags) => tile.flags = flags,
                    None => load.warn(format!("tile flags inválidas em {pos:?}")),
                },
                OTBM_ATTR_ITEM => {
                    // Legado (item embutido no payload da tile) — raro.
                    let Some(id) = tc.read_u16() else {
                        load.warn("item legado sem id em {pos:?}");
                        break;
                    };
                    let mut item = OtmItem::new(id);
                    if load.version == MAP_OTBM_1 && (load.is_subtype_embedded)(id) {
                        item.subtype = tc.read_u8().unwrap_or(0) as u16;
                    }
                    tile.items.push(item);
                }
                other => load.warn(format!("atributo de tile desconhecido ({other}) em {pos:?}")),
            }
        }

        for cc in &child.children {
            let Some(ct) = cc.node_type() else {
                load.warn("filho da tile sem tipo");
                continue;
            };
            match ct {
                OTBM_ITEM => tile.items.push(read_item(cc, load)),
                OTBM_TILE_ZONE => read_tile_zone(cc, load, &mut tile),
                other => load.warn(format!("tipo de filho de tile desconhecido ({other}) em {pos:?}")),
            }
        }

        doc.tiles.push(tile);
    }
}

fn read_tile_zone(node: &TreeNode, load: &mut OtbmLoad, _tile: &mut OtmTile) {
    let mut c = node.read();
    let Some(count) = c.read_u16() else {
        load.warn("contagem de zonas inválida");
        return;
    };
    for _ in 0..count {
        if c.read_u16().is_none() {
            load.warn("id de zona inválido");
            return;
        }
    }
}

fn read_item(node: &TreeNode, load: &mut OtbmLoad) -> OtmItem {
    let mut c = node.read();
    c.skip(1); // byte de tipo (OTBM_ITEM)
    let Some(id) = c.read_u16() else {
        load.warn("item sem id");
        return OtmItem::default();
    };
    let mut item = OtmItem::new(id);

    // Apenas OTBM v1 embute o count no corpo do nó (depende da tabela de itens).
    if load.version == MAP_OTBM_1 && (load.is_subtype_embedded)(id) {
        item.subtype = c.read_u8().unwrap_or(0) as u16;
    }

    while let Some(attr) = c.read_u8() {
        if !read_item_attr(attr, &mut c, &mut item, load) {
            load.warn(format!("item {id}: atributo desconhecido ({attr}), atributos restantes ignorados"));
            break;
        }
    }

    // Filhos: conteúdo de containers.
    for child in &node.children {
        if child.node_type() == Some(OTBM_ITEM) {
            item.container.push(read_item(child, load));
        } else {
            load.warn(format!("item {id}: filho que não é ITEM ignorado"));
        }
    }
    item
}

/// Retorna `false` para atributo desconhecido — espelha o `readItemAttribute_OTBM`
/// que aborta a leitura de atributos do item.
fn read_item_attr(attr: u8, c: &mut BinaryCursor, item: &mut OtmItem, load: &mut OtbmLoad) -> bool {
    match attr {
        OTBM_ATTR_COUNT => {
            let Some(v) = c.read_u8() else { return false };
            item.subtype = u16::from(v);
        }
        OTBM_ATTR_RUNE_CHARGES => {
            let Some(v) = c.read_u8() else { return false };
            item.subtype = u16::from(v);
        }
        OTBM_ATTR_CHARGES => {
            let Some(v) = c.read_u16() else { return false };
            item.subtype = v;
        }
        OTBM_ATTR_ACTION_ID => {
            let Some(v) = c.read_u16() else { return false };
            item.action_id = v;
        }
        OTBM_ATTR_UNIQUE_ID => {
            let Some(v) = c.read_u16() else { return false };
            item.unique_id = v;
        }
        OTBM_ATTR_TEXT => {
            let Some(v) = c.read_string() else { return false };
            item.text = Some(v);
        }
        OTBM_ATTR_DESC => {
            let Some(v) = c.read_string() else { return false };
            item.desc = Some(v);
        }
        OTBM_ATTR_DEPOT_ID => {
            let Some(v) = c.read_u16() else { return false };
            item.depot_id = v;
        }
        OTBM_ATTR_HOUSEDOORID => {
            let Some(v) = c.read_u8() else { return false };
            item.door_id = v;
        }
        OTBM_ATTR_TELE_DEST => {
            let (Some(x), Some(y), Some(z)) = (c.read_u16(), c.read_u16(), c.read_u8()) else {
                return false;
            };
            item.tele_dest = Some(OtmPos { x, y, z });
        }
        OTBM_ATTR_ATTRIBUTE_MAP => return read_attr_map(c, item, load),
        // Atributo conhecido, mas fora do esperado para itens simples:
        _ => return false,
    }
    true
}

fn read_attr_map(c: &mut BinaryCursor, item: &mut OtmItem, load: &mut OtbmLoad) -> bool {
    let Some(n) = c.read_u16() else {
        return false;
    };
    for _ in 0..n {
        let Some(key) = c.read_string() else {
            return false;
        };
        let Some(itype) = c.read_u8() else {
            return false;
        };
        let value = match itype {
            ATTR_TYPE_STRING => match c.read_long_string() {
                Some(s) => s,
                None => return false,
            },
            ATTR_TYPE_INTEGER => match c.read_u32() {
                Some(v) => v.to_string(),
                None => return false,
            },
            ATTR_TYPE_FLOAT => match c.read_u32() {
                Some(bits) => f32::from_bits(bits).to_string(),
                None => return false,
            },
            ATTR_TYPE_DOUBLE => match c.read_u64() {
                Some(bits) => f64::from_bits(bits).to_string(),
                None => return false,
            },
            ATTR_TYPE_BOOLEAN => match c.read_u8() {
                Some(b) => (b != 0).to_string(),
                None => return false,
            },
            other => {
                load.warn(format!("attribute map: tipo de valor desconhecido ({other})"));
                return false;
            }
        };
        item.attribute_map.push((key, value));
    }
    true
}

fn load_towns(node: &TreeNode, load: &mut OtbmLoad, doc: &mut OtbmDocument) {
    let mut seen = HashSet::new();
    for child in &node.children {
        if child.node_type() != Some(OTBM_TOWN) {
            load.warn("nó de town inválido");
            continue;
        }
        let mut c = child.read();
        c.skip(1); // byte de tipo (OTBM_TOWN)
        let (Some(id), Some(name)) = (c.read_u32(), c.read_string()) else {
            load.warn("town incompleto");
            continue;
        };
        if !seen.insert(id) {
            load.warn(format!("town id duplicado {id}, descartado"));
            continue;
        }
        let (Some(x), Some(y), Some(z)) = (c.read_u16(), c.read_u16(), c.read_u8()) else {
            load.warn(format!("town {id}: posição do templo inválida"));
            continue;
        };
        doc.towns.push(OtmTown { id, name, temple: OtmPos { x, y, z } });
    }
}

fn load_waypoints(node: &TreeNode, load: &mut OtbmLoad, doc: &mut OtbmDocument) {
    for child in &node.children {
        if child.node_type() != Some(OTBM_WAYPOINT) {
            load.warn("nó de waypoint inválido");
            continue;
        }
        let mut c = child.read();
        c.skip(1); // byte de tipo (OTBM_WAYPOINT)
        let (Some(name), Some(x), Some(y), Some(z)) =
            (c.read_string(), c.read_u16(), c.read_u16(), c.read_u8())
        else {
            load.warn("waypoint incompleto");
            continue;
        };
        doc.waypoints.push(OtmWaypoint { name, pos: OtmPos { x, y, z } });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::NODE_END;

    /// Escritor mínimo para montar buffers no formato TreeNode (espelha o
    /// `serializeItemNode_OTBM`/`NodeFileWriteHandle` do RME).
    struct W {
        bytes: Vec<u8>,
    }

    impl W {
        fn escaped(b: u8) -> [u8; 2] {
            if matches!(b, NODE_START | crate::binary::NODE_END | crate::binary::ESCAPE_CHAR) {
                [crate::binary::ESCAPE_CHAR, b]
            } else {
                [b, 0]
            }
        }

        fn start_node(&mut self, ty: u8) {
            self.bytes.push(NODE_START);
            self.bytes.push(ty);
        }
        /// A raiz: o NODE_START de índice 4 já foi consumido pelo getRootNode,
        /// então seu payload começa direto no byte de tipo.
        fn root(&mut self, ty: u8) {
            self.bytes.push(ty);
        }
        fn end_node(&mut self) {
            self.bytes.push(NODE_END);
        }
        fn u8(&mut self, b: u8) {
            self.bytes.push(b);
        }
        fn u16(&mut self, v: u16) {
            self.bytes.extend_from_slice(&v.to_le_bytes());
        }
        fn u32(&mut self, v: u32) {
            self.bytes.extend_from_slice(&v.to_le_bytes());
        }
        fn string(&mut self, s: &str) {
            self.raw_string(s.as_bytes());
        }
        /// Escreve bytes crus (possivelmente não-UTF8) com prefixo u16 e escaping.
        fn raw_string(&mut self, bytes: &[u8]) {
            self.u16(bytes.len() as u16);
            for &b in bytes {
                let e = Self::escaped(b);
                self.bytes.push(e[0]);
                if e[1] != 0 {
                    self.bytes.push(e[1]);
                }
            }
        }
    }

    fn doc(bytes: &[u8]) -> OtbmDocument {
        parse(bytes, |_| false).expect("parse deve ter sucesso")
    }

    #[test]
    fn parses_escaped_tokens_and_nesting() {
        let mut w = W { bytes: Vec::new() };
        w.bytes.extend_from_slice(&[0, 0, 0, 0, NODE_START]); // magic wildcard + NODE_START
        w.root(OTBM_ROOTV1);
        w.u32(MAP_OTBM_4);
        w.u16(100);
        w.u16(200);
        w.start_node(OTBM_MAP_DATA);
        w.u8(OTBM_ATTR_DESCRIPTION);
        w.string("chão com caçamba");
        w.start_node(OTBM_TILE_AREA);
        w.u16(1000);
        w.u16(2000);
        w.u8(7);
        w.start_node(OTBM_TILE);
        w.u8(1);
        w.u8(2);
        w.u8(OTBM_ATTR_TILE_FLAGS);
        w.u32(TILESTATE_PROTECTIONZONE);
        w.start_node(OTBM_ITEM);
        w.u16(3040);
        w.u8(OTBM_ATTR_TEXT);
        w.raw_string(&[0xFE, 0xFF, 0xFD, b' ', b'x']);
        w.u8(OTBM_ATTR_COUNT);
        w.u8(5);
        w.end_node(); // item
        w.end_node(); // tile
        w.end_node(); // area
        w.end_node(); // map_data
        w.end_node(); // root

        let d = doc(&w.bytes);
        assert_eq!(d.version, MAP_OTBM_4);
        assert_eq!((d.width, d.height), (100, 200));
        assert_eq!(d.description, "chão com caçamba");
        assert_eq!(d.tiles.len(), 1);
        let tile = &d.tiles[0];
        assert_eq!((tile.x, tile.y, tile.z), (1001, 2002, 7));
        assert_eq!(tile.flags, TILESTATE_PROTECTIONZONE);
        assert_eq!(tile.items.len(), 1);
        let item = &tile.items[0];
        assert_eq!(item.id, 3040);
        assert_eq!(item.subtype, 5);
        assert_eq!(item.text.as_deref(), Some("\u{fffd}\u{fffd}\u{fffd} x"));
        assert!(d.warnings.is_empty(), "warnings: {:?}", d.warnings);
    }

    #[test]
    fn parses_container_and_town() {
        let mut w = W { bytes: Vec::new() };
        w.bytes.extend_from_slice(&[0, 0, 0, 0, NODE_START]);
        w.root(OTBM_ROOTV1);
        w.u32(MAP_OTBM_2);
        w.u16(10);
        w.u16(10);
        w.start_node(OTBM_MAP_DATA);
        w.start_node(OTBM_TILE_AREA);
        w.u16(0);
        w.u16(0);
        w.u8(7);
        w.start_node(OTBM_TILE);
        w.u8(0);
        w.u8(0);
        w.start_node(OTBM_ITEM); // container
        w.u16(1987);
        w.start_node(OTBM_ITEM); // item dentro
        w.u16(4600);
        w.u8(OTBM_ATTR_COUNT);
        w.u8(1);
        w.end_node();
        w.end_node(); // container
        w.end_node(); // tile
        w.end_node(); // area
        w.start_node(OTBM_TOWNS);
        w.start_node(OTBM_TOWN);
        w.u32(1);
        w.string("Dawnport");
        w.u16(32064);
        w.u16(31892);
        w.u8(6);
        w.end_node();
        w.end_node();
        w.end_node(); // map_data
        w.end_node(); // root

        let d = doc(&w.bytes);
        assert_eq!(d.tiles[0].items[0].id, 1987);
        assert_eq!(d.tiles[0].items[0].container.len(), 1);
        assert_eq!(d.tiles[0].items[0].container[0].id, 4600);
        assert_eq!(d.towns.len(), 1);
        assert_eq!(d.towns[0].name, "Dawnport");
        assert_eq!(d.towns[0].temple, OtmPos { x: 32064, y: 31892, z: 6 });
    }

    #[test]
    fn rejects_bad_header() {
        assert!(matches!(parse(&[0, 0, 0, 0, 0xFE], |_| false), Err(OtbmError::Truncated(_))));
        assert!(matches!(parse(&[0, 0, 0, 0], |_| false), Err(OtbmError::TooSmall)));
        assert!(matches!(parse(b"NOTOTBM!!!", |_| false), Err(OtbmError::BadMagic)));
        let mut short = vec![0u8; 5];
        short[4] = NODE_START;
        assert!(matches!(parse(&short, |_| false), Err(OtbmError::Truncated(_))));
    }
}