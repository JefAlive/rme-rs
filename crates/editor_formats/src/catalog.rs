//! Catálogo de assets do cliente (`catalog-content.json`).
//!
//! Fonte da verdade: `reference-src/source/sprite_appearances.cpp::loadCatalogContent`
//! — o arquivo é um array JSON cujas entradas listam cada sprite sheet (com
//! `firstspriteid`/`lastspriteid`/`spritetype`) e o nome do arquivo de
//! `appearances`.

use serde_json::Value;

/// Layout de sprites dentro de uma sheet (SpriteLayout do C++).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpriteLayout {
    #[default]
    OneByOne = 0,
    OneByTwo = 1,
    TwoByOne = 2,
    TwoByTwo = 3,
}

impl SpriteLayout {
    /// Tamanho do sprite individual na sheet (getSpriteSize).
    pub fn sprite_size(self) -> (u32, u32) {
        match self {
            SpriteLayout::OneByOne => (32, 32),
            SpriteLayout::OneByTwo => (32, 64),
            SpriteLayout::TwoByOne => (64, 32),
            SpriteLayout::TwoByTwo => (64, 64),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpriteSheetInfo {
    pub first_id: u32,
    pub last_id: u32,
    pub sprite_type: SpriteLayout,
    /// Nome do arquivo (relativo ao diretório de assets).
    pub file: String,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub appearance_file: String,
    pub sheets: Vec<SpriteSheetInfo>,
    pub sprites_count: u32,
}

impl Catalog {
    /// Encontra a sheet que contém `sprite_id` (lower_bound em `last_id`,
    /// igual ao RME), retornando `None` se o id estiver fora de alcance.
    pub fn sheet_for_sprite(&self, sprite_id: u32) -> Option<&SpriteSheetInfo> {
        if sprite_id == 0 {
            return None;
        }
        let idx = self.sheets.partition_point(|s| s.last_id < sprite_id);
        let sheet = self.sheets.get(idx)?;
        if sprite_id < sheet.first_id {
            return None;
        }
        Some(sheet)
    }
}

/// Parseia o conteúdo do `catalog-content.json` seguindo o `loadCatalogContent`.
pub fn parse_catalog(data: &str) -> Result<Catalog, String> {
    let document: Value =
        serde_json::from_str(data).map_err(|e| format!("catalog-content.json inválido: {e}"))?;

    let array = document
        .as_array()
        .ok_or_else(|| "catalog-content.json deve ser um array JSON".to_string())?;

    let mut catalog = Catalog::default();
    let mut sheets = Vec::with_capacity(array.len());

    for obj in array {
        let Some(typ) = obj.get("type").and_then(Value::as_str) else {
            continue;
        };
        match typ {
            "appearances" => {
                if let Some(file) = obj.get("file").and_then(Value::as_str) {
                    catalog.appearance_file = file.to_string();
                }
            }
            "sprite" => {
                let get = |key: &str| -> Option<u32> {
                    obj.get(key).and_then(Value::as_u64).map(|v| v as u32)
                };
                let (Some(first), Some(last)) = (get("firstspriteid"), get("lastspriteid")) else {
                    continue;
                };
                let sprite_type = match get("spritetype").unwrap_or(0) {
                    1 => SpriteLayout::OneByTwo,
                    2 => SpriteLayout::TwoByOne,
                    3 => SpriteLayout::TwoByTwo,
                    _ => SpriteLayout::OneByOne,
                };
                let file = obj
                    .get("file")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                catalog.sprites_count = catalog.sprites_count.max(last);
                sheets.push(SpriteSheetInfo {
                    first_id: first,
                    last_id: last,
                    sprite_type,
                    file,
                });
            }
            _ => {}
        }
    }

    // loadCatalogContent ordena por lastId (usado no lower_bound).
    sheets.sort_by_key(|s| s.last_id);
    catalog.sheets = sheets;
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_catalog_and_finds_sheet() {
        let json = r#"[
            {"type":"appearances","file":"appearances-abc.dat"},
            {"type":"map","file":"map-x.dat"},
            {"type":"sprite","file":"a.bmp.lzma","spritetype":0,"firstspriteid":0,"lastspriteid":143,"area":0},
            {"type":"sprite","file":"b.bmp.lzma","spritetype":1,"firstspriteid":144,"lastspriteid":287,"area":0}
        ]"#;
        let cat = parse_catalog(json).unwrap();
        assert_eq!(cat.appearance_file, "appearances-abc.dat");
        assert_eq!(cat.sheets.len(), 2);
        assert_eq!(cat.sprites_count, 287);
        assert_eq!(cat.sheets[0].sprite_type, SpriteLayout::OneByOne);
        assert_eq!(cat.sheets[1].sprite_type, SpriteLayout::OneByTwo);
        assert_eq!(cat.sheets[1].sprite_type.sprite_size(), (32, 64));
        assert_eq!(cat.sheet_for_sprite(10).unwrap().first_id, 0);
        assert_eq!(cat.sheet_for_sprite(150).unwrap().first_id, 144);
        assert!(cat.sheet_for_sprite(5000).is_none());
    }
}