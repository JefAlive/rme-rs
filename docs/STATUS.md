# Status do Projeto RME-rs (Handoff)

## ✅ Concluído

### Fase 0 — Limpeza do legado
- `spr.rs` removido; `main.rs` limpo (atlas placeholder 4 cores, demo 16×16 removida);
- `scene.rs` com `sync(device, map, resolve_layer)`;
- `tabs.rs` resolver placeholder (`|type_id| type_id as u32`).

### Fase 1 — Parser OTBM
- `editor_formats/src/binary.rs`: tokens `NODE_START=0xFE`, `NODE_END=0xFF`, `ESCAPE_CHAR=0xFD`, `TreeNode` + `build_tree`.
- `editor_formats/src/otbm.rs`: parse completo (OTBM v1–v6), `OtmTile`/`OtmItem`/`OtmTown`, TILESTATE flags, `subtype_embedded` closure.
- Exemplo `otbm_info.rs`: valida Dawnport.otbm (versão 2 MAP_OTBM_3, 32194×31978, z=2..10, 1762 escapes reais).
- 3 testes unitários; clippy limpo.

### Fase 2 — appearances.dat (protobuf hand-rolled)
- `editor_formats/src/pb.rs`: Reader wire format (varint LEB128, tag, WT 0/1/2/5, `len_payload`, `skip`, `for_each_field`).
- `editor_formats/src/appearances.rs`: mensagens proto, `ItemType`/`SpriteMeta`/`ItemTypeTable`, `build_item_table` (espelha `items.cpp:456`), `load_appearances`/`parse_appearances`.
- `appearances_info.rs`: stats reais (43516 objetos, max id 55117, 3334 ground, 3375 container, 49 fluid, 12 splash, 5190 animados, 3691 com luz, 111959 sprite ids, parse+build ~78 ms).
- 3 testes unitários (packed/unpacked sprite_id, animação, truncamento); clippy limpo.

### Fase 3 — catalog-content.json + sheets `.bmp.lzma`
- `catalog.rs`: parse JSON (5084 sheets, sprites max id 301202), `SpriteLayout` (1x1/1x2/2x1/2x2), `sheet_for_sprite` (lower_bound em `last_id`).
- `sprite.rs`: header CIP (magic `70 0A FA 80 24`, varint 7-bit, lclppb, dict_size, cip size 8 bytes), LZMA1 raw via `lzma-rs` (lc=3,lp=0,pb=2, dict=32MB), BMP pixel offset +10, flip vertical (384×384 BGRA → RGBA 384×384).
- `sheet_info.rs`: decode + benchmark (~25ms/sheet, 23 MB/s), valida sprite 197909 (ground id=100).
- 7 testes totais (catalog 1 + appearances 3 + otbm 3); clippy limpo.

### Fase 4a — Bridge core (import OtbmDocument → MapDocument)
- `editor_core/src/import.rs`: `import_otbm(doc, table) → (MapDocument, Bounds)` com semântica fiel a `Tile::addItem` (ground sobrescreve, alwaysOnBottom ordenado, zone flags, house_id). `is_subtype_embedded` para OTBM v1.
- 3 testes unitários (ground+zone+house, ground overwrite + bottom order, subtype_embedded detection).
- Dependência `editor_formats` adicionada a `editor_core` e `editor_render`.

### ROADMAP.md atualizado
Reflete estado real: Fase 4 dividida em ponte core (concluída) + render/UI (pendente). Tabela de arquivos por fase atualizada.

---

## ⏳ Pendente (Fase 4b–4d — Exibir ground de Dawnport)

### editor_render
1. **`atlas.rs`** — já reescrito com `append(device, queue, &[u8;4096]) → u32` (cresce dinamicamente, bind_group regenerado). ✅
2. **`assets.rs`** (novo) — `SpriteResolver`:
   - Carrega `appearances.dat` + `catalog-content.json` do `assets_dir`.
   - Cache de sheets decodificadas (`AHashMap<u32, DecodedSheet>` onde key = `first_id`).
   - `layer_for(type_id) → u32`: `table.get_opt(type_id).sprite_ids.first()` → `catalog.sheet_for_sprite(spid)` → `decode_sheet` → extrai célula 32×32 conforme `SpriteLayout` → `atlas.append` → cache `sprite_id→layer`.
   - Conversão BGRA→RGBA na extração.
3. **`scene.rs`** — já aceita `resolve_layer: impl Fn(u16)->u32`; nada a mudar.

### editor_ui
4. **`tabs.rs`** — adicionar `sprite_resolver: Option<SpriteResolver>` + `camera_fit_pending: bool` ao `AppState`.
   - Em `ui_viewport`: closure real `resolver.layer_for(device, queue, atlas, type_id)`.
   - Camera fit: ao primeiro frame com tamanho real, centraliza no bbox do mapa (z=7), zoom para caber altura.

### editor_app
5. **`main.rs`** — constantes de debug:
   ```rust
   const ASSETS_DIR: &str = "reference-assets";
   const MAP_PATH: &str = "reference-maps/Dawnport.otbm";
   ```
   - Lê OTBM → `parse` com `is_subtype_embedded` do `import` → `import_otbm` → `MapDocument` + `Bounds`.
   - Carrega `SpriteResolver::load(ASSETS_DIR)` → `AppState`.
   - Cria `SpriteAtlas::new(device)`.
   - Remove `placeholder_sprites` + demo 16×16 hardcoded.
   - Inicializa câmera no bbox (offset/zoom) ou defere ao `tabs.rs` (camera_fit_pending).

### Critério de aceite Fase 4
Abrir `Dawnport.otbm` → chão (ground) do z=7 aparece em 2D usando sprites reais do atlas.

---

## Fases 5–6 (planejadas, não iniciadas)

| Fase | Foco |
|---|---|
| 5 | Itens maiores (2x1/1x2/2x2), drawHeight, ordem de draw RME, pattern x%pw, stackable buckets. |
| 6 | Menu File → Open, logs de load, testes unitários sintéticos OTBM/protobuf. |

---

## Comandos úteis

```bash
# Testes
cargo test -p editor_formats       # 7 testes (Fases 1-3)
cargo test -p editor_core          # 3 testes (import)

# Exemplos
cargo run -p editor_formats --example otbm_info -- reference-maps/Dawnport.otbm
cargo run -p editor_formats --example appearances_info -- reference-assets/appearances-*.dat
cargo run -p editor_formats --example sheet_info -- reference-assets

# Clippy
cargo clippy -p editor_formats --all-targets
cargo clippy -p editor_core --all-targets
cargo clippy -p editor_render --all-targets
```

---

## Referências no disco

```
reference-src/          # remeres-map-editor (C++)
reference-assets/       # appearances-*.dat, catalog-content.json, sprites-*.bmp.lzma
reference-maps/         # Dawnport.otbm, XMLs, Dawnport.png
```

---

## Próximo passo imediato

Implementar `editor_render/src/assets.rs` + conectar em `tabs.rs` + ajustar `main.rs`. Estimado: 4–6 arquivos novos/alterados.