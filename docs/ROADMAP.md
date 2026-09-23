# Roadmap — Carregar e exibir um .otbm (versão crua mínima)

> Objetivo desta fase: o editor **carrega um `.otbm` e exibe o mapa na tela**,
> sem funções de edição. Pipeline 100% novo (`appearances.dat` + `catalog-content.json`
> + sheets `.bmp.lzma`). Legado `.spr` é eliminado (não voltaremos mais nele).
>
> **Regra do projeto:** a lógica do `reference-src` (symlink para
> `remeres-map-editor`) é confiável e é a fonte da verdade. **Porte fiel, sem
> re-engenharia por tentativa/erro.** Cada fase lista o que portar e de onde.

## Decisões de arquitetura

1. **Formatos vivem em `editor_formats`** (crate puro I/O, sem dep de wgpu/egui).
   O crate passa a ser o lugar de: `otbm.rs`, `appearances.rs` (protobuf),
   `catalog.rs` (JSON), `sprite.rs` (sheets LZMA + extração) e tipos auxiliares.
2. **Protobuf hand-rolled.** O schema (`appearances.proto`) é fixo e conhecido;
   wire format simples (varint + wire types). Evita dependência pesada (`prost`/`protobuf`).
   Apenas os campos necessários para exibir/ordenar itens são decodificados.
3. **JSON leve.** `catalog-content.json` (e depois `package.json`) com `serde_json`.
4. **LZMA1 raw:** preferir crate puro Rust `lzma-rs` (raw LZMA1 com lc/lp/pb/dict
   customizados — igual `lzma_raw_decoder` do C++). Falback: FFI `xz2`/liblzma
   (só se lzma-rs não der conta do raw com props custom).
5. **Exibição:** manter a arquitetura atual do wgpu (textura-em-array + instâncias +
   shader de quads 32×32). Cada sprite usado é **extraído sob demanda** da sheet
   para RGBA e registrado como uma layer no atlas (cache `sprite_id → layer`).
   Redimensionar quad/âncora por sprite só entra quando passarmos de ground 32×32.
6. **Mapa em memória:** já existe (`editor_core::SpatialMap`/`Tile`/`Item` com
   `type_id` = item id do OTBM). O rendering resolve `type_id → sprite` via tabela
   de appearances.

## Fase 0 — Limpeza do legado `.spr`

- Remover `editor_formats/src/spr.rs` e o `pub mod spr` do `lib.rs`.
- Remover do `main.rs`: `SPR_PATH`, `SPRITE_LOAD_COUNT`, `SPR_EXTENDED_COUNT`,
  `SPR_HAS_ALPHA`, o `SpriteCatalog::load` e a demo hardcoded 16×16.
- Remover a suposição `type_id == sprite` no `scene.rs` (o `TileInstance.layer_index`
  passa a ser o **índice da layer no atlas de sprites**, não o item id).
- Critério: código compila; atlas placeholder entrou em loop de render vazio.

## Fase 1 — Parser OTBM

Fonte da verdade: `reference-src/source/filehandle.{h,cpp}` (nós 0xFE/0xFF/0xFD),
`reference-src/source/iomap_otbm.cpp` (root, MAP_DATA, TILE_AREA, TILE, ITEM),
resumo em `docs/OTBM.md`.

- `editor_formats/src/otbm.rs`:
  - Leitor de payload com tokens (`NODE_START=0xFE`, `NODE_END=0xFF`, `ESCAPE=0xFD`),
    inteiros LE, strings (`u16 len`), long strings (`u32 len`).
  - Root: magic `OTBM`/wildcard + `u8` + `u32 version` (≤5) + `u16 w/h`.
  - `MAP_DATA=2` (atributos de descrição/spawn/house/zone), depois filhos:
  - `TILE_AREA=4` (`u16 base_x/y`, `u8 base_z`) → `TILE=5`/`HOUSETILE=14`
    (`u8 x_off/y_off`, house `u32`), attrs `TILE_FLAGS=3` (u32), `ITEM=9` (compacto);
  - `ITEM=6` completo: `u16 id`, stream de atributos, filhos container recursivos;
  - `TOWNS=12/13`, `WAYPOINTS=15/16` (ignorados na v0, só logados).
  - Atributos de item por `docs/OTBM.md` §6 (`COUNT=15`, `CHARGES=22`, `ACTION_ID=4`,
    `UNIQUE_ID=5`, `TEXT=6`, `DESC=7`, `DEPOT_ID=10`, `HOUSEDOORID=14`,
    `TELE_DEST=8`, `ATTRIBUTE_MAP=128`).
  - Duplicados (mesma posição) descartados (mantém o primeiro), como o RME.
- Saída: preencher `editor_core::SpatialMap` com `Tile { ground, items, zone, house_id }`.
- Critério: um binário de teste imprime contagem de tiles/items/zonas e projeta o
  bounding box do mapa; bate com o RME abrindo o mesmo arquivo.

## Fase 2 — Tabela de itens (`appearances.dat`, protobuf)

Fonte da verdade: `reference-src/source/protobuf/appearances.proto`,
`reference-src/source/client_assets.cpp` (`loadAppearanceProtobuf`),
`reference-src/source/items.cpp` (`ItemType::loadFromProtobuf`).

