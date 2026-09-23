//! Parser hand-rolled do `appearances.dat` do cliente Tibia (formato
//! protobuf), convertendo para a tabela `ItemType`.
//!
//! Fonte da verdade: `reference-src/source/{client_assets.cpp,
//! sprite_appearances.cpp, items.cpp, graphics.cpp}` + `protobuf/appearances.proto`.
//! O arquivo é protobuf puro (sem LZMA), parseado direto com `ParseFromIstream`.

use crate::pb::{Reader, WT_LEN, WT_VARINT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemGroup {
    #[default]
    None_,
    Ground,
    Container,
    Fluid,
    Splash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemHook {
    #[default]
    None_,
    South,
    East,
}

/// `GameSprite` metametadata do `items.cpp`/`graphics.cpp` que o renderer
/// precisa (draw_height, draw_offset, luz, minimap) — sem depender de GPU.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpriteMeta {
    pub draw_height: u16,
    pub draw_offset: (i32, i32),
    pub minimap_color: u16,
    pub has_light: bool,
    pub light_color: u32,
    pub light_intensity: u32,
}

#[derive(Debug, Clone)]
pub struct ItemType {
    pub id: u16,
    pub client_id: u16,
    pub name: String,
    pub description: String,
    pub group: ItemGroup,
    /// flag `container` → `type = ITEM_TYPE_CONTAINER`.
    pub is_container: bool,
    pub always_on_bottom: bool,
    pub always_on_top_order: u8,
    pub pattern_width: u32,
    pub pattern_height: u32,
    pub pattern_depth: u32,
    pub layers: u32,
    /// `spriteInfo.sprite_id(0)` — sprite principal.
    pub sprite_id: u32,
    /// lista achatada de sprite ids em ordem de arquivo (padrões+animação).
    pub sprite_ids: Vec<u32>,
    pub start_frame: i8,
    pub loop_count: u32,
    pub async_animation: bool,
    pub animation_phases: Vec<(u32, u32)>,
    pub no_move_animation: bool,
    pub is_corpse: bool,
    pub force_use: bool,
    pub has_height: bool,
    pub unpassable: bool,
    pub block_missiles: bool,
    pub block_pathfinder: bool,
    pub pickupable: bool,
    pub moveable: bool,
    pub can_read_text: bool,
    pub is_hangable: bool,
    pub stackable: bool,
    pub is_podium: bool,
    pub rotable: bool,
    pub ignore_look: bool,
    pub has_elevation: bool,
    pub hook: ItemHook,
    pub sprite: SpriteMeta,
}

impl Default for ItemType {
    fn default() -> Self {
        ItemType {
            id: 0,
            client_id: 0,
            name: String::new(),
            description: String::new(),
            group: ItemGroup::default(),
            is_container: false,
            always_on_bottom: false,
            always_on_top_order: 0,
            pattern_width: 0,
            pattern_height: 0,
            pattern_depth: 0,
            layers: 0,
            sprite_id: 0,
            sprite_ids: Vec::new(),
            start_frame: 0,
            loop_count: 0,
            async_animation: false,
            animation_phases: Vec::new(),
            no_move_animation: false,
            is_corpse: false,
            force_use: false,
            has_height: false,
            unpassable: false,
            block_missiles: false,
            block_pathfinder: false,
            pickupable: false,
            moveable: true,
            can_read_text: false,
            is_hangable: false,
            stackable: false,
            is_podium: false,
            rotable: false,
            ignore_look: false,
            has_elevation: false,
            hook: ItemHook::default(),
            sprite: SpriteMeta {
                draw_height: 0,
                draw_offset: (0, 0),
                minimap_color: 0,
                has_light: false,
                light_color: 0,
                light_intensity: 0,
            },
        }
    }
}

impl ItemType {
    /// Número de sprites que o `GameSprite` espera
    /// (`graphics.cpp`: layers * pattern_x * pattern_y * pattern_z *
    ///  max(1, sprite_phase_size)).
    pub fn numsprites(&self) -> usize {
        (self.layers as usize)
            * (self.pattern_width as usize)
            * (self.pattern_height as usize)
            * (self.pattern_depth as usize)
            * self.animation_phases.len().max(1)
    }

    /// Índice dentro de `sprite_ids` para um padrão, idêntico à ordem do RME
    /// em `blitItem`/`Tile::getIndex` (frame→z→y→x→layer).
    pub fn sprite_index(&self, phase: u32, px: u32, py: u32, pz: u32, layer: u32) -> usize {
        let frame = (phase as usize) % self.animation_phases.len().max(1);
        let fx = self.pattern_width.max(1) as usize;
        let fy = self.pattern_height.max(1) as usize;
        let fz = self.pattern_depth.max(1) as usize;
        let layers = self.layers.max(1) as usize;
        let xi = px as usize % fx;
        let yi = py as usize % fy;
        let zi = pz as usize % fz;
        ((((frame * fz) + zi) * fy + yi) * fx + xi) * layers + ((layer as usize) % layers)
    }

    pub fn is_ground_tile(&self) -> bool {
        self.group == ItemGroup::Ground
    }

    pub fn draw_height(&self) -> u16 {
        self.sprite.draw_height
    }

    pub fn draw_offset(&self) -> (i32, i32) {
        self.sprite.draw_offset
    }

    pub fn has_light(&self) -> bool {
        self.sprite.has_light
    }
}

/// Tabela de itens indexada por `id` (ItemDatabase).
#[derive(Debug, Clone, Default)]
pub struct ItemTypeTable {
    pub items: Vec<Option<ItemType>>,
    pub max_item_id: u16,
}

impl ItemTypeTable {
    pub fn get_opt(&self, id: u16) -> Option<&ItemType> {
        self.items.get(id as usize).and_then(|t| t.as_ref())
    }

    /// Semântica do `ItemDatabase::getItemType` original (devolve um ItemType vazio).
    pub fn get(&self, id: u16) -> &ItemType {
        self.get_opt(id).unwrap_or(&DEFAULT_ITEM_TYPE)
    }

    pub fn is_valid(&self, id: u16) -> bool {
        self.get_opt(id).is_some()
    }
}

/// ItemType vazio global usado por `get()` para ids desconhecidos.
static DEFAULT_ITEM_TYPE: ItemType = ItemType {
    id: 0,
    client_id: 0,
    name: String::new(),
    description: String::new(),
    group: ItemGroup::None_,
    is_container: false,
    always_on_bottom: false,
    always_on_top_order: 0,
    pattern_width: 0,
    pattern_height: 0,
    pattern_depth: 0,
    layers: 0,
    sprite_id: 0,
    sprite_ids: Vec::new(),
    start_frame: 0,
    loop_count: 0,
    async_animation: false,
    animation_phases: Vec::new(),
    no_move_animation: false,
    is_corpse: false,
    force_use: false,
    has_height: false,
    unpassable: false,
    block_missiles: false,
    block_pathfinder: false,
    pickupable: false,
    moveable: true,
    can_read_text: false,
    is_hangable: false,
    stackable: false,
    is_podium: false,
    rotable: false,
    ignore_look: false,
    has_elevation: false,
    hook: ItemHook::None_,
    sprite: SpriteMeta {
        draw_height: 0,
        draw_offset: (0, 0),
        minimap_color: 0,
        has_light: false,
        light_color: 0,
        light_intensity: 0,
    },
};

// ---------------------------------------------------------------------------
// Mensagens protobuf (subset do appearances.proto usado pelo carregador)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Appearances {
    pub objects: Vec<Appearance>,
}

