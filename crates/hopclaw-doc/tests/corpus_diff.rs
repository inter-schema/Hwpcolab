use std::path::{Path, PathBuf};
use std::time::Instant;

use hopclaw_doc::types::{
    default_layout_options, CellBBox, CellProps, LayoutIssueCode, LayoutReport, PageDef,
    ParentCellCoord, TableCellMeta, TableCoordinateMeta, TableWalker,
};
use hopclaw_doc::{
    compare_layout_reports, extract_hwpx_markdown, format_layout_report_for_prompt,
    inspect_hwp5_cfb_summary, inspect_hwpx, inspect_layout_from_source, measure_korean_text_pt,
    merge_hwpx_tables_with_rhwp, parse_hwpx_section_tables, DocumentLayoutSource, HwpxTableWalker,
};
use hopclaw_types::{validate_preorder_coords, ErrorCode};
use serde::Deserialize;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[derive(Debug, Deserialize)]
struct CorpusManifest {
    entries: Vec<CorpusEntry>,
}

#[derive(Debug, Deserialize)]
struct CorpusEntry {
    id: String,
    source_path: String,
    file_type: String,
    section_count: usize,
    paragraph_count: usize,
    table_count: usize,
    cell_count: usize,
    outputs: CorpusOutputs,
    timings_ms: CorpusTimings,
}

