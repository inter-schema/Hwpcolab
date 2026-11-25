# `hopclaw-hwpx` Porting Map (`hwpx-edit.js` -> Rust)

Source analyzed:
- `resources/claw-hwp/scripts/hwpx-edit.js` (5,167 lines)
- `src/types/operations.ts` (33 `UnifiedOp` variants)
- `src/core/opTranslator.ts` (`OpTranslator::toHwpxOps`, lines 489–893)

---

## 1. Architecture & Invariants (D-05 / D-06)

1. **ZIP Container & Untouched Entry Preservation**:
   - `mimetype` (`"application/hwp+zip"`) MUST always be entry #0 with `CompressionMethod::Stored` and no data descriptor (`is_valid_hwpx_package`).
   - Any ZIP entry not in `dirty` (e.g. `version.xml`, `settings.xml`, `META-INF/*`, `Preview/*`, untouched `BinData/*`, untouched `Contents/section*.xml`) is copied verbatim via `zip::ZipWriter::raw_copy_file` so its compressed payload and CRC32 remain byte-identical.
2. **Selective XML Rewriting (`quick-xml` 0.42)**:
   - Only modified XML parts (`Contents/sectionN.xml`, `Contents/header.xml`, `Contents/content.hpf`, or newly added `Chart/chartN.xml`) are parsed/rewritten using `quick-xml` streaming events, preserving unaffected sibling elements, attribute ordering, and namespace declarations on `<hs:sec>`, `<hh:head>`, `<opf:package>`.
3. **Cache & Counter Synchronization**:
   - Strip `<hp:linesegarray>...</hp:linesegarray>` whenever paragraph text, runs, styles, or table geometry change (forces clean relayout in Hancom Docs/Office).
   - Synchronize `itemCnt` on `<hh:borderFills>`, `<hh:charProperties>`, `<hh:paraProperties>`, `<hh:bullets>`, `<hh:numberings>` when new definitions are appended (and prefer placeholder `refCount == 0` reuse where `hwpx-edit.js` does).
   - Synchronize `secCnt` on `<hh:head>` if sections are added/removed.
4. **Canonical Table & Paragraph Indexing**:
   - `paragraphs()`: top-level `<hp:p>` across `Contents/sectionN.xml` (sorted numerically by section index), excluding paragraphs nested inside `<hp:tbl>`.
   - `tablesDeep()`: pre-order traversal of all `<hp:tbl>` elements (top-level and nested) matching `TableWalker` / `hwpxTableIndex.ts`.
5. **Blank Template (`assets/templates/blank.hwpx`)**:
   - Embedded via `include_bytes!("../../assets/templates/blank.hwpx")` (derived from Hancom-authored `hancom_native_stub.hwpx` with empty body paragraph and `PrvText.txt`, preserving native `header.xml`, `Scripts/`, `xmlns:hwpunitchar`, and valid `STORED` `mimetype`).
   - Also provides the native stub XML fragments (`paraPr id="2"/"3"`, `bullets`, `numberings`, `Scripts/*`) needed for `injectHancomListInfra` / `ensureCleanParaPr`.

---

## 2. Op-by-Op Port Map (`UnifiedOp` 33 -> `hwpx-edit.js` -> `HwpxEditPlan`)

