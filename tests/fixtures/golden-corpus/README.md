# HOP-Claw Golden Corpus

Baseline outputs and Node sidecar timings captured from:
- `resources/claw-hwp/scripts/extract_text.js --inspect --with-cell-text`
- `resources/claw-hwp/scripts/extract_text.js --format markdown`
- `resources/claw-hwp/scripts/hwpx-edit.js` (`{ inspect: "cells" }`) for `.hwpx`
- Pre-order `TableWalker` table index mirror of `src/core/hwpxTableIndex.ts` for `.hwpx`

## Layout

- `manifest.json`: Index of all 20 corpus documents, file types, counts, output paths, and Node wall-clock timings (`node_inspect_ms`, `node_markdown_ms`).
- `fixtures/nested_depth2.hwpx`: Generated `.hwpx` fixture containing depth-1 and depth-2 nested tables (built via `hwpx-edit.js` from `samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx`).
- `outputs/<id>.inspect.json`: JSON output of `extract_text.js --inspect --with-cell-text`.
- `outputs/<id>.markdown.md`: Markdown output of `extract_text.js --format markdown`.
- `outputs/<id>.hwpx-tables.json`: Pre-order `HwpxParsedTable[]` matching `hwpxTableIndex.ts` (`.hwpx` only).
- `outputs/<id>.hwpx-cells.json`: Output of `hwpx-edit.js { inspect: "cells" }` (`.hwpx` only).
- `generate.mjs`: Reproducible Node generator script (`node tests/fixtures/golden-corpus/generate.mjs`).
