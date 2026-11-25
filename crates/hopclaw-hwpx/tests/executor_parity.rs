use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;

use hopclaw_hwpx::xml_util::{get_attr, scan_top_level};
use hopclaw_hwpx::{
    execute_hwpx_edit_plan, execute_hwpx_edit_plan_model, is_valid_hwpx_package, HwpxDocument,
};
use hopclaw_types::{
    validate_preorder_coords, BorderLineType, BorderSide, CellHAlign, CellVAlign,
    ColInsertPosition, EqualizeMode, ErrorCode, HeaderFooterLocation, HighlightValue,
    HwpxEditPlan, HwpxEditStep, ImageCellTarget, ListStyleMode, NestedTableWidthMode,
    PageNumberAlign, ParaIndexTarget, ParagraphAlign, RowInsertPosition, SealMode, TableWalker,
};
use serde_json::{json, Value};
use zip::{CompressionMethod, ZipArchive};

fn repo_root() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = p.canonicalize().expect("repo root");
    let s = raw.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        raw
    }
}

fn crc_map(bytes: &[u8]) -> HashMap<String, u32> {
    let mut arc = ZipArchive::new(Cursor::new(bytes)).expect("valid zip");
    let mut map = HashMap::new();
    for i in 0..arc.len() {
        let e = arc.by_index(i).expect("entry");
        map.insert(e.name().to_string(), e.crc32());
    }
    map
}

fn minimal_1x1_png() -> Vec<u8> {
    // Valid 1x1 RGBA PNG
    vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]
}

