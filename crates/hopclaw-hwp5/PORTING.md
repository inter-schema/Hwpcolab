# hopclaw-hwp5 Porting Guide

Port of HWP 5.0 binary reading, patching, and creation logic from [resources/claw-hwp/scripts/cell-patch.js](../../resources/claw-hwp/scripts/cell-patch.js) (`11,379` lines) and [resources/claw-hwp/scripts/create.js](../../resources/claw-hwp/scripts/create.js) (`4,616` lines, edit/raw-patch paths) into pure Rust (`crates/hopclaw-hwp5`).

## 1. D-15 License Note

- `hopclaw` repository root `LICENSE` is **GPL-3.0-or-later**.
- [resources/claw-hwp/scripts/package.json](../../resources/claw-hwp/scripts/package.json) is `"private": true`, and `cell-patch.js`, `create.js`, `hwpx-edit.js`, `extract_text.js`, `cell-inspect.js` do not include an explicit standalone copyright/license header or `LICENSE` file (attributed in [resources/claw-hwp/scripts/README.md](../../resources/claw-hwp/scripts/README.md) to upstream `DoHyun468/claw-hwp`).
- Vendored libraries under `resources/claw-hwp/scripts/vendor/` have explicit permissive licenses (`cfb`: Apache-2.0, `fflate`: MIT, `rhwp`: MIT).
- **Action before Phase 2 release**: do not block implementation; confirm GPL-3.0-or-later compatibility / permission with the `claw-hwp` author before public release.

## 2. Rust Crate Architecture & Module Map

### Phase 0 (Format-Level Core)
| Module | Responsibility | JS Origin |
| --- | --- | --- |
| `src/error.rs` | `Hwp5Error` (`thiserror`) for CFB, FileHeader, deflate/inflate, malformed records, missing stream/table/cell, unsupported format (`UnsupportedFormat`) | ad-hoc `throw` / `{ error }` returns in `cell-patch.js` & `create.js` |
| `src/cfb_container.rs` | `Hwp5Container` wrapping `cfb::CompoundFile` (`0.14.0`): open/write CFB bytes without `Sh33tJ5`, read/write/remove/create storages & streams, parse/update `FileHeader` (`256`B, version `u32`, `flags` bit 0 compressed), raw deflate (level 9 first for Hancom Docs compatibility) & inflate via `flate2` | CFB parser/rewriter (`cell-patch.js:438..1055`), `deflateToFit` (`1007..1055`), `ensureDirSlot` / `rebuildChildTree` (`5890..6160`) |
| `src/record.rs` | HWP 5.0 binary record codec (`TagID(10) \| Level(10) \| Size(12)`, extended `0xFFF` + `u32`), `RawRecord`, `parse_records`, `serialize_records`, byte-identical round-trip invariants, canonical tag constants (`0x10..0x6F`) | `parseRecords` / `writeRec` / `serializeRecords` (`cell-patch.js:1..130`) |
| `src/docinfo.rs` | `DocInfo` stream model: `ID_MAPPINGS` (`0x11`, 64B and 72B safe sync), `BIN_DATA` (`0x12`), `FACE_NAME` (`0x13`, 7 script slots), `BORDER_FILL` (`0x14`, 1-based ID, solid fill + L/R/T/B borders + diagonal), `CHAR_SHAPE` (`0x15`, 0-based ID, 70B & 74B safe), `NUMBERING` (`0x17`, 1-based), `BULLET` (`0x18`, 1-based), `PARA_SHAPE` (`0x19`, 0-based ID, 46/54/58B read & 58B emit), `STYLE` (`0x1A`) | `parseIdMappings`, `syncIdMappings`, `findOrAddBorderFill*`, `findOrAddCharShape`, `findOrAddParaShape`, `addBinDataDefToDocInfo`, `resolveNoteStyle` |
| `src/section.rs` | `BodyText/SectionN` record tree traversal & paragraph cluster manipulation: `PARA_HEADER` (`0x42`, 22/24B, MSB `0x8000_0000` last-para normalization), `PARA_TEXT` (`0x43`, UTF-16LE + inline/extended control chars `1` vs `8` wchars + `0x000D` EOP), `PARA_CHAR_SHAPE` (`0x44`), `PARA_LINE_SEG` (`0x45`), `PARA_RANGE_TAG` (`0x46`), `CTRL_HEADER` (`0x47`), `LIST_HEADER` (`0x48`, 34/47B cell header & 16B note header), `PAGE_DEF` (`0x49`), `SHAPE_COMPONENT` (`0x4C`), `TABLE` (`0x4D`), table & cell enumeration | `locateCell`, `enumerateSectionTables`, `tableCellRecords`, `makeParaTextRecord`, `normalizeLastParaFlag`, `normalizeCellLastParaFlag`, `relevelCluster`, `pickFreshInstanceId` |
| `src/lib.rs` | Public Phase-0 & Phase-1 exports | — |

