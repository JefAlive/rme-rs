//! Importação de um `OtbmDocument` (editor_formats) para o `MapDocument` do
//! editor. Espelha o `IOMapOTBM::loadMap` + `Tile::addItem` do RME:
//!  - o 1º/e último item `isGroundTile` vira `Tile.ground` (no RME o slot de
//!    chão é sobrescrito a cada ground; mapas válidos nunca têm dois);
//!  - itens `alwaysOnBottom` são inseridos na posição ordenada por
//!    `alwaysOnTopOrder` (findBottomInsertPosition); os demais vão ao fim;
//!  - `TILESTATE_*` → `TileZoneFlags` e `houseId` → `HOUSE_TILE`.

use editor_formats::appearances::{ItemGroup, ItemTypeTable};
use editor_formats::otbm::{
    OtmItem, OtmTile, OtbmDocument,
    TILESTATE_PROTECTIONZONE, TILESTATE_NOPVP, TILESTATE_NOLOGOUT,
    TILESTATE_PVPZONE, TILESTATE_REFRESH,
};

use crate::item::{Item, ItemAttrFlags, Tile, TileZoneFlags};
use crate::position::Position;
use crate::spatial_map::SpatialMap;
use crate::MapDocument;

/// Bounding box do mapa em coordenadas de tile (para a câmera).
#[derive(Debug, Clone, Copy, Default)]
pub struct Bounds {
    pub min_x: u16,
    pub min_y: u16,
    pub max_x: u16,
    pub max_y: u16,
    pub min_z: u8,
    pub max_z: u8,
}

impl Bounds {
    fn expand(&mut self, pos: Position) {
        self.min_x = self.min_x.min(pos.x);
        self.min_y = self.min_y.min(pos.y);
        self.max_x = self.max_x.max(pos.x);
        self.max_y = self.max_y.max(pos.y);
        self.min_z = self.min_z.min(pos.z);
        self.max_z = self.max_z.max(pos.z);
    }
}

/// Mesmo critério do RME para embutir o count logo após o id (OTBM v1):
/// stackables, fluidos e splashes.
pub fn is_subtype_embedded(table: &ItemTypeTable, id: u16) -> bool {
    table.get_opt(id).is_some_and(|t| {
        t.stackable || t.group == ItemGroup::Fluid || t.group == ItemGroup::Splash
    })
}

/// Converte um mapa OTBM já parseado em um `MapDocument` pronto para o editor.
pub fn import_otbm(doc: &OtbmDocument, table: &ItemTypeTable) -> (MapDocument, Bounds) {
    let mut map = SpatialMap::default();
    let mut bounds = Bounds::default();

    for t in &doc.tiles {
        let pos = Position { x: t.x, y: t.y, z: t.z };
        bounds.expand(pos);
        *map.get_tile_mut(pos) = otm_tile_to_tile(t, table);
    }

    let mut document = MapDocument::new(if doc.description.is_empty() {
        "map.otbm"
    } else {
        doc.description.as_str()
    });
    document.map = map;
    (document, bounds)
}

fn otm_tile_to_tile(src: &OtmTile, table: &ItemTypeTable) -> Tile {
    let mut tile = Tile {
        house_id: src.house_id,
        ..Default::default()
    };
    if src.house_id != 0 {
        tile.zone |= TileZoneFlags::HOUSE_TILE;
    }
    let f = src.flags;
    if f & TILESTATE_PROTECTIONZONE != 0 {
        tile.zone |= TileZoneFlags::PROTECTION_ZONE;
    }
    if f & TILESTATE_NOPVP != 0 {
        tile.zone |= TileZoneFlags::NO_PVP_ZONE;
    }
    if f & TILESTATE_NOLOGOUT != 0 {
        tile.zone |= TileZoneFlags::NO_LOGOUT;
    }
    if f & TILESTATE_PVPZONE != 0 {
        tile.zone |= TileZoneFlags::PVP_ZONE;
    }
    if f & TILESTATE_REFRESH != 0 {
        tile.zone |= TileZoneFlags::REFRESH;
    }

    for item in &src.items {
        add_item(&mut tile, item, table);
    }
    tile
}

/// Espelho de `Tile::addItem` (ground sobrescreve; alwaysOnBottom ordenado
/// por alwaysOnTopOrder; demais no final).
fn add_item(tile: &mut Tile, src: &OtmItem, table: &ItemTypeTable) {
    let item = to_item(src, table);
    let Some(typ) = table.get_opt(item.type_id) else {
        tile.items.push(item);
        return;
    };

    if typ.group == ItemGroup::Ground {
        tile.ground = Some(item);
        return;
    }

    if typ.always_on_bottom {
        let insert_at = tile
            .items
            .iter()
            .position(|existing| {
                let et = table.get_opt(existing.type_id);
                !et.is_some_and(|e| e.always_on_bottom)
                    || et.is_some_and(|e| typ.always_on_top_order < e.always_on_top_order)
            })
            .unwrap_or(tile.items.len());
        tile.items.insert(insert_at, item);
    } else {
        tile.items.push(item);
    }
}

