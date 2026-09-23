# Formato OTBM — Catálogo de regras para carregar e exibir mapas

> **Atualização importante (estudo do fonte):** este RME/Canary usa `CLIENT_VERSION 1100`
> (`definitions.h:53`) e, portanto, **os loaders legados (Tibia.dat/.spr/items.otb) estão
> compilados fora**; o pipeline ativo é o novo `appearances.dat` (protobuf) +
> `catalog-content.json` + sheets LZMA 384×384 (`client_assets.cpp:61-171`,
> `sprite_appearances.cpp`). Ver seção 10 para os dois caminhos possíveis no Rust.

Catálogo extraído do código-fonte do RME (symlink `reference-src/`, interna
`remeres-map-editor`). O objetivo é servir de base para a implementação Rust
carregar um `.otbm` e exibi-lo.

Endpoints principais do RME:
- `reference-src/source/filehandle.h` + `filehandle.cpp` — formato binário de nós.
- `reference-src/source/iomap_otbm.h` + `iomap_otbm.cpp` — leitura/escrita OTBM.
- `reference-src/source/tile.h` / `tile.cpp` — modelo do tile e ordenação.
- `reference-src/source/items.h` / `items.cpp` — definição de itens (ItemType).
- `reference-src/source/map_drawer.cpp` — regras de renderização.
- `reference-src/source/const.h`, `position.h` — limites e coordenadas.

## 1. Formato binário dos nós (a base de tudo)

`filehandle.h:43-47`, `filehandle.cpp:425-483`

O arquivo é uma árvore de nós com 3 tokens:

| Token | Byte | Função |
|---|---|---|
| `NODE_START` | `0xFE` | início de nó |
| `NODE_END` | `0xFF` | fim de nó |
| `ESCAPE_CHAR` | `0xFD` | escape: o byte seguinte é lido literal |

Semântica de leitura do payload de um nó (loop até achar token):
- `0xFE` → abre nó filho.
- `0xFF` → fecha o nó e volta ao pai.
- `0xFD` → desescapa: o próximo byte é append no payload (`0xFD`/`0xFE`/`0xFF` literais).

Inteiros são little-endian (`getU8/getU16/getU32`, `getString` = `u16 len` + bytes,
`getLongString` = `u32 len` + bytes).

## 2. Cabeçalho / magic

`iomap_otbm.cpp:4882-4892`

- 4 bytes: `"OTBM"` **ou** `0x00 0x00 0x00 0x00` (wildcard).
- byte `[4]` deve ser `0xFE` (início do root node).
- Parsing real começa em `offset 4` (`MemoryNodeFileReadHandle(data + 4)`).

## 3. Root node

`iomap_otbm.cpp:4917-4957`, `getVersionInfo:4688-4711`

- skip 1 byte (tipo do nó).
- `u32 version` → enum `MapVersionID` (`client_assets.h:24-34`):
  `MAP_OTBM_1=0, MAP_OTBM_2=1, MAP_OTBM_3=2, MAP_OTBM_4=3, MAP_OTBM_5=4, MAP_OTBM_6=5`.
  Erro se `> MAP_OTBM_LAST_VERSION` (5).
- `u16 width`, `u16 height` (dimensões do mapa em tiles).
- `getVersionInfo` ainda lê `u32 majorVersionItems`, `u32 minorVersionItems`,
  e skip de 4 bytes (versão do OTB, deprecated).

## 4. Nó MAP_DATA (filho do root)

`iomap_otbm.cpp:4959-5004`

Primeiro byte do filho: `OTBM_MAP_DATA=2`. Depois, atributos (`u8` + payload):

| Attr | Value | Payload |
|---|---|---|
| `OTBM_ATTR_DESCRIPTION` | 1 | string |
| `OTBM_ATTR_EXT_SPAWN_MONSTER_FILE` | 2 | string |
| `OTBM_ATTR_EXT_HOUSE_FILE` | 13 | string |
| `OTBM_ATTR_EXT_SPAWN_NPC_FILE` | 23 | string |
| `OTBM_ATTR_EXT_ZONE_FILE` | 24 | string |

Soldados desconhecidos → warning e segue. Após os atributos, os nós filhos do
MAP_DATA são processados (TILE_AREA, TOWNS, WAYPOINTS).

## 5. TILE_AREA (o grosso do mapa)

`iomap_otbm.cpp:5035-5187`