#[derive(Debug, Clone, Default)]
pub struct Appearance {
    pub id: Option<u32>,
    pub frame_groups: Vec<FrameGroup>,
    pub flags: Option<AppearanceFlags>,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Default)]
pub struct FrameGroup {
    pub fixed_frame_group: Option<u32>,
    pub sprite_info: Option<SpriteInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct SpriteInfo {
    pub pattern_width: u32,
    pub pattern_height: u32,
    pub pattern_depth: u32,
    pub layers: u32,
    pub sprite_ids: Vec<u32>,
    pub animation: Option<SpriteAnimation>,
}

#[derive(Debug, Clone, Default)]
pub struct SpriteAnimation {
    pub default_start_phase: u32,
    pub synchronized: bool,
    pub loop_count: u32,
    pub sprite_phases: Vec<(u32, u32)>,
}

/// Campo opcional bool proto2: `Some(v)` = presente, valor v; `None` = ausente.
/// Métodos `has_*()` em protoc geram `presente`; `value()` geram `presente && v`.
#[derive(Debug, Clone, Default)]
pub struct AppearanceFlags {
    pub bank: Option<()>,
    pub clip: Option<bool>,
    pub top: Option<bool>,
    pub bottom: Option<bool>,
    pub container: Option<bool>,
    pub cumulative: Option<bool>,
    pub take: Option<bool>,
    pub unmove: Option<bool>,
    pub liquidcontainer: Option<bool>,
    pub liquidpool: Option<bool>,
    pub no_movement_animation: Option<bool>,
    pub forceuse: Option<bool>,
    pub unpass: Option<bool>,
    pub unsight: Option<bool>,
    pub avoid: Option<bool>,
    pub hang: Option<bool>,
    pub rotate: Option<bool>,
    pub ignore_look: Option<bool>,
    pub show_off_socket: Option<bool>,
    pub corpse: Option<bool>,
    pub player_corpse: Option<bool>,
    pub height: Option<u32>,
    pub shift: Option<(u32, u32)>,
    pub light: Option<(u32, u32)>,
    pub hook: Option<u32>,
    pub automap: Option<u32>,
    pub lenshelp: Option<()>,
    pub write: Option<()>,
    pub write_once: Option<()>,
}

impl AppearanceFlags {
    pub fn has_clip(&self) -> bool {
        self.clip.is_some()
    }
    pub fn has_top(&self) -> bool {
        self.top.is_some()
    }
    pub fn has_bottom(&self) -> bool {
        self.bottom.is_some()
    }
    pub fn has_bank(&self) -> bool {
        self.bank.is_some()
    }
    pub fn has_height(&self) -> bool {
        self.height.is_some()
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn parse(b: &[u8]) -> Option<Appearances> {
    let mut out = Appearances::default();
    let mut r = Reader::new(b);
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        match tag.field() {
            1 => {
                let payload = r.len_payload()?;
                out.objects.push(parse_appearance(payload)?);
            }
            _ => r.skip(wt)?,
        }
    }
    Some(out)
}

fn parse_appearance(payload: &[u8]) -> Option<Appearance> {
    let mut out = Appearance::default();
    let mut r = Reader::new(payload);
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        match tag.field() {
            1 => out.id = Some(r.varint()? as u32),
            2 => out.frame_groups.push(parse_frame_group(r.len_payload()?)?),
            3 => out.flags = Some(parse_flags(r.len_payload()?)?),
            4 => out.name = String::from_utf8_lossy(r.len_payload()?).into_owned(),
            5 => out.description = String::from_utf8_lossy(r.len_payload()?).into_owned(),
            _ => r.skip(wt)?,
        }
    }
    Some(out)
}

fn parse_frame_group(payload: &[u8]) -> Option<FrameGroup> {
    let mut out = FrameGroup::default();
    let mut r = Reader::new(payload);
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        match tag.field() {
            1 => out.fixed_frame_group = Some(r.varint()? as u32),
            3 => out.sprite_info = Some(parse_sprite_info(r.len_payload()?)?),
            _ => r.skip(wt)?,
        }
    }
    Some(out)
}