fn to_item(src: &OtmItem, _table: &ItemTypeTable) -> Item {
    let mut item = Item::new(src.id);
    if src.subtype != 0 {
        item.attrs |= ItemAttrFlags::HAS_COUNT;
        item.count = src.subtype.min(u8::MAX as u16) as u8;
    }
    if src.action_id != 0 {
        item.attrs |= ItemAttrFlags::HAS_ACTION_ID;
        item.action_id = src.action_id;
    }
    if src.unique_id != 0 {
        item.attrs |= ItemAttrFlags::HAS_UNIQUE_ID;
        item.unique_id = src.unique_id;
    }
    if let Some(text) = &src.text {
        item.attrs |= ItemAttrFlags::HAS_TEXT;
        item.text = Some(text.as_str().into());
    }
    if !src.container.is_empty() {
        item.attrs |= ItemAttrFlags::HAS_CONTAINER;
        item.container = Some(Box::new(
            src.container.iter().map(|c| to_item(c, _table)).collect(),
        ));
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_formats::appearances::ItemType;
    use editor_formats::otbm::OtmTile;

    fn table() -> ItemTypeTable {
        let mut items = vec![None; 2001];
        items[96] = Some(ItemType { id: 96, group: ItemGroup::Ground, sprite_ids: vec![1000], ..Default::default() });
        items[100] = Some(ItemType { id: 100, group: ItemGroup::Ground, sprite_ids: vec![1100], ..Default::default() });
        items[2000] = Some(ItemType { id: 2000, group: ItemGroup::None_, always_on_bottom: true, always_on_top_order: 2, ..Default::default() });
        ItemTypeTable { items, max_item_id: 2000 }
    }

    fn tile(x: u16, y: u16, z: u8, flags: u32, house_id: u32, items: Vec<(u16, u16)>) -> OtmTile {
        OtmTile {
            x, y, z,
            flags,
            house_id,
            items: items.into_iter().map(|(id, subtype)| { let mut it = OtmItem { id, ..Default::default() }; it.subtype = subtype; it }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn imports_ground_zone_and_house() {
        let table = table();
        let doc = OtbmDocument {
            version: 2,
            description: "test".into(),
            tiles: vec![tile(1, 2, 7, TILESTATE_PROTECTIONZONE | TILESTATE_NOLOGOUT, 77, vec![(96, 0)])],
            ..Default::default()
        };
        let (doc, bounds) = import_otbm(&doc, &table);
        let t = doc.map.get_tile(Position { x: 1, y: 2, z: 7 }).unwrap();
        assert_eq!(t.ground.as_ref().unwrap().type_id, 96);
        assert!(t.zone.contains(TileZoneFlags::PROTECTION_ZONE));
        assert!(t.zone.contains(TileZoneFlags::NO_LOGOUT));
        assert!(t.zone.contains(TileZoneFlags::HOUSE_TILE));
        assert_eq!(t.house_id, 77);
        assert_eq!(bounds.min_x, 1);
        assert_eq!(bounds.max_x, 1);
        assert_eq!(bounds.max_y, 2);
    }

    #[test]
    fn ground_overwrites_and_bottom_items_are_ordered() {
        let table = table();
        let doc = OtbmDocument {
            version: 2,
            tiles: vec![tile(
                0, 0, 7, 0, 0,
                vec![(2000, 0), (96, 0), (100, 0), (2000, 0)],
            )],
            ..Default::default()
        };
        let (doc, _) = import_otbm(&doc, &table);
        let t = doc.map.get_tile(Position { x: 0, y: 0, z: 7 }).unwrap();
        // último ground vence (RME sobrescreve o slot)
        assert_eq!(t.ground.as_ref().unwrap().type_id, 100);
        // both 2000 are alwaysOnBottom (order 2) e devem ficar ordenados
        assert_eq!(t.items.len(), 2);
        assert!(t.items.iter().all(|i| i.type_id == 2000));
    }

    #[test]
    fn subtype_embedded_detects_stackables_and_fluids() {
        let mut table = table();
        table.items[7] = Some(ItemType { id: 7, stackable: true, ..Default::default() });
        table.items[8] = Some(ItemType { id: 8, group: ItemGroup::Fluid, ..Default::default() });
        table.items[9] = Some(ItemType { id: 9, group: ItemGroup::Splash, ..Default::default() });
        assert!(is_subtype_embedded(&table, 7));
        assert!(is_subtype_embedded(&table, 8));
        assert!(is_subtype_embedded(&table, 9));
        assert!(!is_subtype_embedded(&table, 96));
        assert!(!is_subtype_embedded(&table, 9999));
    }
}