Cabeçalho: `u16 base_x`, `u16 base_y`, `u8 base_z`.

Cada tile filho:
- primeiro byte: `OTBM_TILE=5` ou `OTBM_HOUSETILE=14`.
- `u8 x_offset`, `u8 y_offset` → posição absoluta = `base + offset`.
- `OTBM_HOUSETILE`: `u32 house_id` a seguir.

Atributos do tile (`u8` + payload):
- `OTBM_ATTR_TILE_FLAGS=3` → `u32` flags de mapa. Enums (`tile.h:28-44`):
  `TILESTATE_PROTECTIONZONE=0x0001, NOPVP=0x0004, NOLOGOUT=0x0008,
  PVPZONE=0x0010, REFRESH=0x0020`.
- `OTBM_ATTR_ITEM=9` → item compacto: `u16 id` (+`u8 count` se OTBM1 e
  `stackable|splash|fluidContainer`).

Filhos do tile:
- `OTBM_ITEM=6` → item completo (u16 id + stream de atributos + filhos de container).
- `OTBM_TILE_ZONE=19` → `u16 zone_count` + `u16 zone_id` ×count.

Regras:
- Tiles duplicados (mesma posição) são **descartados** (mantém o primeiro).
- `needsFullTileUpdate` = teve ground duplicado (`isGroundTile`/`ground_equivalent`
  com `tile->ground` já setado) ou `alwaysOnBottom` com items já presentes → `update()`.
  Senão `finalizeLoadedState()`.

## 6. Item — criação compacta e atributos

`iomap_otbm.cpp:4281-4303`, `unserializeAttributes:4379-4395`

`Item::Create_OTBM`:
- `u16 id` (item id de servidor), validado contra `g_items` (definição de itens).
- OTBM1: `u8 count` se o tipo for `stackable || isSplash() || isFluidContainer()`.

Stream de atributos (loop `u8 attr` + payload até acabar):

| Attr | Value | Payload |
|---|---|---|
| `OTBM_ATTR_COUNT` | 15 | u8 → subtype |
| `OTBM_ATTR_RUNE_CHARGES` | 12 | u8 → subtype |
| `OTBM_ATTR_CHARGES` | 22 | u16 → subtype |
| `OTBM_ATTR_ACTION_ID` | 4 | u16 |
| `OTBM_ATTR_UNIQUE_ID` | 5 | u16 |
| `OTBM_ATTR_TEXT` | 6 | string |
| `OTBM_ATTR_DESC` | 7 | string |
| `OTBM_ATTR_DEPOT_ID` | 10 | skip 2 |
| `OTBM_ATTR_HOUSEDOORID` | 14 | skip 1 |
| `OTBM_ATTR_TELE_DEST` | 8 | skip 5 |
| `OTBM_ATTR_ATTRIBUTE_MAP` | 128 | mapa de atributos |

`ATTRIBUTE_MAP` (`item_attributes.cpp:279-391`): `u16 count`, e por entrada
string key (`u16 len`), depois valor com `u8 type`:
1=string long (`u32 len`), 2=u32, 4=bool (u8), 5=u64.

Containers: após os próprios atributos, filhos `OTBM_ITEM` recursivos
(`complexitem.cpp`).

## 7. TOWNS / WAYPOINTS (opcionais para exibir)

- `OTBM_TOWNS=12` → filho `OTBM_TOWN=13`: `u32 town_id`, string nome,
  `u16 x, u16 y, u8 z` (posição do templo).
- `OTBM_WAYPOINTS=15` → filho `OTBM_WAYPOINT=16`: string nome,
  `u16 x, u16 y, u8 z`.

## 8. Regras de renderização

Cadeia de resolução do sprite:
`item_id (OTBM)` → **definição de itens** (ItemType com `clientID`) → **.spr sprite id**.

`map_drawer.cpp:1343-1418` (`BlitItem`), `tile.cpp:326-387` (`addItem`):

1. **Ordem de empilhamento (addItem/addLoadedItem)**:
   - `isGroundTile()` → vira `tile->ground` (só um por tile; novo substitui).
   - `ground_equivalent != 0` → cria piso virtual `Item(gid)` como novo ground e
     o item real vai para o **começo** de `items` (fundo).
   - `alwaysOnBottom` → inserido no fundo, ordenado por `alwaysOnTopOrder`
     (default 0), antes do primeiro item em que `!alwaysOnBottom || topOrder < alwaysOnTopOrder`.
   - demais → append no fim de `items`.