### Group A: Body Text & Paragraph Operations
| `UnifiedOp` (33) | `hwpx-edit.js` op(s) | `hwpx-edit.js` handler | Rust target & key behavior |
|---|---|---|---|
| `replace_body_text` | `replace_text { find, replace }` | `opReplaceText` (L343) | Control-aware token splice inside `<hp:t>` (`fwSpace`/`tab`/`lineBreak`/`hyphen`/`nbSpace`/`titleMark`/`insertBegin`/`insertEnd` zero-width for matching) + Pass 2 cross-run splice across adjacent text runs; drop `linesegarray`. |
| `set_paragraph_text` | `set_paragraph_text { index, text }` | `opSetParagraphText` (L412) | Resolve top-level paragraph index (`-1`/out-of-bounds clamp to last), preserve `<hp:p>` attrs & first `charPrIDRef`, replace body with single `<hp:run><hp:t>`. |
| `append_paragraph` | `append_paragraph { text }` + optional `apply_paragraph_style { index: -1, target, align }` | `opAppendParagraph` (L424) | Clone last body `<hp:p>` attrs (fresh `id`) & `charPrIDRef`, append before `</hs:sec>` in last section; if `align` set, apply clean align `paraPr`. |
| `append_heading` | `append_paragraph { text }` + `apply_text_style { target, bold: true, size_pt }` | `opAppendParagraph` + `opApplyTextStyle` | Append paragraph + apply bold & heading size (`1->16pt, 2->14pt, 3->12pt, 4->11pt`, default `13pt`). |
| *(direct)* | `fill_template { values, require_unique }` | `opFillTemplate` (L369) | Loop `opReplaceText` per entry; reject if `require_unique` and any key matched > 1 spot. |
| *(direct)* | `delete_paragraph { index }` | `opDeleteParagraph` (L449) | Remove top-level `<hp:p>` at `index` (`0..len`). |
| *(direct)* | `set_page_break { index, on? }` | `opSetPageBreak` (L1757) | Set/toggle `pageBreak="0|1"` on `<hp:p>`. |
| *(direct)* | `set_column_break { index, on? }` | `opSetColumnBreak` (L1772) | Set/toggle `columnBreak="0|1"` on `<hp:p>`. |
| *(direct)* | `set_field_value { name, value }` | `opSetFieldValue` (L4497) | Replace `<hp:t>` between `<hp:fldBegin name="...">` and `<hp:fldEnd>`. |
| *(direct)* | `para_line { index, fill_color?, border_color?, border_width_mm?, sides? }` | `opParaLine` (L3174) | Replace paragraph with 1x1 callout table (full content width) + `set_cell_background` + `set_cell_border`. |

### Group B: Table Content & Structure Operations
| `UnifiedOp` (33) | `hwpx-edit.js` op(s) | `hwpx-edit.js` handler | Rust target & key behavior |
|---|---|---|---|
| `set_cell_text` | `set_cell_text { table, row, col, text, fit? }` | `opSetCellText` (L537) | Resolve table via `tablesDeep()`, locate cell by `<hp:cellAddr colAddr=...>` (positional fallback); if `fit`, splice into longest ` {2,}` padding run via `fitValueIntoLayout`; rewrite first `<hp:p>` in `<hp:subList>`. |
| `insert_table` | `insert_table { index, rows, cols, cells }` | `opInsertTable` (L1287) | Build `<hp:tbl>` by cloning `tables()[0]` (row 1 body cell preferred) or Hancom-verified fallback (`borderFillIDRef` invisible table + 0.12mm solid cell), fit page width, set `cellAddr`/`cellSpan="1"`, freshen IDs, insert after `afterParaIndex` (`-1` prepends). |
| `insert_nested_table` | `insert_nested_table { table, row, col, rows, cols, cells/data, headerRow?, widthMode? }` | `opInsertNestedTable` (L1355) | **HWPX only**: reject if target table `depth >= 2` (max depth <= 2) or `rows`/`cols` not in `1..=8`; fit cell content width (`fit_cell`); insert into empty last `<hp:p>` or append new `<hp:p>` inside target cell `<hp:subList>`, drop `linesegarray`. |
| `insert_table_row` | `insert_table_row { table, row, where: before\|after, cells? }` | `opInsertTableRow` (L618) | Clone reference row `row`, populate `cells`, insert `before`/`after`, normalize `rowAddr` across all rows, `rowCnt += 1`, freshen IDs. |
| *(direct)* | `append_table_row { table, cells? }` | `opAppendTableRow` (L581) | Clone last row, bump `rowAddr`, `rowCnt += 1`. |
| `delete_table_row` | `delete_table_row { table, row }` | `opDeleteTableRow` (L644) | Remove `<hp:tr>` at `row`, `rowCnt -= 1`. |
| `insert_table_col` | `insert_table_column { table, col, where: before\|after, cells? }` | `opInsertTableColumn` (L685) | For each row clone cell `col`, fill `cells[r]`, insert `before`/`after`, normalize `colAddr`, `colCnt += 1`, freshen IDs. |
| *(direct)* | `append_table_column { table, cells? }` | `opAppendTableColumn` (L655) | Append cloned last cell per row, bump `colAddr`, `colCnt += 1`. |
| `delete_table_col` | `delete_table_column { table, col }` | `opDeleteTableColumn` (L710) | Remove cell `col` per row, `colCnt -= 1`. |
| `merge_cells` | `merge_cells { table, mode: horizontal\|vertical, ... }` | `opMergeCells` (L729) | Rectangular merge expands into horizontal per-row merges (`colSpan`) + vertical merge (`rowSpan`), removing absorbed `<hp:tc>` elements and dropping `linesegarray`. |
| `unmerge_cells` | `unmerge_cells { table, row, col }` | `opUnmergeCells` (L799) | Restore merged cell (`colSpan > 1` or `rowSpan > 1`) into 1x1 grid cells inheriting shell/borderFill/cellMargin, dividing `cellSz` evenly. |
| `split_cell` | `split_cell { table, row, col, rows, cols }` | `opSplitCell` (L870) | Split unmerged 1x1 cell into `rows x cols` sub-cells; shift downstream `colAddr`/`rowAddr` and grow `colSpan`/`rowSpan` of spanning neighbors; bump `colCnt`/`rowCnt`. |
| `equalize_table` | `distribute_table { table, mode }` | `opDistributeTable` (L2480) | Average column widths (`width`), row heights (`height`), or `both` across `<hp:cellSz>`. |