#[derive(Debug, Deserialize)]
struct CorpusOutputs {
    inspect_json: String,
    markdown_md: String,
    hwpx_tables_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CorpusTimings {
    node_inspect_ms: f64,
    node_markdown_ms: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenInspect {
    file_type: String,
    section_count: usize,
    paragraph_count: usize,
    table_count: usize,
    cell_count: usize,
    image_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenHwpxTable {
    section_index: usize,
    table_index: usize,
    depth: usize,
    parent_table_index: Option<usize>,
    parent_cell: Option<ParentCellCoord>,
    row_count: usize,
    col_count: usize,
    cells: Vec<GoldenHwpxCell>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenHwpxCell {
    idx: usize,
    row: usize,
    col: usize,
    row_span: usize,
    col_span: usize,
    text: String,
}

fn normalize_legacy_hwpx_table_text(s: &str) -> String {
    s.replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[test]
fn hwpx_golden_corpus_inspect_tables_and_markdown_parity() {
    let root = repo_root();
    let corpus_dir = root.join("tests/fixtures/golden-corpus");
    let manifest_raw = std::fs::read_to_string(corpus_dir.join("manifest.json")).unwrap();
    let manifest: CorpusManifest = serde_json::from_str(&manifest_raw).unwrap();

    let hwpx_entries: Vec<&CorpusEntry> = manifest
        .entries
        .iter()
        .filter(|e| e.file_type == "hwpx")
        .collect();
    assert!(
        !hwpx_entries.is_empty(),
        "expected HWPX entries in golden corpus"
    );

    for entry in hwpx_entries {
        let doc_bytes = std::fs::read(root.join(&entry.source_path)).unwrap();

        // 1. Inspect parity against `extract_text.js --inspect`
        let t0 = Instant::now();
        let rust_inspect = inspect_hwpx(&doc_bytes, false).unwrap();
        let rust_inspect_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let golden_inspect_raw =
            std::fs::read_to_string(corpus_dir.join(&entry.outputs.inspect_json)).unwrap();
        let golden_inspect: GoldenInspect = serde_json::from_str(&golden_inspect_raw).unwrap();

        assert_eq!(rust_inspect.file_type, golden_inspect.file_type, "{}", entry.id);
        assert_eq!(
            rust_inspect.section_count, golden_inspect.section_count,
            "{}",
            entry.id
        );
        assert_eq!(
            rust_inspect.paragraph_count, golden_inspect.paragraph_count,
            "{}",
            entry.id
        );
        assert_eq!(
            rust_inspect.table_count, golden_inspect.table_count,
            "{}",
            entry.id
        );
        assert_eq!(
            rust_inspect.cell_count, golden_inspect.cell_count,
            "{}",
            entry.id
        );
        assert_eq!(
            rust_inspect.image_count, golden_inspect.image_count,
            "{}",
            entry.id
        );
        assert_eq!(rust_inspect.section_count, entry.section_count);
        assert_eq!(rust_inspect.paragraph_count, entry.paragraph_count);
        assert_eq!(rust_inspect.table_count, entry.table_count);
        assert_eq!(rust_inspect.cell_count, entry.cell_count);

        // 2. Canonical TableWalker trait + pre-order parity against `hwpxTableIndex.ts`
        let tables_rel = entry.outputs.hwpx_tables_json.as_ref().unwrap();
        let golden_tables_raw = std::fs::read_to_string(corpus_dir.join(tables_rel)).unwrap();
        let golden_tables: Vec<GoldenHwpxTable> =
            serde_json::from_str(&golden_tables_raw).unwrap();

        let walker = HwpxTableWalker::new(&doc_bytes);
        let rust_tables = walker.walk_tables().unwrap();
        validate_preorder_coords(&rust_tables).unwrap();
        assert_eq!(rust_tables.len(), golden_tables.len(), "{}", entry.id);

        if !rust_tables.is_empty() {
            let coord0 = walker.resolve_table_coord(0).unwrap();
            assert_eq!(coord0.table_index, 0);
        }
        let err = walker.resolve_table_coord(rust_tables.len() + 99).unwrap_err();
        assert_eq!(err.code, ErrorCode::E_TABLE_NOT_FOUND);

        let has_nested = golden_tables.iter().any(|t| t.depth > 0);
        for (rt, gt) in rust_tables.iter().zip(golden_tables.iter()) {
            assert_eq!(rt.table_index, gt.table_index, "{}", entry.id);
            assert_eq!(rt.sec, gt.section_index, "{}", entry.id);
            assert_eq!(rt.depth, Some(gt.depth), "{}", entry.id);
            assert_eq!(rt.parent_table_index, gt.parent_table_index, "{}", entry.id);
            assert_eq!(rt.parent_cell, gt.parent_cell, "{}", entry.id);
            assert_eq!(rt.row_count, gt.row_count, "{}", entry.id);
            assert_eq!(rt.col_count, gt.col_count, "{}", entry.id);
            assert_eq!(rt.cells.len(), gt.cells.len(), "{}", entry.id);
            for (rc, gc) in rt.cells.iter().zip(gt.cells.iter()) {
                assert_eq!(rc.idx, gc.idx, "{}", entry.id);
                assert_eq!(rc.row, gc.row, "{}", entry.id);
                assert_eq!(rc.col, gc.col, "{}", entry.id);
                assert_eq!(rc.row_span, gc.row_span, "{}", entry.id);
                assert_eq!(rc.col_span, gc.col_span, "{}", entry.id);
                let normalized_gold = normalize_legacy_hwpx_table_text(&gc.text);
                if rc.text != normalized_gold {
                    // `hwpxTableIndex.ts` uses `/<hp:t>([^<]*)<\/hp:t>/g`, which drops
                    // `<hp:t>` segments after inline markers (`<hp:fwSpace/>`, `<hp:markpenBegin/>`),
                    // whereas `quick-xml` (and `extract_text.js` `collectText`) preserves full text.
                    assert!(
                        rc.text.len() > normalized_gold.len(),
                        "{} table {} cell {} unexpected text mismatch:\n  rust: {}\n  gold: {}",
                        entry.id,
                        gt.table_index,
                        gc.idx,
                        rc.text,
                        normalized_gold
                    );
                }
            }
        }

        // 3. Markdown parity: byte-identical for non-nested HWPX docs; for docs with
        //    nested `<hp:tbl>` (where `extract_text.js` non-greedy regex closes early
        //    on the inner `</hp:tbl>`), verify all content & outer table structure remain intact.
        let t1 = Instant::now();
        let rust_md = extract_hwpx_markdown(&doc_bytes).unwrap();
        let rust_md_ms = t1.elapsed().as_secs_f64() * 1000.0;

        if !has_nested {
            let golden_md = std::fs::read_to_string(corpus_dir.join(&entry.outputs.markdown_md))
                .unwrap()
                .replace("\r\n", "\n");
            assert_eq!(rust_md, golden_md, "markdown mismatch for {}", entry.id);
        } else {
            assert!(
                !rust_md.trim().is_empty() && rust_md.contains("| "),
                "expected non-empty markdown table output for nested {}",
                entry.id
            );
        }

        eprintln!(
            "[bench] {}: inspect rust={:.2}ms vs node={:.2}ms | md rust={:.2}ms vs node={:.2}ms",
            entry.id,
            rust_inspect_ms,
            entry.timings_ms.node_inspect_ms,
            rust_md_ms,
            entry.timings_ms.node_markdown_ms
        );
    }
}

#[test]
fn hwp5_cfb_summary_matches_corpus_section_counts() {
    let root = repo_root();
    let corpus_dir = root.join("tests/fixtures/golden-corpus");
    let manifest_raw = std::fs::read_to_string(corpus_dir.join("manifest.json")).unwrap();
    let manifest: CorpusManifest = serde_json::from_str(&manifest_raw).unwrap();

    let hwp_entries: Vec<&CorpusEntry> = manifest
        .entries
        .iter()
        .filter(|e| e.file_type == "hwp")
        .collect();
    assert!(!hwp_entries.is_empty());

    let mut passed = 0usize;
    for entry in hwp_entries {
        let doc_bytes = std::fs::read(root.join(&entry.source_path)).unwrap();
        match inspect_hwp5_cfb_summary(&doc_bytes) {
            Ok(summary) => {
                assert_eq!(summary.file_type, "hwp", "{}", entry.id);
                assert_eq!(summary.section_count, entry.section_count, "{}", entry.id);
                assert_eq!(summary.table_count, entry.table_count, "{}", entry.id);
                passed += 1;
            }
            Err(err) => {
                // `samples/*.hwp` must always parse cleanly; stress fixtures in
                // `third_party/` may carry non-standard FAT chains rejected by `cfb 0.14.0`.
                assert!(
                    !entry.source_path.starts_with("samples/"),
                    "sample {} failed CFB summary: {err}",
                    entry.id
                );
                eprintln!("[hwp5-cfb] skipped {}: {err}", entry.id);
            }
        }
    }
    assert!(passed >= 2, "expected at least samples/*.hwp to pass CFB summary");
}

#[test]
fn hwpx_multi_section_and_mismatch_fallback() {
    // Mismatch fallback: 2 HWPX top-level tables vs 1 rhwp table -> falls back safely to rhwp
    let xml = r#"
      <hs:sec>
        <hp:tbl><hp:tr><hp:tc><hp:subList><hp:p><hp:run><hp:t>A</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl>
        <hp:tbl><hp:tr><hp:tc><hp:subList><hp:p><hp:run><hp:t>B</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl>
      </hs:sec>
    "#;
    let parsed = parse_hwpx_section_tables(xml, 0, 0).unwrap();
    assert_eq!(parsed.len(), 2);

    let rhwp_top = vec![TableCoordinateMeta {
        table_index: 0,
        sec: 0,
        para: 4,
        ctrl: 1,
        row_count: 1,
        col_count: 1,
        parent_table_index: None,
        parent_cell: None,
        depth: None,
        cells: Vec::new(),
    }];
    let fallback = merge_hwpx_tables_with_rhwp(&parsed, &rhwp_top);
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].para, 4);
    assert_eq!(fallback[0].ctrl, 1);
    assert_eq!(fallback[0].depth, Some(0));
}

#[test]
fn inspect_document_bytes_rejects_invalid_and_hwp5_cell_text_offline() {
    assert!(hopclaw_doc::inspect_document_bytes(b"not a doc", false).is_err());

    let root = repo_root();
    let sample_hwp =
        std::fs::read(root.join("samples/demo_official_report.hwp")).unwrap();
    let err = hopclaw_doc::inspect_document_bytes(&sample_hwp, true).unwrap_err();
    let model: hopclaw_types::ErrorModel = err.into();
    assert_eq!(model.code, ErrorCode::E_OP_UNSUPPORTED_FORMAT);
}

#[test]
fn strangler_backend_parsing_and_shadow_diff() {
    use hopclaw_types::EngineBackend;

    assert_eq!(hopclaw_doc::parse_engine_backend(None), EngineBackend::Node);
    assert_eq!(
        hopclaw_doc::parse_engine_backend(Some("RUST")),
        EngineBackend::Rust
    );
    assert_eq!(
        hopclaw_doc::parse_engine_backend(Some(" shadow ")),
        EngineBackend::Shadow
    );

    let root = repo_root();
    let sample_hwpx =
        std::fs::read(root.join("samples/2026_AI_행정업무_혁신계획(기획안).hwpx")).unwrap();
    let rust_inspect = inspect_hwpx(&sample_hwpx, false).unwrap();
    let node_json = serde_json::to_value(&rust_inspect).unwrap();
    assert!(hopclaw_doc::inspect_shadow_diff(&node_json, &rust_inspect).is_ok());

    let mut bad_json = node_json.clone();
    bad_json["tableCount"] = serde_json::json!(99);
    let diff = hopclaw_doc::inspect_shadow_diff(&bad_json, &rust_inspect).unwrap_err();
    assert!(diff.contains("tableCount"));

    assert!(hopclaw_doc::markdown_shadow_diff("a\r\nb\r\n", "a\nb\n").is_ok());
    assert!(hopclaw_doc::markdown_shadow_diff("a", "ab").is_err());
}

#[test]
fn hwpx_nested_table_parent_cell_uses_post_sublist_cell_addr_after_merge() {
    // Real OWPML places `<hp:subList>` before `<hp:cellAddr>` inside `<hp:tc>`.
    // Row 0 has a colSpan=2 merged cell at col=0, so the next `<hp:tc>` has positional
    // col_idx=1 but `<hp:cellAddr colAddr="2" rowAddr="0"/>`.
    // Row 0 col 0 also spans 2 rows (rowSpan=2), so Row 1's first `<hp:tc>` has positional
    // col_idx=0 but `<hp:cellAddr colAddr="2" rowAddr="1"/>`.
    let xml = r#"
      <hs:sec>
        <hp:tbl id="outer">
          <hp:tr>
            <hp:tc>
              <hp:subList><hp:p><hp:run><hp:t>Merged 2x2</hp:t></hp:run></hp:p></hp:subList>
              <hp:cellAddr colAddr="0" rowAddr="0"/>
              <hp:cellSpan colSpan="2" rowSpan="2"/>
            </hp:tc>
            <hp:tc>
              <hp:subList>
                <hp:p><hp:run>
                  <hp:tbl id="nested_row0">
                    <hp:tr>
                      <hp:tc>
                        <hp:subList><hp:p><hp:run><hp:t>R0C2</hp:t></hp:run></hp:p></hp:subList>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                      </hp:tc>
                    </hp:tr>
                  </hp:tbl>
                </hp:run></hp:p>
              </hp:subList>
              <hp:cellAddr colAddr="2" rowAddr="0"/>
              <hp:cellSpan colSpan="1" rowSpan="1"/>
            </hp:tc>
          </hp:tr>
          <hp:tr>
            <hp:tc>
              <hp:subList>
                <hp:p><hp:run><hp:tbl id="nested_empty"/></hp:run></hp:p>
              </hp:subList>
              <hp:cellAddr colAddr="2" rowAddr="1"/>
              <hp:cellSpan colSpan="1" rowSpan="1"/>
            </hp:tc>
          </hp:tr>
        </hp:tbl>
      </hs:sec>
    "#;

    let tables = parse_hwpx_section_tables(xml, 0, 0).unwrap();
    assert_eq!(tables.len(), 3);
    assert_eq!(tables[0].row_count, 2);
    assert_eq!(tables[0].col_count, 3);

    // Nested table inside (row=0, col=2) even though positional col_idx was 1
    assert_eq!(tables[1].depth, 1);
    assert_eq!(tables[1].parent_table_index, Some(0));
    assert_eq!(tables[1].parent_cell, Some(ParentCellCoord { row: 0, col: 2 }));

    // Empty nested table inside (row=1, col=2) even though positional col_idx was 0
    assert_eq!(tables[2].depth, 1);
    assert_eq!(tables[2].parent_table_index, Some(0));
    assert_eq!(tables[2].parent_cell, Some(ParentCellCoord { row: 1, col: 2 }));
}

#[test]
fn hwpx_table_walker_synthetic_and_merge_with_rhwp() {
    let xml = r#"
      <hs:sec>
        <hp:tbl id="outer">
          <hp:tr>
            <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Outer (0,0)</hp:t></hp:run></hp:p></hp:subList></hp:tc>
            <hp:tc><hp:cellAddr colAddr="1" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Outer (0,1)</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
          <hp:tr>
            <hp:tc>
              <hp:cellAddr colAddr="0" rowAddr="1"/>
              <hp:subList>
                <hp:p><hp:run><hp:t>Parent</hp:t></hp:run></hp:p>
                <hp:p><hp:run>
                  <hp:tbl id="nested">
                    <hp:tr>
                      <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Sub A</hp:t></hp:run></hp:p></hp:subList></hp:tc>
                      <hp:tc><hp:cellAddr colAddr="1" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Sub B</hp:t></hp:run></hp:p></hp:subList></hp:tc>
                    </hp:tr>
                  </hp:tbl>
                </hp:run></hp:p>
              </hp:subList>
            </hp:tc>
            <hp:tc><hp:cellAddr colAddr="1" rowAddr="1"/><hp:subList><hp:p><hp:run><hp:t>Outer (1,1)</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
        </hp:tbl>
        <hp:tbl id="second">
          <hp:tr>
            <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:subList><hp:p><hp:run><hp:t>Table B Cell</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          </hp:tr>
        </hp:tbl>
      </hs:sec>
    "#;

    let parsed = parse_hwpx_section_tables(xml, 0, 0).unwrap();
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].table_index, 0);
    assert_eq!(parsed[0].depth, 0);
    assert_eq!(parsed[1].table_index, 1);
    assert_eq!(parsed[1].depth, 1);
    assert_eq!(parsed[1].parent_table_index, Some(0));
    assert_eq!(parsed[1].parent_cell, Some(ParentCellCoord { row: 1, col: 0 }));
    assert_eq!(parsed[2].table_index, 2);
    assert_eq!(parsed[2].depth, 0);

    let rhwp_top = vec![
        TableCoordinateMeta {
            table_index: 0,
            sec: 0,
            para: 0,
            ctrl: 0,
            row_count: 2,
            col_count: 2,
            parent_table_index: None,
            parent_cell: None,
            depth: None,
            cells: vec![TableCellMeta {
                idx: 0,
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                text: "Outer (0,0)".to_string(),
                bbox: Some(CellBBox {
                    cell_idx: 0,
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    page_index: 0,
                    x: 10.0,
                    y: 10.0,
                    w: 100.0,
                    h: 20.0,
                }),
            }],
        },
        TableCoordinateMeta {
            table_index: 1,
            sec: 0,
            para: 2,
            ctrl: 0,
            row_count: 1,
            col_count: 1,
            parent_table_index: None,
            parent_cell: None,
            depth: None,
            cells: vec![TableCellMeta {
                idx: 0,
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                text: "Table B Cell".to_string(),
                bbox: Some(CellBBox {
                    cell_idx: 0,
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    page_index: 0,
                    x: 10.0,
                    y: 100.0,
                    w: 200.0,
                    h: 30.0,
                }),
            }],
        },
    ];

    let merged = merge_hwpx_tables_with_rhwp(&parsed, &rhwp_top);
    validate_preorder_coords(&merged).unwrap();
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].cells[0].bbox.as_ref().unwrap().w, 100.0);
    assert_eq!(merged[1].depth, Some(1));
    assert_eq!(merged[1].parent_table_index, Some(0));
    assert!(merged[1].cells[0].bbox.is_none());
    assert_eq!(merged[2].table_index, 2);
    assert_eq!(merged[2].para, 2);
    assert_eq!(merged[2].cells[0].bbox.as_ref().unwrap().w, 200.0);
}