fn run_node_hwpx_edit(input_path: &std::path::Path, output_path: &std::path::Path, ops: &[Value]) {
    let script = repo_root().join("resources/claw-hwp/scripts/hwpx-edit.js");
    let payload = json!({
        "path": input_path.to_string_lossy(),
        "output": output_path.to_string_lossy(),
        "operations": ops,
    });
    let mut child = Command::new("node")
        .arg(&script)
        .current_dir(repo_root())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn node hwpx-edit.js");
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin
            .write_all(payload.to_string().as_bytes())
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait hwpx-edit.js");
    assert!(
        out.status.success(),
        "hwpx-edit.js failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_node_extract_inspect(path: &std::path::Path) -> Value {
    let script = repo_root().join("resources/claw-hwp/scripts/extract_text.js");
    let out = Command::new("node")
        .arg(&script)
        .arg("--inspect")
        .arg("--with-cell-text")
        .arg(path)
        .current_dir(repo_root())
        .output()
        .expect("run extract_text.js --inspect");
    assert!(
        out.status.success(),
        "extract_text.js --inspect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("valid inspect json")
}

fn run_node_extract_markdown(path: &std::path::Path) -> String {
    let script = repo_root().join("resources/claw-hwp/scripts/extract_text.js");
    let out = Command::new("node")
        .arg(&script)
        .arg("--format")
        .arg("markdown")
        .arg(path)
        .current_dir(repo_root())
        .output()
        .expect("run extract_text.js --format markdown");
    assert!(
        out.status.success(),
        "extract_text.js --format markdown failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8 markdown")
}

#[test]
fn untouched_entries_crc_identical_and_mimetype_stored_first() {
    let samples = [
        "samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx",
        "samples/demo_official_report.hwpx",
    ];
    for rel in samples {
        let orig = std::fs::read(repo_root().join(rel)).expect("read sample");
        let before_crcs = crc_map(&orig);
        let plan = HwpxEditPlan {
            is_new_document: false,
            steps: vec![HwpxEditStep::AppendParagraph {
                text: "CRC 검증 문단".to_string(),
                align: None,
            }],
        };
        let edited = execute_hwpx_edit_plan(&orig, &plan).expect("execute edit plan");
        assert!(
            is_valid_hwpx_package(&edited),
            "{rel} edited output failed is_valid_hwpx_package"
        );

        let mut arc = ZipArchive::new(Cursor::new(&edited)).expect("valid edited zip");
        let first = arc.by_index(0).expect("entry 0");
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        drop(first);

        let after_crcs = crc_map(&edited);
        for (name, crc) in &before_crcs {
            if name == "Contents/section0.xml" {
                assert_ne!(after_crcs.get(name), Some(crc), "{rel}: section0 should change");
            } else {
                assert_eq!(
                    after_crcs.get(name),
                    Some(crc),
                    "{rel}: untouched entry {name} CRC32 changed"
                );
            }
        }
    }
}

#[test]
fn header_item_cnt_and_sec_cnt_stay_synced() {
    let sample = std::fs::read(repo_root().join("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx"))
        .expect("read sample");
    let plan = HwpxEditPlan {
        is_new_document: false,
        steps: vec![
            HwpxEditStep::SetCellBackground {
                table_index: 0,
                row: 0,
                col: 0,
                color: "#DDEEFF".to_string(),
            },
            HwpxEditStep::SetCellBorder {
                table_index: 0,
                row: 0,
                col: 0,
                sides: vec![BorderSide::Top, BorderSide::Bottom],
                color: "#112233".to_string(),
                width_hu: 142,
                line_type: BorderLineType::Dashed,
            },
            HwpxEditStep::ApplyTextStyle {
                target_text: "교수".to_string(),
                font_family: None,
                size_hu: Some(1200),
                bold: Some(true),
                italic: Some(true),
                underline: Some(true),
                color: Some("#AA0000".to_string()),
                highlight: Some(HighlightValue::Bool(true)),
                letter_spacing: Some(-5.0),
                char_ratio: Some(95.0),
            },
            HwpxEditStep::ApplyParagraphStyle {
                para_index: Some(ParaIndexTarget::Index(0)),
                target_text: None,
                align: Some(ParagraphAlign::Center),
                indent_hu: Some(283),
                line_spacing_percent: Some(150.0),
                spacing_before_hu: Some(283),
                spacing_after_hu: Some(283),
            },
            HwpxEditStep::SetListStyle {
                para_index: Some(1),
                target_text: None,
                mode: ListStyleMode::Bullet,
            },
        ],
    };

    let edited = execute_hwpx_edit_plan(&sample, &plan).expect("execute plan");
    assert!(is_valid_hwpx_package(&edited));

    let doc = HwpxDocument::open(edited).expect("open edited");
    let sec_count = doc.archive.section_names().len();
    let header_name = doc.archive.header_name().expect("header.xml");
    let header = doc.archive.read_text_ro(&header_name).expect("read header");

    let head_el = &scan_top_level(&header, "hh:head").unwrap()[0];
    assert_eq!(
        get_attr(head_el.attrs(&header, "hh:head"), "secCnt").as_deref(),
        Some(sec_count.to_string().as_str())
    );

    for (list_tag, item_tag) in [
        ("hh:borderFills", "hh:borderFill"),
        ("hh:charProperties", "hh:charPr"),
        ("hh:paraProperties", "hh:paraPr"),
        ("hh:bullets", "hh:bullet"),
        ("hh:numberings", "hh:numbering"),
    ] {
        if let Some(list_el) = scan_top_level(&header, list_tag).unwrap().first() {
            let actual = scan_top_level(list_el.inner(&header), item_tag)
                .unwrap()
                .len();
            let item_cnt = get_attr(list_el.attrs(&header, list_tag), "itemCnt");
            assert_eq!(
                item_cnt.as_deref(),
                Some(actual.to_string().as_str()),
                "{list_tag} itemCnt out of sync"
            );
        }
    }
}

#[test]
fn nested_table_depth_and_8x8_limits_enforced() {
    // 1. Depth 0 -> insert depth-1 (2x2) -> insert depth-2 (8x8) succeeds
    let plan = HwpxEditPlan {
        is_new_document: true,
        steps: vec![
            HwpxEditStep::InsertTable {
                after_para_index: Some(0),
                headers: vec!["Col1".to_string(), "Col2".to_string()],
                rows: vec![vec!["Outer00".to_string(), "Outer01".to_string()]],
            },
            HwpxEditStep::InsertNestedTable {
                table_index: 0,
                row: 0,
                col: 0,
                rows: 2,
                cols: 2,
                data: Some(vec![
                    vec!["D1-00".to_string(), "D1-01".to_string()],
                    vec!["D1-10".to_string(), "D1-11".to_string()],
                ]),
                header_row: true,
                width_mode: NestedTableWidthMode::FitCell,
            },
            HwpxEditStep::InsertNestedTable {
                table_index: 1,
                row: 0,
                col: 0,
                rows: 8,
                cols: 8,
                data: Some(vec![vec!["D2-00".to_string()]]),
                header_row: false,
                width_mode: NestedTableWidthMode::FitCell,
            },
            HwpxEditStep::SetCellText {
                table_index: 2,
                row: 7,
                col: 7,
                text: "D2-77".to_string(),
                fit: false,
            },
        ],
    };

    let bytes = execute_hwpx_edit_plan(&[], &plan).expect("depth 2 & 8x8 should succeed");
    assert!(is_valid_hwpx_package(&bytes));

    let doc = HwpxDocument::open(bytes.clone()).expect("open");
    let walked = doc.walk_tables().expect("walk tables");
    validate_preorder_coords(&walked).expect("valid preorder coords");
    assert_eq!(walked.len(), 3);
    assert_eq!(walked[0].depth, Some(0));
    assert_eq!(walked[1].depth, Some(1));
    assert_eq!(walked[1].parent_table_index, Some(0));
    assert_eq!(walked[2].depth, Some(2));
    assert_eq!(walked[2].parent_table_index, Some(1));
    assert_eq!(walked[2].row_count, 8);
    assert_eq!(walked[2].col_count, 8);
    assert!(walked[2].cells.iter().any(|c| c.text == "D2-77"));

    // 2. Depth 2 -> inserting another nested table inside table 2 must fail with E_OP_UNSUPPORTED_FORMAT
    let depth3_plan = HwpxEditPlan {
        is_new_document: false,
        steps: vec![HwpxEditStep::InsertNestedTable {
            table_index: 2,
            row: 0,
            col: 0,
            rows: 2,
            cols: 2,
            data: None,
            header_row: false,
            width_mode: NestedTableWidthMode::FitCell,
        }],
    };
    let err = execute_hwpx_edit_plan_model(&bytes, &depth3_plan).expect_err("depth 3 must fail");
    assert_eq!(err.code, ErrorCode::E_OP_UNSUPPORTED_FORMAT);

    // 3. 9x2 and 2x9 nested tables must be rejected
    for (r, c) in [(9, 2), (2, 9)] {
        let too_big = HwpxEditPlan {
            is_new_document: false,
            steps: vec![HwpxEditStep::InsertNestedTable {
                table_index: 0,
                row: 0,
                col: 1,
                rows: r,
                cols: c,
                data: None,
                header_row: false,
                width_mode: NestedTableWidthMode::FitCell,
            }],
        };
        assert!(execute_hwpx_edit_plan(&bytes, &too_big).is_err());
    }
}

#[test]
fn missing_table_returns_e_table_not_found() {
    let plan = HwpxEditPlan {
        is_new_document: true,
        steps: vec![HwpxEditStep::SetCellText {
            table_index: 99,
            row: 0,
            col: 0,
            text: "x".to_string(),
            fit: false,
        }],
    };
    let err = execute_hwpx_edit_plan_model(&[], &plan).expect_err("missing table should fail");
    assert_eq!(err.code, ErrorCode::E_TABLE_NOT_FOUND);
}

#[test]
fn all_33_steps_execute_cleanly_and_revalidate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let png_path = dir.path().join("seal.png");
    std::fs::write(&png_path, minimal_1x1_png()).expect("write png");
    let png_str = png_path.to_string_lossy().to_string();

    let plan = HwpxEditPlan {
        is_new_document: true,
        steps: vec![
            HwpxEditStep::SetParagraphText {
                index: 0,
                text: "제목 {{KEY}}".to_string(),
            },
            HwpxEditStep::ReplaceText {
                find: "{{KEY}}".to_string(),
                replace: "2026".to_string(),
            },
            HwpxEditStep::AppendParagraph {
                text: "서명란: 홍길동 (인)".to_string(),
                align: Some(ParagraphAlign::Left),
            },
            HwpxEditStep::AppendHeading {
                level: 1,
                text: "1. 개요".to_string(),
                size_hu: 1600,
            },
            HwpxEditStep::InsertTable {
                after_para_index: Some(1),
                headers: vec!["항목".to_string(), "내용".to_string(), "비고".to_string()],
                rows: vec![
                    vec!["가".to_string(), "내용1       끝".to_string(), "B1".to_string()],
                    vec!["나".to_string(), "내용2".to_string(), "B2".to_string()],
                ],
            },
            HwpxEditStep::SetCellText {
                table_index: 0,
                row: 1,
                col: 1,
                text: "값".to_string(),
                fit: true,
            },
            HwpxEditStep::InsertTableRow {
                table_index: 0,
                row: 2,
                position: RowInsertPosition::Below,
                cells: Some(vec!["다".to_string(), "내용3".to_string(), "B3".to_string()]),
            },
            HwpxEditStep::DeleteTableRow {
                table_index: 0,
                row: 3,
            },
            HwpxEditStep::InsertTableCol {
                table_index: 0,
                col: 2,
                position: ColInsertPosition::Right,
                cells: Some(vec!["추가열".to_string(), "C1".to_string(), "C2".to_string()]),
            },
            HwpxEditStep::DeleteTableCol {
                table_index: 0,
                col: 3,
            },
            HwpxEditStep::MergeCells {
                table_index: 0,
                from_row: 2,
                from_col: 0,
                to_row: 2,
                to_col: 1,
            },
            HwpxEditStep::UnmergeCells {
                table_index: 0,
                row: 2,
                col: 0,
            },
            HwpxEditStep::SplitCell {
                table_index: 0,
                row: 2,
                col: 2,
                into_rows: 2,
                into_cols: 1,
            },
            HwpxEditStep::EqualizeTable {
                table_index: 0,
                mode: EqualizeMode::Width,
            },
            HwpxEditStep::SetCellBackground {
                table_index: 0,
                row: 0,
                col: 0,
                color: "#EEEEEE".to_string(),
            },
            HwpxEditStep::SetCellAlign {
                table_index: 0,
                row: 0,
                col: 0,
                end_row: Some(0),
                end_col: Some(2),
                horizontal: Some(CellHAlign::Center),
                vertical: Some(CellVAlign::Center),
            },
            HwpxEditStep::SetCellBorder {
                table_index: 0,
                row: 0,
                col: 0,
                sides: vec![BorderSide::Bottom],
                color: "#000000".to_string(),
                width_hu: 142,
                line_type: BorderLineType::Double,
            },
            HwpxEditStep::SetColWidth {
                table_index: 0,
                col: 0,
                width_hu: 8504,
                keep_table_width: false,
            },
            HwpxEditStep::SetRowHeight {
                table_index: 0,
                row: 0,
                height_hu: 2835,
            },
            HwpxEditStep::SetCellPadding {
                table_index: 0,
                row: 0,
                col: 0,
                left_hu: Some(400),
                right_hu: Some(400),
                top_hu: Some(200),
                bottom_hu: Some(200),
            },
            HwpxEditStep::FitTableToPage {
                table_index: 0,
                target_width_hu: None,
            },
            HwpxEditStep::ShrinkCellFont {
                table_index: 0,
                row: 0,
                col: 0,
                size_hu: 800,
                min_hu: 700,
            },
            HwpxEditStep::InsertImage {
                path: png_str.clone(),
                anchor_text: None,
                width_hu: Some(5669),
                height_hu: Some(5669),
                cell: None,
            },
            HwpxEditStep::InsertImage {
                path: png_str.clone(),
                anchor_text: None,
                width_hu: Some(2835),
                height_hu: Some(2835),
                cell: Some(ImageCellTarget {
                    table_index: 0,
                    row: 1,
                    col: 2,
                }),
            },
            HwpxEditStep::PlaceSeal {
                anchor_text: "(인)".to_string(),
                seal_image_path: png_str,
                mode: SealMode::Auto,
            },
            HwpxEditStep::SetHeaderFooter {
                location: HeaderFooterLocation::Header,
                text: "문서 머리말".to_string(),
            },
            HwpxEditStep::InsertPageNumber {
                location: HeaderFooterLocation::Footer,
                align: PageNumberAlign::Center,
            },
            HwpxEditStep::InsertFootnote {
                anchor_text: "개요".to_string(),
                text: "각주 설명".to_string(),
                para_index: None,
            },
            HwpxEditStep::SetColumns {
                count: 2,
                spacing_hu: Some(1134),
            },
            HwpxEditStep::SetPageMargin {
                left_hu: Some(5669),
                right_hu: Some(5669),
                top_hu: Some(4252),
                bottom_hu: Some(4252),
            },
        ],
    };

    let bytes = execute_hwpx_edit_plan(&[], &plan).expect("all ops succeed");
    assert!(is_valid_hwpx_package(&bytes));

    let out_file = dir.path().join("all_ops.hwpx");
    std::fs::write(&out_file, &bytes).expect("write all_ops.hwpx");
    let inspect = run_node_extract_inspect(&out_file);
    assert_eq!(inspect["fileType"].as_str(), Some("hwpx"));
    assert_eq!(inspect["tableCount"].as_u64(), Some(1));
}

#[test]
fn differential_parity_against_hwpx_edit_js_on_samples() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Sample 1: lesson plan HWPX (contains existing table)
    let sample1_path = repo_root().join("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwpx");
    let sample1_bytes = std::fs::read(&sample1_path).expect("read sample1");
    let rust_plan1 = HwpxEditPlan {
        is_new_document: false,
        steps: vec![
            HwpxEditStep::SetCellText {
                table_index: 0,
                row: 0,
                col: 1,
                text: "인공지능과 알고리즘".to_string(),
                fit: false,
            },
            HwpxEditStep::InsertTableRow {
                table_index: 0,
                row: 1,
                position: RowInsertPosition::Below,
                cells: Some(vec!["추가행".to_string(), "내용A".to_string()]),
            },
            HwpxEditStep::InsertNestedTable {
                table_index: 0,
                row: 0,
                col: 0,
                rows: 2,
                cols: 2,
                data: Some(vec![
                    vec!["중첩1".to_string(), "중첩2".to_string()],
                    vec!["중첩3".to_string(), "중첩4".to_string()],
                ]),
                header_row: false,
                width_mode: NestedTableWidthMode::FitCell,
            },
            HwpxEditStep::SetHeaderFooter {
                location: HeaderFooterLocation::Header,
                text: "교수학습과정안 머리말".to_string(),
            },
            HwpxEditStep::SetColumns {
                count: 2,
                spacing_hu: Some(1134),
            },
        ],
    };
    let rust_out1 = execute_hwpx_edit_plan(&sample1_bytes, &rust_plan1).expect("rust edit 1");
    assert!(is_valid_hwpx_package(&rust_out1));
    let rust_file1 = dir.path().join("rust_1.hwpx");
    let node_file1 = dir.path().join("node_1.hwpx");
    std::fs::write(&rust_file1, &rust_out1).unwrap();

    run_node_hwpx_edit(
        &sample1_path,
        &node_file1,
        &[
            json!({"type": "set_cell_text", "table": 0, "row": 0, "col": 1, "text": "인공지능과 알고리즘"}),
            json!({"type": "insert_table_row", "table": 0, "row": 1, "where": "after", "cells": ["추가행", "내용A"]}),
            json!({"type": "insert_nested_table", "table": 0, "row": 0, "col": 0, "rows": 2, "cols": 2, "data": [["중첩1", "중첩2"], ["중첩3", "중첩4"]], "headerRow": false, "widthMode": "fit_cell"}),
            json!({"type": "set_header", "text": "교수학습과정안 머리말"}),
            json!({"type": "set_columns", "count": 2, "gap_mm": 4}),
        ],
    );

    let rust_insp1 = run_node_extract_inspect(&rust_file1);
    let node_insp1 = run_node_extract_inspect(&node_file1);
    assert_eq!(rust_insp1["sectionCount"], node_insp1["sectionCount"]);
    assert_eq!(rust_insp1["tableCount"], node_insp1["tableCount"]);
    assert_eq!(rust_insp1["cellCount"], node_insp1["cellCount"]);
    assert_eq!(
        rust_insp1["tables"][1]["cells"],
        node_insp1["tables"][1]["cells"],
        "nested table cells must match hwpx-edit.js identically"
    );

    let rust_doc1 = HwpxDocument::open(rust_out1).unwrap();
    let walked1 = rust_doc1.walk_tables().unwrap();
    assert_eq!(walked1.len(), 2);
    assert_eq!(walked1[1].depth, Some(1));
    assert_eq!(walked1[1].parent_table_index, Some(0));

    // Sample 2: official report HWPX (table insertion + merge + text ops)
    let sample2_path = repo_root().join("samples/demo_official_report.hwpx");
    let sample2_bytes = std::fs::read(&sample2_path).expect("read sample2");
    let rust_plan2 = HwpxEditPlan {
        is_new_document: false,
        steps: vec![
            HwpxEditStep::AppendParagraph {
                text: "추가 보고 문단".to_string(),
                align: None,
            },
            HwpxEditStep::InsertTable {
                after_para_index: None,
                headers: vec!["구분".to_string(), "세부".to_string(), "담당".to_string()],
                rows: vec![
                    vec!["1단계".to_string(), "설계".to_string(), "팀A".to_string()],
                    vec!["2단계".to_string(), "구현".to_string(), "팀B".to_string()],
                ],
            },
            HwpxEditStep::MergeCells {
                table_index: 0,
                from_row: 1,
                from_col: 0,
                to_row: 2,
                to_col: 0,
            },
            HwpxEditStep::SetCellText {
                table_index: 0,
                row: 1,
                col: 1,
                text: "아키텍처 설계".to_string(),
                fit: false,
            },
        ],
    };
    let rust_out2 = execute_hwpx_edit_plan(&sample2_bytes, &rust_plan2).expect("rust edit 2");
    assert!(is_valid_hwpx_package(&rust_out2));
    let rust_file2 = dir.path().join("rust_2.hwpx");
    let node_file2 = dir.path().join("node_2.hwpx");
    std::fs::write(&rust_file2, &rust_out2).unwrap();

    run_node_hwpx_edit(
        &sample2_path,
        &node_file2,
        &[
            json!({"type": "append_paragraph", "text": "추가 보고 문단"}),
            json!({"type": "insert_table", "index": "end", "rows": 3, "cols": 3, "cells": [["구분", "세부", "담당"], ["1단계", "설계", "팀A"], ["2단계", "구현", "팀B"]]}),
            json!({"type": "merge_cells", "table": 0, "mode": "vertical", "col": 0, "start": 1, "count": 2}),
            json!({"type": "set_cell_text", "table": 0, "row": 1, "col": 1, "text": "아키텍처 설계"}),
        ],
    );

    let rust_insp2 = run_node_extract_inspect(&rust_file2);
    let node_insp2 = run_node_extract_inspect(&node_file2);
    assert_eq!(rust_insp2["sectionCount"], node_insp2["sectionCount"]);
    assert_eq!(rust_insp2["tableCount"], node_insp2["tableCount"]);
    assert_eq!(rust_insp2["cellCount"], node_insp2["cellCount"]);
    assert_eq!(rust_insp2["tables"], node_insp2["tables"]);

    let rust_md2 = run_node_extract_markdown(&rust_file2);
    let node_md2 = run_node_extract_markdown(&node_file2);
    assert_eq!(rust_md2, node_md2);
}

