use smallvec::SmallVec;

pub type ItemTypeId = u16;

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
    pub struct ItemAttrFlags: u8 {
        const HAS_COUNT      = 1 << 0;
        const HAS_ACTION_ID  = 1 << 1;
        const HAS_UNIQUE_ID  = 1 << 2;
        const HAS_TEXT       = 1 << 3;
        const HAS_CONTAINER  = 1 << 4;
    }
}

/// Instância de item no mapa. Metadados "estáticos" (bloqueia passagem, luz,
/// se é porta) vivem em ItemTypeTable, indexados por `type_id` — nunca duplicados aqui.
#[derive(Clone, Debug)]
pub struct Item {
    pub type_id: ItemTypeId,
    pub attrs: ItemAttrFlags,
    pub count: u8,          // subtype / stack count / fluid type
    pub action_id: u16,
    pub unique_id: u16,
    pub text: Option<Box<str>>,          // só aloca se HAS_TEXT
    pub container: Option<Box<Vec<Item>>>, // só aloca se HAS_CONTAINER
}

impl Item {
    pub fn new(type_id: ItemTypeId) -> Self {
        Self {
            type_id,
            attrs: ItemAttrFlags::empty(),
            count: 1,
            action_id: 0,
            unique_id: 0,
            text: None,
            container: None,
        }
    }
}

bitflags::bitflags! {
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
    pub struct TileZoneFlags: u16 {
        const PROTECTION_ZONE = 1 << 0;
        const NO_LOGOUT       = 1 << 1;
        const PVP_ZONE        = 1 << 2;
        const NO_PVP_ZONE     = 1 << 3;
        const HOUSE_TILE      = 1 << 4;
        const REFRESH         = 1 << 5;
    }
}

/// SmallVec: a maioria dos tiles tem 0-3 itens além do chão — evita heap
/// para o caso comum, sem sacrificar tiles com pilhas grandes.
#[derive(Clone, Debug, Default)]
pub struct Tile {
    pub ground: Option<Item>,
    pub items: SmallVec<[Item; 4]>,
    pub zone: TileZoneFlags,
    pub house_id: u32,
    pub spawn_id: Option<u32>,
}

impl Tile {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ground.is_none() && self.items.is_empty()
            && self.zone.is_empty() && self.house_id == 0 && self.spawn_id.is_none()
    }
}