2. **Render por tile (DrawTile)**: desenha `ground` depois `items` do fundo ao topo.
3. **Sprite**: `ItemType.clientID → GameSprite`; índice do frame:
   `getHardwareID(0, subtype, x%pattern_x, y%pattern_y, z%pattern_z, frame)`.
4. **Offsets**: `screen = (tile_origin - drawOffset)`, depois `-= drawHeight`
   (empilhamento vertical).
5. **Subtype de frame**:
   - splash/fluid: `item->getSubtype()`.
   - hangable: `pattern_x = 0`.
   - stackable: buckets do count — `<=1→0, <=2→1, <=3→2, <=4→3, <10→4, <25→5, <50→6, senão 7`.
6. **Transparência** (modo transparent): ground 1×1 é opaco; sprites maiores ou
   escadas dividem alpha por 2.
7. **Minimap/tint**: `sprite->getMiniMapColor()` — primeira cor ≠0, priorizando ground;
   tint por flags (selecionado, PZ/PVP/etc).

## 9. Limites e constantes

`const.h:26-36`, `position.h:87-94`

- `MapLayers=16`, `MapMinLayer=0`, `MapMaxLayer=15`, `MapGroundLayer=7`.
- `MapMaxWidth=65000`, `MapMaxHeight=65000`.
- `Position.isValid()`: `(0,0,0)` é inválida; precisa `0<=x<=65000`, `0<=y<=65000`,
  `0<=z<=15`.
- Armazenamento do RME: quadtree de floors, 4×4 tiles por floor
  (`locs[(x&3)*4 + (y&3)]`). Para exibir em Rust, qualquer map hash/tile por posição
  serve — o OTBM só impõe as áreas de 256×256 no *save*.

## 10. As duas rotas: item_id → sprite

O OTBM usa **item ids**, mas a tela precisa de **sprites**. As definições vivem
em arquivos de client. Há dois caminhos mutuamente exclusivos no RME (por
`CLIENT_VERSION` em `definitions.h`):

### Rota A — Legado (Tibia 10.x e anteriores): `.spr` + `.dat` (+ opcional `items.otb`)
- `clientVersion` < 1100 → `g_gui.loadOtfi`, `ItemDatabase::loadFromOtb`,
  `ClientVersion::loadSpriteData` etc.
- Cadeia: OTBM `item id` → `Tibia.dat` (informação por item id: `clientID`, flags
  `isGroundTile/ground_equivalent/alwaysOnBottom/alwaysOnTopOrder/stackable`,
  hooked sprites para hangable/automap) → **sprite id** no `Tibia.spr`.
- Nosso Rust já tem o leitor de `.spr` (`editor_formats::spr`) com sprites 32×32
  (`SPR_EXTENDED_COUNT`, `SPR_HAS_ALPHA`, `decode(sprite_id) → [u8; 32·32·4]`).
  Falta o parser de `Tibia.dat` (ou `items.otb` + dat de sprites) para mapear
  `item_id → sprite_id` e as flags de ordenação.

### Rota B — Novo (clientes 11.x+/Canary, o que ESTE fork usa)
- `CLIENT_VERSION 1100` → `client_assets.cpp`: `ClientAssets::loadAppearanceProtobuf`
  (`appearance.dat`/`appearances.dat`, protobuf) + `catalog-content.json` +
  sheets de sprites `.lzma` (descompactadas uma vez em `spriteSheetBlob`, 384×384,
  LZMA+DEVIRTUALIZE, cache em disco em `__appearanceDatPackages`).
- Cadeia: OTBM `item id` **==** id da `appearances.dat`; o `AppearanceInformations`
  dá flags (`flags.scaleX/Y`, `ground`, `groundEquivalent`, `alwaysOnBottom`,
  `alwaysOnTopOrder`, `stackable`) e `spriteInfo` (`id`), e
  `ClientAssets::getGameSprite` resolve cada sprite (com tamanho/offset próprios).
- Implica atlas de tamanhos variados (32×32, 32×64, 64×64…) e decodificação LZMA
  no Rust — mais trabalho que a rota A.

### Recomendação para a versão crua mínima (Rust)
Para "carregar e exibir o mínimo", começar pela **rota A** (`.spr` já suportado
no editor_formats), restrito a **ground 32×32** como primeiro marco; depois evoluir
para `.dat`, multi-tamanho e, se necessário, rota B.