#[test]
fn insert_image_into_cell_after_merged_colspan_uses_cell_addr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let png_path = dir.path().join("badge.png");
    std::fs::write(&png_path, minimal_1x1_png()).expect("write png");
    let png_str = png_path.to_string_lossy().to_string();

    // Row 1 merges cols 0..=1 into a colSpan=2 cell, so logical col=2 is the second <hp:tc> in row 1.
    let plan = HwpxEditPlan {
        is_new_document: true,
        steps: vec![
            HwpxEditStep::InsertTable {
                after_para_index: None,
                headers: vec!["A".to_string(), "B".to_string(), "C".to_string()],
                rows: vec![vec!["L".to_string(), "M".to_string(), "R".to_string()]],
            },
            HwpxEditStep::MergeCells {
                table_index: 0,
                from_row: 1,
                from_col: 0,
                to_row: 1,
                to_col: 1,
            },
            HwpxEditStep::InsertImage {
                path: png_str,
                anchor_text: None,
                width_hu: Some(2835),
                height_hu: Some(2835),
                cell: Some(ImageCellTarget {
                    table_index: 0,
                    row: 1,
                    col: 2,
                }),
            },
        ],
    };

    let bytes = execute_hwpx_edit_plan(&[], &plan).expect("insert image after colSpan merge");
    assert!(is_valid_hwpx_package(&bytes));

    let doc = HwpxDocument::open(bytes).expect("reopen edited document");
    let loc = doc.get_table(0).expect("table 0");
    let sec_xml = doc.archive.read_text_ro(&loc.section).expect("section xml");
    let tbl_inner = loc.span.inner(&sec_xml);
    let rows = hopclaw_hwpx::xml_util::scan_top_level(tbl_inner, "hp:tr").expect("rows");
    let r1_inner = rows[1].inner(tbl_inner);
    let tcs = hopclaw_hwpx::xml_util::scan_top_level(r1_inner, "hp:tc").expect("tcs");
    assert_eq!(tcs.len(), 2, "merged row 1 should have 2 <hp:tc> elements");
    assert!(
        !tcs[0].inner(r1_inner).contains("<hp:pic"),
        "merged cell at col 0..=1 must not receive image intended for colAddr=2"
    );
    assert!(
        tcs[1].inner(r1_inner).contains("<hp:pic"),
        "cell at colAddr=2 (second <hp:tc>) must contain inserted <hp:pic>"
    );
}