### Phase 1/2 (Executor & Operation Modules, post `[types-frozen]`)
| Module | Responsibility | JS Origin |
| --- | --- | --- |
| `src/table.rs` | Table insertion (pure-Rust `tbl ` cluster synthesis + `BorderFill` remap, no `rhwp`), cell text patching, alignment, padding, background, border/diagonal style, col width, row height, `fit_table_to_page`, `shrink_cell_font`, `equalize_table`, row/col insert/delete, cell merge/unmerge/split, cell/table property setters, `insert_nested_table` -> `E_OP_UNSUPPORTED_FORMAT` | `patchCellsInPlace`, `applyCellText`, `collapseTrailingEmptyCellParas`, `appendTableInPlace`, `insertTableRowInPlace`, `deleteTableRowInPlace`, `insertTableColInPlace`, `deleteTableColInPlace`, `mergeCellsInPlace`, `unmergeCellsInPlace`, `splitCellInPlace`, `applyCellStyleInPlace`, `applyCellPropertyInPlace`, `applyTablePropertyInPlace`, `setColWidthInPlace`, `setRowHeightInPlace`, `fitTableToPageInPlace`, `setCellPaddingInPlace`, `shrinkCellFontInPlace`, `equalizeTableInPlace` |
| `src/text.rs` | Global/scoped text replacement (`PARA_CHAR_SHAPE` shift, `PARA_HEADER` MSB preserved), paragraph append/heading/hr/insert-line, list numbering/bullet assignment | `replaceTextInPlace`, `appendParagraphInPlace`, `insertParaLineInPlace`, `applyListInPlace` |
| `src/style.rs` | Text style (`CHAR_SHAPE` + optional `PARA_RANGE_TAG` highlight/underline) and paragraph style (`PARA_SHAPE` + optional `BORDER_FILL` shading/frame) | `applyTextStyleInPlace`, `applyParagraphStyleInPlace` |
| `src/layout.rs` | Page size/orientation/margins (`PAGE_DEF` `0x49`), multi-column (`'dloc'`), header/footer, page numbering, footnote/endnote | `setupDocumentInPlace`, `setPageMarginInPlace`, `setColumnsInPlace`, `insertHeaderFooterTextInPlace`, `insertPageNumberInPlace`, `insertFootnoteInPlace`, `insertEndnoteInPlace` |
| `src/objects.rs` | Images & seals (`BinData/BIN000N.<ext>` + `$pic` + `CTRL_DATA`), OOXML chart (`OOXMLChartContents`), GSO shape/textbox/equation, bookmark/hyperlink/field, `apply_object_property` (`SHAPE_COMPONENT`), `delete_object` (`BinData` renumber + `PrvImage` invalidation) | `insertImageInPlace`, `placeSealInPlace`, `insertChartInPlace`, `insertShapeInPlace`, `insertTextboxInPlace`, `insertEquationInPlace`, `insertBookmarkInPlace`, `insertHyperlinkInPlace`, `insertFieldInPlace`, `applyObjectPropertyInPlace`, `deleteObjectInPlace` |
| `src/executor.rs` | `Hwp5EditPlan` / op dispatcher with `rhwp`-free coordinate resolution (`TableWalker`) and structured diff/error reporting | `create.js` `HANDLERS`, `cell-patch.js` `patchOps` |

## 3. Op-by-Op Port Map (`UnifiedOp` -> `create.js` / `cell-patch.js` -> `hopclaw-hwp5`)

All 33 `UnifiedOp` variants from [src/types/operations.ts](../../src/types/operations.ts) and [src/core/opTranslator.ts](../../src/core/opTranslator.ts):