fn parse_sprite_info(payload: &[u8]) -> Option<SpriteInfo> {
    let mut out = SpriteInfo::default();
    let mut r = Reader::new(payload);
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        match tag.field() {
            1 => out.pattern_width = r.varint()? as u32,
            2 => out.pattern_height = r.varint()? as u32,
            3 => out.pattern_depth = r.varint()? as u32,
            4 => out.layers = r.varint()? as u32,
            5 => match wt {
                WT_VARINT => out.sprite_ids.push(r.varint()? as u32),
                WT_LEN => {
                    let pay = r.len_payload()?;
                    let mut sr = Reader::new(pay);
                    while !sr.is_done() {
                        out.sprite_ids.push(sr.varint()? as u32);
                    }
                }
                _ => return None,
            },
            6 => out.animation = Some(parse_animation(r.len_payload()?)?),
            _ => r.skip(wt)?,
        }
    }
    Some(out)
}

fn parse_animation(payload: &[u8]) -> Option<SpriteAnimation> {
    let mut out = SpriteAnimation::default();
    let mut r = Reader::new(payload);
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        match tag.field() {
            1 => out.default_start_phase = r.varint()? as u32,
            2 => out.synchronized = r.varint()? != 0,
            5 => out.loop_count = r.varint()? as u32,
            6 => {
                let pay = r.len_payload()?;
                let mut sr = Reader::new(pay);
                let mut min = 0u32;
                let mut max = 0u32;
                while !sr.is_done() {
                    let t2 = sr.tag()?;
                    let w2 = t2.wire_type();
                    match t2.field() {
                        1 => min = sr.varint()? as u32,
                        2 => max = sr.varint()? as u32,
                        _ => sr.skip(w2)?,
                    }
                }
                out.sprite_phases.push((min, max));
            }
            _ => r.skip(wt)?,
        }
    }
    Some(out)
}