### Group C: Table & Cell Styling / Sizing Operations
| `UnifiedOp` (33) | `hwpx-edit.js` op(s) | `hwpx-edit.js` handler | Rust target & key behavior |
|---|---|---|---|
| `set_cell_background` | `set_cell_background { table, row, col, color, mode: "both" }` | `opSetCellBackground` (L1566) | `cellzone` (`ensureBorderFill` with `winBrush` + `addCellzone` adjacent-zone merge) AND/OR `shade` (`applyCellShadeColor` via `charPr@shadeColor`). |
| `set_cell_align` | `set_cell_align { table, row, col, horizontal?, vertical? }` | `opSetCellAlign` (L1680) | Vertical -> `<hp:subList vertAlign="TOP\|CENTER\|BOTTOM">` (`MIDDLE`->`CENTER`); Horizontal -> clone/reuse `hh:paraPr` with `<hh:align horizontal="..."/>` on cell's first `<hp:p>`. Range `(row..=endRow, col..=endCol)` expands per cell. |
| `set_cell_border` | `set_cell_border { table, row, col, color, width, sides }` | `opSetCellBorder` (L1651) | `ensureBorderFill` with requested sides (`LEFT/RIGHT/TOP/BOTTOM`) + default `0.12 mm #000000` on other sides, then `addCellzone`. |
| *(direct)* | `set_cell_diagonal { table, row, col, direction, color?, width? }` | `opSetCellDiagonal` (L1665) | `ensureBorderFill` with `slash` or `backSlash` + `<hh:diagonal>`, then `addCellzone`. |
| `set_col_width` | `set_col_width { table, col, width_mm, keep_table_width? }` | `opSetColWidth` (L2499) | Update `<hp:cellSz width>` in col `col` across rows (if `keep_table_width`, compensate adjacent col clamped >= 5mm; else update table `<hp:sz width>`). |
| `set_row_height` | `set_row_height { table, row, height_mm }` | `opSetRowHeight` (L2553) | Update `<hp:cellSz height>` for all cells in `row`. |
| `set_cell_padding` | `set_cell_padding { table, row, col, left_mm?, right_mm?, top_mm?, bottom_mm?, margin_mm? }` | `opSetCellPadding` -> `opSetCellMargin` (L2303, L2567) | Set `hasMargin="1"` on `<hp:tc>` and update/append `<hp:cellMargin left/right/top/bottom>`. |
| `fit_table_to_page` | `fit_table_to_page { table, target_width_mm? }` | `opFitTableToPage` (L2576) | Scale column `<hp:cellSz width>` proportionally to printable page width (`pagePr.width - margin.left - margin.right`) or `target_width_mm`, absorbing rounding remainder in last col, and update `<hp:sz width>`. |
| `shrink_cell_font` | `shrink_cell_font { table, row, col, size_pt, min_pt? }` | `opShrinkCellFont` (L2627) | Clamp `max(min_pt.unwrap_or(8.0), size_pt)` and apply `apply_text_style` on cell's text. |
| *(direct)* | `set_cell_size`, `set_table_margin`, `set_table_inner_margin`, `set_table_size`, `set_table_props`, `set_title_cell`, `set_table_split_border` | L2261–L2473 | Direct table/cell attribute & `<hp:outMargin>`/`<hp:inMargin>`/`<hp:sz>`/`<hh:borderFill breakCellSeparateLine="1">` edits. |