| # | `UnifiedOp.type` | `create.js` / JSON `op` | `cell-patch.js` Origin | `hopclaw-hwp5` Target & Notes |
| --- | --- | --- | --- | --- |
| 1 | `replace_text` | `replace_text` (`create.js:2316`) | `replaceTextInPlace` (`1472..1612`) | `text::replace_text` (all / section / para / table cell, shifts `PARA_CHAR_SHAPE`) |
| 2 | `update_table_cell` | `fill_table` / `patch_cells` (`create.js:2284`) | `patchCellsInPlace` (`1057..1342`), `applyCellText` (`256..435`) | `table::update_cells` (single & multi-line `\n`, preserves/updates `PARA_CHAR_SHAPE`, collapses trailing empty paras) |
| 3 | `append_paragraph` | `add_paragraph` / `add_heading` / `add_hr` (`create.js:1866..1968`) | `appendParagraphInPlace` (`2398..2643`) | `text::append_paragraph` (22/24B `PARA_HEADER` matching doc, fresh `instance_id`, break-only `PARA_LINE_SEG`) |
| 4 | `insert_table` | `add_table` (`create.js:1975`) | `appendTableInPlace` (`1748..2396`) | `table::insert_table` (synthesizes table cluster in pure Rust, clones/allocates `BorderFill` + col widths) |
| 5 | `insert_nested_table` | *throws* (`opTranslator.ts:624`) | not supported in `.hwp` | Returns `Hwp5Error::UnsupportedFormat` (`E_OP_UNSUPPORTED_FORMAT`) per D-02 |
| 6 | `merge_cells` | `merge_cells` (`create.js:3384`) | `mergeCellsInPlace` (`10315..10618`) | `table::merge_cells` |
| 7 | `unmerge_cells` | `unmerge_cells` (`create.js:3415`) | `unmergeCellsInPlace` (`9804..9952`) | `table::unmerge_cells` |
| 8 | `split_cell` | `split_cell` (`create.js:3433`) | `splitCellInPlace` (`9573..9803`) | `table::split_cell` (vertical / horizontal / grid sub-splits) |
| 9 | `insert_table_row` | `insert_row` (`create.js:3454`) | `insertTableRowInPlace` (`10048..10164`) | `table::insert_row` (updates `TABLE` row count, clones adjacent row cluster & `row` indices) |
| 10 | `delete_table_row` | `delete_row` (`create.js:3481`) | `deleteTableRowInPlace` (`9488..9572`) | `table::delete_row` (recomputes `row` indices & `TABLE` metadata) |
| 11 | `insert_table_col` | `insert_col` (`create.js:3506`) | `insertTableColInPlace` (`10165..10265`) | `table::insert_col` (scales/assigns col width & inserts column cells) |
| 12 | `delete_table_col` | `delete_col` (`create.js:3533`) | `deleteTableColInPlace` (`10266..10314`) | `table::delete_col` (updates span/col indices & `TABLE` cols) |
| 13 | `set_cell_style` | `set_cell_style` (`create.js:3558`) | `applyCellStyleInPlace` (`10619..10847`) | `table::set_cell_style` (fill color, borders, diagonal, merges existing `BORDER_FILL` attributes) |
| 14 | `set_cell_property` | `set_cell_property` (`create.js:3186`) | `applyCellPropertyInPlace` (`8505..8800`) | `table::set_cell_property` (width, height, margins, valign, halign, header row flag) |
| 15 | `set_table_property` | `set_table_property` (`create.js:3227`) | `applyTablePropertyInPlace` (`8802..8968`) | `table::set_table_property` (outMargin, wrap/pos, `page_split`, header repeat) |
| 16 | `set_col_width` | `set_col_width` (`create.js:3591`) | `setColWidthInPlace` (`10848..10945`) | `table::set_col_width` |
| 17 | `set_row_height` | `set_row_height` (`create.js:3611`) | `setRowHeightInPlace` (`10946..11015`) | `table::set_row_height` |
| 18 | `equalize_table` | `equalize_table` (`create.js:3360`) | `equalizeTableInPlace` (`9153..9337`) | `table::equalize_table` |
| 19 | `fit_table_to_page` | `fit_table_to_page` (`create.js:3340`) | `fitTableToPageInPlace` (`11016..11108`) | `table::fit_table_to_page` (reads `PAGE_DEF` usable width) |
| 20 | `set_cell_padding` | `set_cell_padding` (`create.js:3629`) | `setCellPaddingInPlace` (`11109..11160`) | `table::set_cell_padding` |
| 21 | `shrink_cell_font` | `shrink_cell_font` (`create.js:3651`) | `shrinkCellFontInPlace` (`11216..11379`) | `table::shrink_cell_font` (font size step-down until `fitValueIntoLayout` fits cell) |
| 22 | `apply_text_style` | `set_text_style` (`create.js:3043`) | `applyTextStyleInPlace` (`7172..7891`) | `style::apply_text_style` (`CHAR_SHAPE` + optional `PARA_RANGE_TAG`) |
| 23 | `apply_paragraph_style` | `set_paragraph_style` (`create.js:3086`) | `applyParagraphStyleInPlace` (`7892..8249`) | `style::apply_paragraph_style` (`PARA_SHAPE` + optional `BORDER_FILL`) |
| 24 | `apply_list` | `set_list` (`create.js:3146`) | `applyListInPlace` (`8250..8504`) | `text::apply_list` (`NUMBERING` / `BULLET` + `PARA_SHAPE` refs) |
| 25 | `setup_document` | `setup_document` (`create.js:3648..3720`) | `setupDocumentInPlace` (`1613..1747`) | `layout::setup_document` (`PAGE_DEF` size/orientation/margins; fixes `create.js` truthy `"portrait"` bug!) |
| 26 | `set_page_margin` | `set_page_margin` (`create.js:3675`) | `setPageMarginInPlace` (`11161..11215`) | `layout::set_page_margin` (`PAGE_DEF` margin offsets) |
| 27 | `set_columns` | `set_columns` (`create.js:2447`) | `setColumnsInPlace` (`3892..4060`) | `layout::set_columns` (`'dloc'` `CTRL_HEADER`) |
| 28 | `set_header_footer` | `add_header_footer` (`create.js:2481`) | `insertHeaderFooterTextInPlace` (`4061..4335`) | `layout::set_header_footer` (`'daeh'` / `'toof'` clusters) |
| 29 | `set_page_number` | `add_page_number` (`create.js:2412`) | `insertPageNumberInPlace` (`3757..3891`) | `layout::set_page_number` (`'pgnp'` / `'#pg '` controls) |
| 30 | `insert_note` | `add_footnote` / `add_endnote` (`create.js:2356`) | `insertFootnoteInPlace` / `insertEndnoteInPlace` (`3572..3756`) | `layout::insert_note` (`'nf  '` / `'ne  '` + note `STYLE` + `LIST_HEADER` 16B) |
| 31 | `insert_image` | `add_image` (`create.js:2063`) / `place_seal` (`create.js:2643`) | `insertImageInPlace` (`4336..4717`) / `placeSealInPlace` (`4718..4853`) | `objects::insert_image` / `place_seal` (`BinData/BIN000N.<ext>` + `BIN_DATA` + `$pic`) |
| 32 | `insert_object` | `add_chart` (`2728`) / `add_shape` (`2792`) / `add_equation` (`2859`) / `add_textbox` (`2921`) / `add_bookmark` (`2963`) / `add_hyperlink` (`2985`) / `add_field` (`3016`) | `insertChartInPlace`, `insertShapeInPlace`, `insertEquationInPlace`, `insertTextboxInPlace`, `insertBookmarkInPlace`, `insertHyperlinkInPlace`, `insertFieldInPlace` (`3070..5880`, `6200..7170`) | `objects::insert_object` (dispatches by object kind: chart / shape / equation / textbox / bookmark / hyperlink / field) |
| 33 | `modify_or_delete_object` | `set_object_property` (`create.js:3270`) / `delete_object` (`create.js:3310`) | `applyObjectPropertyInPlace` (`8969..9152`) / `deleteObjectInPlace` (`5125..5310`) | `objects::set_object_property` & `objects::delete_object` (`BinData` compaction + `PrvImage` invalidation) |