fn parse_flags(payload: &[u8]) -> Option<AppearanceFlags> {
    let mut out = AppearanceFlags::default();
    let mut r = Reader::new(payload);
    // sub-mensagens simples: leem um único campo varint
    fn simple_u32(pay: &[u8]) -> Option<u32> {
        let mut sr = Reader::new(pay);
        let tag = sr.tag()?;
        if tag.field() == 1 {
            sr.varint().map(|v| v as u32)
        } else {
            None
        }
    }
    while !r.is_done() {
        let tag = r.tag()?;
        let wt = tag.wire_type();
        match tag.field() {
            1 => {
                let pay = simple_u32(r.len_payload()?)?;
                let _ = pay;
                out.bank = Some(());
            }
            2 => out.clip = Some(r.varint()? != 0),
            3 => out.top = Some(r.varint()? != 0),
            4 => out.bottom = Some(r.varint()? != 0),
            5 => out.container = Some(r.varint()? != 0),
            6 => out.cumulative = Some(r.varint()? != 0),
            12 => out.liquidpool = Some(r.varint()? != 0),
            13 => out.unpass = Some(r.varint()? != 0),
            14 => out.unmove = Some(r.varint()? != 0),
            15 => out.unsight = Some(r.varint()? != 0),
            16 => out.avoid = Some(r.varint()? != 0),
            17 => out.no_movement_animation = Some(r.varint()? != 0),
            18 => out.take = Some(r.varint()? != 0),
            19 => out.liquidcontainer = Some(r.varint()? != 0),
            20 => out.hang = Some(r.varint()? != 0),
            21 => out.hook = Some(simple_u32(r.len_payload()?)?),
            22 => out.rotate = Some(r.varint()? != 0),
            23 => {
                let pay = r.len_payload()?;
                let mut sr = Reader::new(pay);
                let mut a = 0u32;
                let mut b = 0u32;
                while !sr.is_done() {
                    let t2 = sr.tag()?;
                    let w2 = t2.wire_type();
                    match t2.field() {
                        1 => a = sr.varint()? as u32,
                        2 => b = sr.varint()? as u32,
                        _ => sr.skip(w2)?,
                    }
                }
                out.light = Some((a, b));
            }
            26 => {
                let pay = r.len_payload()?;
                let mut sr = Reader::new(pay);
                let mut a = 0u32;
                let mut b = 0u32;
                while !sr.is_done() {
                    let t2 = sr.tag()?;
                    let w2 = t2.wire_type();
                    match t2.field() {
                        1 => a = sr.varint()? as u32,
                        2 => b = sr.varint()? as u32,
                        _ => sr.skip(w2)?,
                    }
                }
                out.shift = Some((a, b));
            }
            27 => out.height = Some(simple_u32(r.len_payload()?)?),
            29 => out.ignore_look = Some(r.varint()? != 0),
            30 => out.automap = Some(simple_u32(r.len_payload()?)?),
            31 => {
                let _ = r.len_payload()?;
                out.lenshelp = Some(());
            }
            42 => out.corpse = Some(r.varint()? != 0),
            43 => out.player_corpse = Some(r.varint()? != 0),
            46 => out.show_off_socket = Some(r.varint()? != 0),
            10 => {
                let _ = r.len_payload()?;
                out.write = Some(());
            }
            11 => {
                let _ = r.len_payload()?;
                out.write_once = Some(());
            }
            _ => r.skip(wt)?,
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Conversão para a tabela ItemType (espelha loadFromProtobuf)
// ---------------------------------------------------------------------------

pub fn build_item_table(appearances: &Appearances) -> ItemTypeTable {
    let mut table = ItemTypeTable::default();

    for object in &appearances.objects {
        let Some(flags) = &object.flags else {
            eprintln!(
                "[ItemDatabase::loadFromProtobuf] - Item with id {} is invalid and was ignored.",
                object.id.unwrap_or(0)
            );
            continue;
        };

        let Some(id) = object.id else {
            continue;
        };

        if id as usize >= table.items.len() {
            table.items.resize(id as usize + 1, None);
        }

        let mut t = ItemType {
            id: id as u16,
            client_id: id as u16,
            name: object.name.clone(),
            description: object.description.clone(),
            ..ItemType::default()
        };

        if flags.container == Some(true) {
            t.is_container = true;
            t.group = ItemGroup::Container;
        } else if flags.has_bank() {
            t.group = ItemGroup::Ground;
        } else if flags.liquidcontainer == Some(true) {
            t.group = ItemGroup::Fluid;
        } else if flags.liquidpool == Some(true) {
            t.group = ItemGroup::Splash;
        }

        if flags.has_clip() || flags.has_top() || flags.has_bottom() {
            t.always_on_bottom = true;
        }
        t.always_on_top_order = if flags.clip == Some(true) {
            1
        } else if flags.top == Some(true) {
            3
        } else if flags.bottom == Some(true) {
            2
        } else {
            0
        };

        t.animation_phases.clear();

        for framegroup in &object.frame_groups {
            let Some(sprite_info) = &framegroup.sprite_info else {
                continue;
            };
            let animation = &sprite_info.animation;

            t.pattern_width = sprite_info.pattern_width;
            t.pattern_height = sprite_info.pattern_height;
            t.pattern_depth = sprite_info.pattern_depth;
            t.layers = sprite_info.layers;

            if let Some(anim) = animation.as_ref().filter(|a| !a.sprite_phases.is_empty()) {
                t.start_frame = anim.default_start_phase as i8;
                t.loop_count = anim.loop_count;
                t.async_animation = !anim.synchronized;
                t.animation_phases = anim.sprite_phases.clone();
            }

            t.sprite_id = sprite_info.sprite_ids.first().copied().unwrap_or(0);
            t.sprite_ids = sprite_info.sprite_ids.clone();
        }

        t.no_move_animation = flags.no_movement_animation == Some(true);
        t.is_corpse = flags.corpse == Some(true) || flags.player_corpse == Some(true);
        t.force_use = flags.forceuse == Some(true);
        t.has_height = flags.has_height();
        t.unpassable = flags.unpass == Some(true);
        t.block_missiles = flags.unsight == Some(true);
        t.block_pathfinder = flags.avoid == Some(true);
        t.pickupable = flags.take == Some(true);
        t.moveable = flags.unmove != Some(true);
        t.can_read_text = flags.write.is_some() || flags.write_once.is_some();
        t.is_hangable = flags.hang == Some(true);
        t.stackable = flags.cumulative == Some(true);
        t.is_podium = flags.show_off_socket == Some(true);
        t.rotable = flags.rotate == Some(true);
        t.ignore_look = flags.ignore_look == Some(true);
        t.has_elevation = flags.has_height();

        t.hook = match flags.hook {
            Some(1) => ItemHook::South,
            Some(_) => ItemHook::East,
            None => ItemHook::None_,
        };

        // GameSprite metadata (graphics.cpp loadItemSpriteMetadata + items.cpp)
        t.sprite.draw_height = flags.height.map(|h| h as u16).unwrap_or(0);
        if let Some((x, y)) = flags.shift {
            t.sprite.draw_offset = (x as i32, y as i32);
        }
        if let Some((brightness, color)) = flags.light {
            t.sprite.light_color = color;
            t.sprite.light_intensity = brightness;
            t.sprite.has_light = true;
        }
        if let Some(color) = flags.automap {
            t.sprite.minimap_color = color as u16;
        }

        if t.id > table.max_item_id {
            table.max_item_id = t.id;
        }
        let id = t.id as usize;
        table.items[id] = Some(t);
    }

    table
}

/// Parseia os bytes do `appearances.dat` e constrói a tabela de itens.
pub fn load_appearances(data: &[u8]) -> Option<ItemTypeTable> {
    let appearances = parse(data)?;
    Some(build_item_table(&appearances))
}

pub fn parse_appearances(data: &[u8]) -> Option<Appearances> {
    parse(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Encode helpers (wire format) para testes sem depender de assets.
    fn varint(out: &mut Vec<u8>, v: u64) {
        let mut x = v;
        loop {
            let b = (x & 0x7f) as u8;
            x >>= 7;
            if x == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }

    fn field_varint(out: &mut Vec<u8>, field: u32, v: u64) {
        varint(out, u64::from(field << 3));
        varint(out, v);
    }

    fn field_len(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
        varint(out, u64::from((field << 3) | 2));
        varint(out, payload.len() as u64);
        out.extend_from_slice(payload);
    }

    #[test]
    fn parses_appearances_validating_item_table() {
        // Aparecência: id=2, nome, flags (bank) → ground
        let mut app = Vec::new();
        let mut obj = Vec::new();
        field_varint(&mut obj, 1, 2); // id
        let mut flags = Vec::new();
        field_len(&mut flags, 1, &[0x08, 0x00]); // bank { waypoints = 0 }
        field_varint(&mut flags, 13, 1); // unpass = true
        field_len(&mut obj, 3, &flags);
        field_len(&mut obj, 4, b"Grass"); // name
        // sprite_info com pattern 1x1x1, layers 1, sprite_ids [100,200]
        let mut si = Vec::new();
        field_varint(&mut si, 1, 1);
        field_varint(&mut si, 2, 1);
        field_varint(&mut si, 3, 1);
        field_varint(&mut si, 4, 1);
        field_varint(&mut si, 5, 100); // unpacked sprite_id
        field_len(&mut si, 5, &[0xC8, 0x01]); // packed: 200
        let mut fg = Vec::new();
        field_varint(&mut fg, 1, 2); // fixed_frame_group = OBJECT_INITIAL
        field_len(&mut fg, 3, &si);
        field_len(&mut obj, 2, &fg);
        field_len(&mut app, 1, &obj);

        let table = super::load_appearances(&app).unwrap();
        let item = table.get_opt(2).unwrap();
        assert_eq!(item.name, "Grass");
        assert_eq!(item.group, ItemGroup::Ground);
        assert!(item.is_ground_tile());
        assert!(item.unpassable);
        assert_eq!(item.always_on_top_order, 0);
        assert_eq!(item.sprite_ids, vec![100, 200]);
        // numsprites segue a fórmula do RME (pattern*layers*fases), não o tamanho
        // da lista de sprite ids — aqui pattern 1x1x1, 1 layer, sem animação → 1.
        assert_eq!(item.numsprites(), 1);
        assert!(table.get_opt(999).is_none());
        assert!(!table.get(999).is_ground_tile());
    }

    #[test]
    fn parses_container_and_animations() {
        let mut app = Vec::new();
        let mut obj = Vec::new();
        field_varint(&mut obj, 1, 7);
        let mut flags = Vec::new();
        field_varint(&mut flags, 5, 1); // container = true
        field_varint(&mut flags, 6, 1); // cumulative = true (stackable)
        field_len(&mut obj, 3, &flags);
        // animation com 2 phases, fase default 1, não sincronizado
        let mut anim = Vec::new();
        field_varint(&mut anim, 1, 1); // default_start_phase
        field_varint(&mut anim, 2, 0); // synchronized = false
        field_varint(&mut anim, 5, 3); // loop_count
        let mut phase = Vec::new();
        field_varint(&mut phase, 1, 500);
        field_varint(&mut phase, 2, 1000);
        field_len(&mut anim, 6, &phase);
        let mut si = Vec::new();
        field_varint(&mut si, 1, 2);
        field_varint(&mut si, 2, 3);
        field_varint(&mut si, 3, 1);
        field_varint(&mut si, 4, 2);
        field_len(&mut si, 6, &anim);
        let mut fg = Vec::new();
        field_len(&mut fg, 3, &si);
        field_len(&mut obj, 2, &fg);
        field_len(&mut app, 1, &obj);

        let table = super::load_appearances(&app).unwrap();
        let item = table.get_opt(7).unwrap();
        assert_eq!(item.group, ItemGroup::Container);
        assert!(item.is_container);
        assert!(item.stackable);
        assert_eq!(item.layers, 2);
        assert_eq!(item.pattern_width, 2);
        assert_eq!(item.pattern_height, 3);
        assert_eq!(item.animation_phases, vec![(500, 1000)]);
        assert_eq!(item.start_frame, 1);
        assert_eq!(item.loop_count, 3);
        assert!(item.async_animation);
        // numsprites = 2*2*3*1 * max(1,1) = 12
        assert_eq!(item.numsprites(), 12);
    }

    #[test]
    fn rejects_truncated_data() {
        let app = [0x0a, 0x05, 0x08, 0x01]; // campo 1 len 5 mas só 4 bytes
        assert!(super::load_appearances(&app).is_none());
    }
}