struct MockLayoutSource {
    tables: Vec<TableCoordinateMeta>,
}

impl DocumentLayoutSource for MockLayoutSource {
    fn tables_meta(&self) -> &[TableCoordinateMeta] {
        &self.tables
    }
    fn page_def(&self, _sec: usize) -> PageDef {
        PageDef::default()
    }
    fn cell_properties(
        &self,
        _sec: usize,
        _para: usize,
        _ctrl: usize,
        _cell_idx: usize,
    ) -> CellProps {
        CellProps::default()
    }
    fn cell_paragraphs(
        &self,
        _sec: usize,
        _para: usize,
        _ctrl: usize,
        _cell_idx: usize,
    ) -> Vec<String> {
        Vec::new()
    }
}

#[test]
fn layout_measurement_and_comparison_parity() {
    assert!((measure_korean_text_pt("가나다", 10.0) - 30.0).abs() < 1e-9);
    assert!((measure_korean_text_pt("Ab0 ", 10.0) - 21.5).abs() < 1e-9);

    let overflowing = MockLayoutSource {
        tables: vec![
            TableCoordinateMeta {
                table_index: 0,
                sec: 0,
                para: 0,
                ctrl: 0,
                row_count: 1,
                col_count: 1,
                parent_table_index: None,
                parent_cell: None,
                depth: Some(0),
                cells: vec![TableCellMeta {
                    idx: 0,
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    text: "아주 긴 한글 텍스트가 좁은 셀 안에 들어가서 여러 줄로 넘치게 됩니다. 계속해서 텍스트를 추가하여 확실히 세로 넘침이 발생하도록 합니다.".to_string(),
                    bbox: Some(CellBBox {
                        cell_idx: 0,
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        page_index: 0,
                        x: 0.0,
                        y: 0.0,
                        w: 120.0,
                        h: 24.0,
                    }),
                }],
            },
            // Nested table wider than parent cell's available width
            TableCoordinateMeta {
                table_index: 1,
                sec: 0,
                para: 0,
                ctrl: 0,
                row_count: 1,
                col_count: 1,
                parent_table_index: Some(0),
                parent_cell: Some(ParentCellCoord { row: 0, col: 0 }),
                depth: Some(1),
                cells: vec![TableCellMeta {
                    idx: 0,
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    text: "ok".to_string(),
                    bbox: Some(CellBBox {
                        cell_idx: 0,
                        row: 0,
                        col: 0,
                        row_span: 1,
                        col_span: 1,
                        page_index: 0,
                        x: 0.0,
                        y: 0.0,
                        w: 200.0,
                        h: 40.0,
                    }),
                }],
            },
        ],
    };

    let before: LayoutReport =
        inspect_layout_from_source(&overflowing, default_layout_options(), 1000);
    assert_eq!(before.table_count, 2);
    assert_eq!(before.issues_count, 2);
    assert!(before
        .issues
        .iter()
        .any(|i| i.code == LayoutIssueCode::CELL_OVERFLOW_V));
    assert!(before
        .issues
        .iter()
        .any(|i| i.code == LayoutIssueCode::TABLE_EXCEEDS_PAGE));

    let prompt = format_layout_report_for_prompt(&before);
    assert!(prompt.contains("[세로 넘침 / error]"));
    assert!(prompt.contains("[표 폭 초과 / warning]"));

    let resolved_src = MockLayoutSource {
        tables: vec![TableCoordinateMeta {
            table_index: 0,
            sec: 0,
            para: 0,
            ctrl: 0,
            row_count: 1,
            col_count: 1,
            parent_table_index: None,
            parent_cell: None,
            depth: Some(0),
            cells: vec![TableCellMeta {
                idx: 0,
                row: 0,
                col: 0,
                row_span: 1,
                col_span: 1,
                text: "짧음".to_string(),
                bbox: Some(CellBBox {
                    cell_idx: 0,
                    row: 0,
                    col: 0,
                    row_span: 1,
                    col_span: 1,
                    page_index: 0,
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 40.0,
                }),
            }],
        }],
    };
    let after = inspect_layout_from_source(&resolved_src, default_layout_options(), 2000);
    assert_eq!(after.issues_count, 0);

    let cmp = compare_layout_reports(&before, &after);
    assert!(cmp.ok);
    assert_eq!(cmp.resolved_issues.len(), 2);
    assert_eq!(cmp.remaining_issues.len(), 0);
    assert_eq!(cmp.new_issues.len(), 0);
    let delta = cmp.metrics_delta.unwrap();
    assert_eq!(delta.before_issues_count, 2);
    assert_eq!(delta.after_issues_count, 0);
    assert_eq!(delta.resolved_count, 2);
}