### Additional Raw-Patch Helpers in `cell-patch.js`
- `insertParaLineInPlace` (`2644..2915`): splits/inserts paragraph line into an existing paragraph or cell (`table::insert_para_line` / `text`).
- `patchViaSheetjs` (`1343..1471`): in-place CFB rewrite fallback — superseded cleanly by `cfb::CompoundFile` (`cfb_container.rs`).

## 4. Critical Format Invariants Verified on Corpus Samples

Verified across `samples/demo_official_report.hwp`, `samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp`, and `third_party/rhwp/rhwp-studio/public/samples/*.hwp`:
1. **Unit scale**: `1 mm = 7200 / 25.4 ≈ 283.465 HWPUNIT`.
2. **Colors**: HWP 5.0 uses little-endian `0x00BBGGRR` (`rgb(r, g, b)` -> `r | (g << 8) | (b << 16)`).
3. **`ID_MAPPINGS` (`0x11`)**: both **64-byte** (`16 × u32`) and **72-byte** (`18 × u32`) payloads exist in real documents; preserve payload length while syncing counts at fixed indices `0..15`.
4. **Record lengths vary by HWP generator version**:
   - `CHAR_SHAPE` (`0x15`): **70 bytes** (older) or **74 bytes** (modern) — match existing document's length.
   - `PARA_SHAPE` (`0x19`): **46, 54, or 58 bytes** (read dynamically, emit 58B or matching existing).
   - `BORDER_FILL` (`0x14`): **1-based** ID (`0` = none), 40/47/53/71B variants in corpus (solid fill synthesized as 53B).
   - `PARA_HEADER` (`0x42`): **22 or 24 bytes** — preserve exact length and MSB `0x8000_0000` last-paragraph flag (must ONLY be set on the final paragraph of a section or cell `LIST_HEADER` subtree).
   - `LIST_HEADER` (`0x48`): **34 or 47 bytes** inside level-2 table cells; **16 bytes** inside footnotes/endnotes/headers/footers.
5. **Deflate compatibility**: use **raw deflate at level 9** (`flate2::Compression::best()`) for compressed `DocInfo` and `BodyText/SectionN`, and never write SheetJS's `Sh33tJ5` marker stream.