- `editor_formats/src/appearances.rs` — mini-leitor protobuf:
  - varint / wire types; parsed `Appearances` (objetos `<Appearance>`).
  - `Appearance { id, name, flags, frame_group[idx].sprite_info }`.
  - Flags relevantes para renderizar/ordenar: `ground`, `groundEquivalent`,
    `alwaysOnBottom` (bottom), `alwaysOnTopOrder` (top), `stackable` (cumulative),
    `height`, `translucent`, `shift`/etc. são lembrados para fases futuras.
  - `SpriteInfo`: `pattern_width/height/depth`, `layers`, `sprite_id[]`, `animation`.
- Índice `type_id → &Appearance` (vetor esparso, como RME).
- Critério: com o `appearances.dat` de `reference-assets`, todo item presente no
  Dawnport resolve para uma appearance com sprite válido.

## Fase 3 — Sprite sheets (`.bmp.lzma`)

Fonte da verdade: `reference-src/source/sprite_appearances.cpp`
(`loadCatalogContent`, `loadSpriteSheet`, `getSpriteUVs`, `getSprite`).

- `editor_formats/src/catalog.rs` — `catalog-content.json`:
  - itens `type == "appearances"` → nome do arquivo de appearances;
  - `type == "sprite"` → `{ firstspriteid, lastspriteid, spritetype, file }`;
    `spritetype ∈ {0:32×32, 1:32×64, 2:64×32, 3:64×64}`; lista ordenada por `lastId`.
- `editor_formats/src/sprite.rs` — decode de sheet:
  - header CIP de 32 bytes (zeropad + `70 0A FA 80 24` + tamanho LZMA 7-bit)
    exatamente como em `loadSpriteSheet`; props `lc/lp/pb` + `dict_size` lidos da
    mesma forma; pulo dos 8 bytes de "compressed size".
  - `lzma` raw decode → BMP 384×384 → `pixelOffset` (offset 10) → flip vertical →
    pixels RGBA/BGRA (mesmo caminho do C++: `glTexImage2D` usa BGRA).
  - `get_sprite(sheet, sprite_id)` → copia o quad do sprite (col/row conforme
    `SPRITE_SHEET_WIDTH / size.width`) para `Sprites{ w, h, pixels }`.
- Critério: extrai um sprite conhecido e bate com o que o client exibe (check via
  `Dawnport.png` para os chãos).

## Fase 4 — Ligação para a tela (ground 32×32)

Fonte da verdade: `reference-src/source/map_drawer.cpp` (BlitItem/DrawTile),
`reference-src/source/gui.cpp` (ordem `ClientAssets::loadAppearanceProtobuf`
→ `g_items.loadFromProtobuf`).

- `editor_render::atlas` ganha API de **registrar sprite sob demanda**:
  `layer_for(sprite_id)` → cache `HashMap<sprite_id, u32 layer>`; novo sprite extraído
  via Fase 3 e copiado com `queue.write_texture` numa layer (atlas cresce).
- `scene.rs`: resolver `ground.type_id → sprite_id → layer` e montar `TileInstance`
  (v0: só ground; `layer_index = layer`; tint branco).
- `main.rs`: load do assets dir + do `.otbm`. **Assets dir e `.otbm` vem de
  constantes de debug apontando pro `reference-assets` e `reference-maps`** (menu
  Open real fica pra depois); câmera inicial centralizada no bbox do mapa.
- Critério: abrir `Dawnport.otbm` → chão (ground) do z=7 aparece em 2D.

## Fase 5 — Itens, profundidade e sprites maiores

- Extensão de `TileInstance` com `quad_size`/âncora para 32×64/64×32/64×64 e
  `drawHeight` (empilhamento em Z, `flags.height`).
- Desenhar `items` após o ground seguindo a ordem do RME (`alwaysOnBottom` por
  `alwaysOnTopOrder`, depois demais), com `pattern` `x%pw, y%ph, z%pd` e
  `stackable` buckets (já catalogados em `docs/OTBM.md` §8).
- `translucent`/alpha, formações de borda (`autoborder`) ficam para depois.

## Fase 6 — Robustez e arsenal

- Menu File → Open (.otbm) com dialog; salvamento não faz parte desta fase.
- Logs (console) das etapas de load (contagens, avisos), útil para debug.
- Testes unitários do parser OTBM e do protobuf com bytes sintéticos.

## Arquivos que tocam por fase

| Fase | Arquivos |
|---|---|
| 0 | `editor_formats/src/{lib,spr}.rs`, `main.rs`, `scene.rs` |
| 1 | `editor_formats/src/otbm.rs` (+ `Cargo.toml` sem deps novas) |
| 2 | `editor_formats/src/appearances.rs`, `editor_formats/src/pb.rs` |
| 3 | `editor_formats/src/{catalog,sprite}.rs`, deps: `serde_json`,`lzma-rs` |
| 4 | `editor_render/src/atlas.rs`, `scene.rs`, `main.rs`, `tabs.rs` |
| 5 | `editor_render/src/{instance,scene}.rs`, `pipeline.rs`, `shader.wgsl` |
| 6 | `main.rs`, docs |