### Group D: Text, Paragraph, List & Named Style Operations
| `UnifiedOp` (33) | `hwpx-edit.js` op(s) | `hwpx-edit.js` handler | Rust target & key behavior |
|---|---|---|---|
| `apply_text_style` | `apply_text_style { target, ... }` | `opApplyTextStyle` (L2771) | `highlight`: splice `<hp:markpenBegin color="..."/>...<hp:markpenEnd/>` inside `<hp:t>` (or strip on `false`). Other attrs (`size_pt` -> `height = pt*100`, `color`, `bold`, `italic`, `underline`, `strikethrough`, `supscript`, `subscript`, `letter_spacing`, `char_ratio`, `fontFace`): clone base `hh:charPr`, reuse `refCount==0` placeholder or append + bump `hh:charProperties@itemCnt`, split run if `target` is a substring. |
| `apply_paragraph_style` | `apply_paragraph_style { index, target?, align?, indent?, lineSpacing?, spacing_before?, spacing_after? }` | `opApplyParagraphStyle` (L3006) | Resolve body paragraph (or search inside table cells if `target` misses body); build web-safe `<hp:switch>` (`HwpUnitChar` `mm*100` / default `mm*200`) `hh:paraPr` preserving unspecified fields from current `paraPr`, or fallback clone; retarget `paraPrIDRef` & drop `linesegarray`. |
| `set_list_style` | `set_bullet_list` / `set_number_list` (+ `clear_list`) | `opSetParagraphList` (L2112) | Try `reuseExistingListParaPr`; else `injectHancomListInfra` from embedded stub (adds `paraPr`, `bullets`, `numberings`, `xmlns:hwpunitchar`, `Scripts/*`); else fallback text prefix / `stockizeParaPrInner` + `ensureBulletAsNumbering`/`ensureNumbering`. |
| *(direct)* | `apply_style { index, style }` | `opApplyStyle` (L3131) | Lookup `<hh:style>` by `id`/`name`/`engName`, retarget `styleIDRef`, `paraPrIDRef`, and run `charPrIDRef`. |

### Group E: Objects, Notes, Headers/Footers & Page Layout Operations
| `UnifiedOp` (33) | `hwpx-edit.js` op(s) | `hwpx-edit.js` handler | Rust target & key behavior |
|---|---|---|---|
| `insert_image` | `insert_image` or `set_cell_image` | `opInsertImage` (L3419) / `opSetCellImage` (L4016) | Embed binary into `BinData/imageN.<ext>`, add `<opf:item>` in `content.hpf`, build `<hp:pic>` (clone existing or schema-complete fallback). In-cell mode sets `treatAsChar="1"`, `CELL_OBJ_MARGIN` (`~1.4mm`), `fitToCell`, `CENTER/CENTER` align. |
| `place_seal` | `place_seal { anchor, source, mode? }` | `opPlaceSeal` (L3725) | Embed seal image, detect aspect ratio (PNG/JPEG/BMP header), find `occurrence`-th anchor match across free paragraphs (`PAGE` frame) & nested table cells (`PARA` frame via `sealDescend`), compute CJK/ASCII metric offset, insert floating `<hp:pic>` (`treatAsChar="0" flowWithText="0" allowOverlap="1" textWrap="IN_FRONT_OF_TEXT"`). |
| `set_header_footer` | `set_header` / `set_footer` (+ `remove_header`/`remove_footer`) | `opSetHeaderFooter` (L4213) / `opRemoveHeaderFooter` (L4308) | Update existing `<hp:header>`/`<hp:footer>` or insert `<hp:ctrl>` paragraph after section 0's first `<hp:p>`, optional clean align `paraPr`. |
| `insert_page_number` | `set_page_number { where?, align? }` | `opSetPageNumber` (L4281) | Insert header/footer `<hp:ctrl>` holding `<hp:autoNum num="1" numType="PAGE">`. |
| `insert_footnote` | `insert_footnote { index, anchor?, text }` (+ `insert_endnote`) | `opInsertNote` (L4342) | Append `<hp:run><hp:ctrl><hp:footNote\|endNote id="0">...</hp:ctrl></hp:run>` to target paragraph. |
| `set_columns` | `set_columns { count, gap_mm? }` | `opSetColumns` (L4579) | Update `<hp:colPr colCount="..." sameSz="1" sameGap="..."/>` in every section. |
| `set_page_margin` | `set_page_setup { ... }` | `opSetPageSetup` (L4607) | Update `<hp:pagePr width height>` (A3/A4/A5/B4/B5/Letter/Legal + portrait/landscape) and `<hp:margin left/right/top/bottom>` in every section. |
| *(direct)* | `replace_image`, `delete_image`, `delete_object`, `insert_hyperlink`, `insert_bookmark`, `insert_equation`, `insert_chart`, `insert_shape`, `insert_textbox`, `set_object_*`, `set_cell_shape`, `set_cell_chart`, `set_cell_equation`, `set_caption` | L3292–L5024 | Supported directly in `HwpxEditPlan` for full `hwpx-edit.js` parity (`Chart/chartN.xml` OOXML generation, `renumberParts`, etc.). |
