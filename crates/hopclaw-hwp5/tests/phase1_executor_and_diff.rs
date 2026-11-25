use std::path::PathBuf;
use std::process::Command;

use hopclaw_hwp5::{
    execute_hwp5_plan, inspect_hwp5, reject_unsupported_unified_op, Hwp5Container, Hwp5Inspector,
};
use hopclaw_types::{
    validate_preorder_coords, BorderLineType, BorderSide, CellHAlign, CellVAlign,
    ColInsertPosition, EqualizeMode, ErrorCode, HeaderFooterLocation, HighlightValue, Hwp5EditPlan,
    Hwp5EditStep, Hwp5TableTarget, InspectDocumentResult, ListStyleMode, PageNumberAlign,
    ParaIndexTarget, ParagraphAlign, RowInsertPosition, SealMode, UnifiedOp,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn run_node_inspect(hwp_path: &std::path::Path) -> InspectDocumentResult {
    let script = repo_root().join("resources/claw-hwp/scripts/extract_text.js");
    let output = Command::new("node")
        .arg(&script)
        .arg(hwp_path)
        .arg("--inspect")
        .arg("--with-cell-text")
        .output()
        .expect("spawn node extract_text.js");
    assert!(
        output.status.success(),
        "extract_text.js failed on {}: {}",
        hwp_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse extract_text.js JSON output")
}

/// Minimal valid 2x2 RGBA PNG bytes (87 bytes) for image & seal tests.
fn tiny_png_bytes() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R', // IHDR
        0x00, 0x00, 0x00, 0x02, // width = 2
        0x00, 0x00, 0x00, 0x02, // height = 2
        0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xB6, 0x0D, 0x24, // 8bit RGBA + CRC
        0x00, 0x00, 0x00, 0x12, b'I', b'D', b'A', b'T', // IDAT
        0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x44, 0x24, 0x00, 0x00, 0x00, 0x00,
        0xFF, 0xFF, 0x03, 0x00, 0x48, 0x00, 0x00, 0x01, // zlib stream placeholder
        0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ]
}

#[test]
fn insert_nested_table_is_rejected_with_e_op_unsupported_format() {
    let op = UnifiedOp::InsertNestedTable {
        table_index: 0,
        row: 0,
        col: 0,
        rows: 2,
        cols: 2,
        data: None,
        header_row: None,
        width_mode: None,
    };
    let err = reject_unsupported_unified_op(&op).unwrap_err();
    assert_eq!(err.code, ErrorCode::E_OP_UNSUPPORTED_FORMAT);
}

#[test]
fn rust_table_walker_matches_node_extract_text_on_corpus_samples() {
    let full_match_samples = [
        repo_root().join("assets/templates/blank.hwp"),
        repo_root().join("samples/demo_official_report.hwp"),
    ];

    for path in &full_match_samples {
        let bytes = std::fs::read(path).unwrap();
        let rust_inspect = inspect_hwp5(&bytes, true).unwrap();
        let node_inspect = run_node_inspect(path);

        assert_eq!(rust_inspect.file_type, "hwp");
        assert_eq!(rust_inspect.section_count, node_inspect.section_count);
        assert_eq!(rust_inspect.table_count, node_inspect.table_count);
        assert_eq!(rust_inspect.cell_count, node_inspect.cell_count);

        let rust_tables = rust_inspect.tables.as_ref().unwrap();
        validate_preorder_coords(rust_tables).expect("canonical preorder coords");
        let node_tables = node_inspect.tables.as_ref().unwrap();
        assert_eq!(rust_tables.len(), node_tables.len());

        for (rt, nt) in rust_tables.iter().zip(node_tables.iter()) {
            assert_eq!(rt.table_index, nt.table_index);
            assert_eq!(rt.row_count, nt.row_count);
            assert_eq!(rt.col_count, nt.col_count);
            assert_eq!(rt.cells.len(), nt.cells.len());
            for (rc, nc) in rt.cells.iter().zip(nt.cells.iter()) {
                assert_eq!(rc.row, nc.row);
                assert_eq!(rc.col, nc.col);
                assert_eq!(rc.row_span, nc.row_span);
                assert_eq!(rc.col_span, nc.col_span);
                assert_eq!(rc.text.trim(), nc.text.trim());
            }
        }
    }

    // Lesson plan sample contains a 10x4 (34-cell) binary table; rhwp's getCellInfo stops after cell 8,
    // while pure-Rust Hwp5Inspector recovers all 34 cells and matches Node on cells 0..8.
    let lesson_path = repo_root().join("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp");
    let lesson_bytes = std::fs::read(&lesson_path).unwrap();
    let rust_lesson = inspect_hwp5(&lesson_bytes, true).unwrap();
    let node_lesson = run_node_inspect(&lesson_path);
    let rt = &rust_lesson.tables.as_ref().unwrap()[0];
    let nt = &node_lesson.tables.as_ref().unwrap()[0];
    assert_eq!((rt.row_count, rt.col_count, rt.cells.len()), (10, 4, 34));
    validate_preorder_coords(rust_lesson.tables.as_ref().unwrap()).unwrap();
    for (rc, nc) in rt.cells.iter().zip(nt.cells.iter()) {
        assert_eq!((rc.row, rc.col, rc.text.trim()), (nc.row, nc.col, nc.text.trim()));
    }
}

#[test]
fn full_new_document_plan_roundtrips_cleanly_through_node_extract_text() {
    let tmp = tempfile::tempdir().unwrap();
    let img_path = tmp.path().join("stamp.png");
    std::fs::write(&img_path, tiny_png_bytes()).unwrap();
    let img_str = img_path.to_string_lossy().into_owned();

    let tbl0 = Hwp5TableTarget {
        table_index: 0,
        sec: None,
        para: None,
        ctrl: None,
    };

    let plan = Hwp5EditPlan {
        is_new_document: true,
        steps: vec![
            Hwp5EditStep::SetupDocument {
                width_hu: 59527,
                height_hu: 84188,
                margin_hu: 5669,
            },
            Hwp5EditStep::SetPageMargin {
                left_hu: Some(5669),
                right_hu: Some(5669),
                top_hu: Some(5000),
                bottom_hu: Some(5000),
            },
            Hwp5EditStep::AppendHeading {
                level: 1,
                text: "2026년 프로젝트 추진 계획서".to_string(),
                size_hu: 1600,
            },
            Hwp5EditStep::AppendParagraph {
                text: "작성자: 홍길동 (도장) / 상태: DRAFT".to_string(),
                align: Some(ParagraphAlign::Left),
            },
            Hwp5EditStep::ReplaceText {
                find: "DRAFT".to_string(),
                replace: "FINAL".to_string(),
            },
            Hwp5EditStep::ApplyTextStyle {
                target_text: "FINAL".to_string(),
                font_family: Some("Malgun Gothic".to_string()),
                size_hu: Some(1100),
                bold: Some(true),
                italic: Some(false),
                underline: Some(true),
                color: Some("#1E40AF".to_string()),
                highlight: Some(HighlightValue::Color("#FEF08A".to_string())),
                letter_spacing: Some(-5.0),
                char_ratio: Some(98.0),
            },
            Hwp5EditStep::ApplyParagraphStyle {
                para_index: Some(ParaIndexTarget::Last),
                target_text: None,
                align: Some(ParagraphAlign::Center),
                indent_hu: Some(0),
                line_spacing_percent: Some(150.0),
                spacing_before_hu: Some(200),
                spacing_after_hu: Some(300),
            },
            Hwp5EditStep::AppendParagraph {
                text: "첫 번째 항목".to_string(),
                align: None,
            },
            Hwp5EditStep::SetListStyle {
                para_index: None,
                target_text: Some("첫 번째 항목".to_string()),
                mode: ListStyleMode::Numbered,
            },
            Hwp5EditStep::AppendTable {
                headers: vec!["구분".into(), "항목".into(), "비고".into()],
                rows: vec![
                    vec!["1".into(), "기획".into(), "완료".into()],
                    vec!["2".into(), "구현".into(), "진행".into()],
                ],
            },
            Hwp5EditStep::SetCellText {
                target: tbl0.clone(),
                row: 2,
                col: 2,
                text: "최종\n완료".to_string(),
                fit: true,
            },
            Hwp5EditStep::SetCellBackground {
                target: tbl0.clone(),
                row: 1,
                col: 0,
                color: "#E0F2FE".to_string(),
            },
            Hwp5EditStep::SetCellBorder {
                target: tbl0.clone(),
                row: 1,
                col: 1,
                sides: vec![BorderSide::Top, BorderSide::Bottom],
                color: Some("#0F172A".to_string()),
                width_hu: Some(283),
                line_type: Some(BorderLineType::Double),
            },
            Hwp5EditStep::SetCellAlign {
                target: tbl0.clone(),
                row: 1,
                col: 0,
                end_row: Some(2),
                end_col: Some(2),
                horizontal: Some(CellHAlign::Center),
                vertical: Some(CellVAlign::Center),
            },
            Hwp5EditStep::InsertTableRow {
                target: tbl0.clone(),
                row: 2,
                position: RowInsertPosition::Below,
                cells: Some(vec!["3".into(), "검증".into(), "예정".into()]),
            },
            Hwp5EditStep::InsertTableCol {
                target: tbl0.clone(),
                col: 2,
                position: ColInsertPosition::Right,
                cells: Some(vec!["담당".into(), "A".into(), "B".into(), "C".into()]),
            },
            Hwp5EditStep::MergeCells {
                target: tbl0.clone(),
                from_row: 3,
                from_col: 0,
                to_row: 3,
                to_col: 1,
            },
            Hwp5EditStep::UnmergeCells {
                target: tbl0.clone(),
                row: 3,
                col: 0,
            },
            Hwp5EditStep::DeleteTableCol {
                target: tbl0.clone(),
                col: 3,
            },
            Hwp5EditStep::DeleteTableRow {
                target: tbl0.clone(),
                row: 3,
            },
            Hwp5EditStep::SplitCell {
                target: tbl0.clone(),
                row: 2,
                col: 2,
                into_rows: 2,
                into_cols: 1,
            },
            Hwp5EditStep::EqualizeTable {
                target: tbl0.clone(),
                mode: EqualizeMode::Width,
            },
            Hwp5EditStep::SetColWidth {
                target: tbl0.clone(),
                col: 0,
                width_hu: 8000,
                keep_table_width: true,
            },
            Hwp5EditStep::SetRowHeight {
                target: tbl0.clone(),
                row: 1,
                height_hu: 3400,
            },
            Hwp5EditStep::SetCellPadding {
                target: tbl0.clone(),
                row: 1,
                col: 1,
                left_hu: Some(500),
                right_hu: Some(500),
                top_hu: Some(200),
                bottom_hu: Some(200),
            },
            Hwp5EditStep::FitTableToPage {
                target: tbl0.clone(),
                target_width_hu: None,
            },
            Hwp5EditStep::ShrinkCellFont {
                target: tbl0.clone(),
                row: 1,
                col: 1,
                size_hu: 900,
                min_hu: 750,
            },
            Hwp5EditStep::SetColumns {
                count: 2,
                spacing_hu: Some(1500),
            },
            Hwp5EditStep::SetHeaderFooter {
                location: HeaderFooterLocation::Header,
                text: "대외비".to_string(),
            },
            Hwp5EditStep::InsertPageNumber {
                location: HeaderFooterLocation::Footer,
                align: PageNumberAlign::Center,
            },
            Hwp5EditStep::InsertFootnote {
                anchor_text: "홍길동".to_string(),
                text: "책임 연구원".to_string(),
                para_index: None,
            },
            Hwp5EditStep::InsertImage {
                path: img_str.clone(),
                anchor_text: None,
                width_hu: Some(2835),
                height_hu: None,
                cell: None,
            },
            Hwp5EditStep::PlaceSeal {
                anchor_text: "(도장)".to_string(),
                seal_image_path: img_str,
                mode: SealMode::Overlap,
            },
        ],
    };

    let result = execute_hwp5_plan(None, &plan).expect("execute_hwp5_plan");
    assert_eq!(result.steps_applied, plan.steps.len());

    let cfb = Hwp5Container::open_bytes(&result.bytes).expect("open result CFB");
    assert!(!cfb.has_stream("/Sh33tJ5"));
    assert!(cfb.has_stream("/BinData/BIN0001.png"));
    assert!(cfb.has_stream("/BinData/BIN0002.png"));

    let out_file = tmp.path().join("rust_full.hwp");
    std::fs::write(&out_file, &result.bytes).unwrap();

    let rust_inspect = inspect_hwp5(&result.bytes, true).unwrap();
    let node_inspect = run_node_inspect(&out_file);

    assert_eq!(rust_inspect.section_count, node_inspect.section_count);
    assert_eq!(rust_inspect.table_count, 1);
    assert_eq!(node_inspect.table_count, 1);
    assert_eq!(rust_inspect.cell_count, node_inspect.cell_count);
    assert_eq!(rust_inspect.image_count, Some(2));
}

#[test]
fn differential_test_node_create_js_vs_rust_executor() {
    let tmp = tempfile::tempdir().unwrap();
    let node_out = tmp.path().join("node_ref.hwp");
    let rust_out = tmp.path().join("rust_ref.hwp");

    let node_spec = serde_json::json!({
        "path": node_out.to_string_lossy(),
        "operations": [
            { "type": "setup_document", "page_size": "a4", "margin_mm": 20 },
            { "type": "append_heading", "level": 1, "text": "차등 검증 보고서" },
            { "type": "append_paragraph", "text": "본문 확인용 문장입니다." },
            { "type": "append_table", "headers": ["항목", "상태"], "rows": [["빌드", "성공"], ["테스트", "대기"]] },
            { "type": "set_cell_text", "section": 0, "tableIndex": 0, "row": 2, "col": 1, "text": "통과" }
        ]
    });

    let script = repo_root().join("resources/claw-hwp/scripts/create.js");
    let mut child = Command::new("node")
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn create.js");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(node_spec.to_string().as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().expect("wait create.js");
    assert!(
        output.status.success(),
        "create.js failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rust_plan = Hwp5EditPlan {
        is_new_document: true,
        steps: vec![
            Hwp5EditStep::SetupDocument {
                width_hu: 59527,
                height_hu: 84188,
                margin_hu: 5669,
            },
            Hwp5EditStep::AppendHeading {
                level: 1,
                text: "차등 검증 보고서".to_string(),
                size_hu: 1600,
            },
            Hwp5EditStep::AppendParagraph {
                text: "본문 확인용 문장입니다.".to_string(),
                align: None,
            },
            Hwp5EditStep::AppendTable {
                headers: vec!["항목".into(), "상태".into()],
                rows: vec![vec!["빌드".into(), "성공".into()], vec!["테스트".into(), "대기".into()]],
            },
            Hwp5EditStep::SetCellText {
                target: Hwp5TableTarget {
                    table_index: 0,
                    sec: None,
                    para: None,
                    ctrl: None,
                },
                row: 2,
                col: 1,
                text: "통과".to_string(),
                fit: false,
            },
        ],
    };

    let rust_res = execute_hwp5_plan(None, &rust_plan).expect("rust create");
    std::fs::write(&rust_out, &rust_res.bytes).unwrap();

    let node_ins = run_node_inspect(&node_out);
    let rust_node_ins = run_node_inspect(&rust_out);
    let rust_native_ins = Hwp5Inspector::open_bytes(&rust_res.bytes)
        .unwrap()
        .inspect(true)
        .unwrap();

    assert_eq!(rust_node_ins.file_type, node_ins.file_type);
    assert_eq!(rust_node_ins.section_count, node_ins.section_count);
    assert_eq!(rust_node_ins.table_count, node_ins.table_count);
    assert_eq!(rust_node_ins.cell_count, node_ins.cell_count);
    assert_eq!(rust_native_ins.table_count, node_ins.table_count);
    assert_eq!(rust_native_ins.cell_count, node_ins.cell_count);

    let rt = &rust_node_ins.tables.as_ref().unwrap()[0];
    let nt = &node_ins.tables.as_ref().unwrap()[0];
    assert_eq!(rt.row_count, nt.row_count);
    assert_eq!(rt.col_count, nt.col_count);
    for (rc, nc) in rt.cells.iter().zip(nt.cells.iter()) {
        assert_eq!(rc.text.trim(), nc.text.trim());
    }
}

#[test]
fn edit_existing_official_report_roundtrips_via_node_extract_text() {
    let src = repo_root().join("samples/demo_official_report.hwp");
    let src_bytes = std::fs::read(&src).unwrap();
    let baseline = inspect_hwp5(&src_bytes, true).unwrap();
    assert!(baseline.table_count > 0);

    let plan = Hwp5EditPlan {
        is_new_document: false,
        steps: vec![
            Hwp5EditStep::SetCellText {
                target: Hwp5TableTarget {
                    table_index: 0,
                    sec: None,
                    para: None,
                    ctrl: None,
                },
                row: 0,
                col: 0,
                text: "변경됨".to_string(),
                fit: false,
            },
            Hwp5EditStep::SetCellBackground {
                target: Hwp5TableTarget {
                    table_index: 0,
                    sec: None,
                    para: None,
                    ctrl: None,
                },
                row: 0,
                col: 0,
                color: "#DBEAFE".to_string(),
            },
        ],
    };

    let edited = execute_hwp5_plan(Some(&src_bytes), &plan).expect("edit demo_official_report");
    let tmp = tempfile::tempdir().unwrap();
    let edited_path = tmp.path().join("edited_report.hwp");
    std::fs::write(&edited_path, &edited.bytes).unwrap();

    let rust_after = inspect_hwp5(&edited.bytes, true).unwrap();
    let node_after = run_node_inspect(&edited_path);
    assert_eq!(rust_after.table_count, node_after.table_count);
    assert_eq!(rust_after.cell_count, node_after.cell_count);
    assert_eq!(
        node_after.tables.as_ref().unwrap()[0].cells[0].text.trim(),
        "변경됨"
    );
}

#[test]
fn regression_append_table_header_valign_and_onta_fourcc() {
    use hopclaw_hwp5::{tag, DocInfoDoc, SectionDoc};

    let tbl0 = Hwp5TableTarget {
        table_index: 0,
        sec: None,
        para: None,
        ctrl: None,
    };
    let plan = Hwp5EditPlan {
        is_new_document: true,
        steps: vec![
            Hwp5EditStep::AppendParagraph {
                text: "각주 기준 문장".to_string(),
                align: None,
            },
            Hwp5EditStep::AppendTable {
                headers: vec!["제목1".into(), "제목2".into()],
                rows: vec![vec!["값1".into(), "값2".into()]],
            },
            Hwp5EditStep::InsertFootnote {
                anchor_text: "기준".to_string(),
                text: "보충 설명".to_string(),
                para_index: None,
            },
            Hwp5EditStep::InsertPageNumber {
                location: HeaderFooterLocation::Footer,
                align: PageNumberAlign::Center,
            },
            Hwp5EditStep::SetCellAlign {
                target: tbl0,
                row: 1,
                col: 0,
                end_row: None,
                end_col: None,
                horizontal: Some(CellHAlign::Center),
                vertical: Some(CellVAlign::Bottom),
            },
        ],
    };

    let res = execute_hwp5_plan(None, &plan).expect("execute plan");
    let mut cfb = Hwp5Container::open_bytes(&res.bytes).unwrap();
    let _di = DocInfoDoc::parse(&cfb.read_docinfo().unwrap()).unwrap();
    let sec = SectionDoc::parse(&cfb.read_section(0).unwrap()).unwrap();

    // 1. Verify LIST_HEADER bytes [2..6] valign and [6..8] header flag
    let tbls = sec.enumerate_tables().unwrap();
    assert_eq!(tbls.len(), 1);
    let cells = &tbls[0].cells;
    assert_eq!(cells.len(), 4);
    for c in &cells[0..2] {
        let b = &sec.records[c.list_header_rec_idx].body;
        assert_eq!(u32::from_le_bytes(b[2..6].try_into().unwrap()), 0x0020_0000);
        assert_eq!(u16::from_le_bytes(b[6..8].try_into().unwrap()), 4);
    }
    // Data cell (1,0) had vertical=Bottom applied (`2 << 21 == 0x0040_0000`), (1,1) keeps center `0x0020_0000`
    let c10 = &sec.records[cells[2].list_header_rec_idx].body;
    assert_eq!(u32::from_le_bytes(c10[2..6].try_into().unwrap()), 0x0040_0000);
    assert_eq!(u16::from_le_bytes(c10[6..8].try_into().unwrap()), 0);
    let c11 = &sec.records[cells[3].list_header_rec_idx].body;
    assert_eq!(u32::from_le_bytes(c11[2..6].try_into().unwrap()), 0x0020_0000);
    assert_eq!(u16::from_le_bytes(c11[6..8].try_into().unwrap()), 0);

    // 2. Verify all auto-number CTRL_HEADER records start with b"onta" (`6f 6e 74 61`)
    let onta_ctrls: Vec<_> = sec
        .records
        .iter()
        .filter(|r| r.tag == tag::CTRL_HEADER && r.level == 3 && r.body.len() >= 8)
        .collect();
    assert_eq!(onta_ctrls.len(), 2, "expected footnote + page_number onta controls");
    for ctrl in onta_ctrls {
        assert_eq!(&ctrl.body[0..4], b"onta");
    }

    // Confirm Node extract_text still inspects cleanly
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("onta_hdr.hwp");
    std::fs::write(&p, &res.bytes).unwrap();
    let node_ins = run_node_inspect(&p);
    assert_eq!(node_ins.table_count, 1);
    assert_eq!(node_ins.cell_count, 4);
}

#[test]
fn regression_shrink_cell_font_preserves_multi_run_styles_and_shorter_cell_text_prunes_runs() {
    use hopclaw_hwp5::{parse_para_char_shapes, tag, DocInfoDoc, ParaHeaderRef, SectionDoc};

    let tbl0 = Hwp5TableTarget {
        table_index: 0,
        sec: None,
        para: None,
        ctrl: None,
    };

    // Create a 1x1 cell, set multi-word text, style a substring bold+red (splitting into 3 runs),
    // then shrink_cell_font and verify distinct attributes stay intact across runs.
    let plan_multi = Hwp5EditPlan {
        is_new_document: true,
        steps: vec![
            Hwp5EditStep::AppendTable {
                headers: vec![],
                rows: vec![vec!["Prefix BoldTail Suffix".to_string()]],
            },
            Hwp5EditStep::ApplyTextStyle {
                target_text: "BoldTail".to_string(),
                font_family: None,
                size_hu: None,
                bold: Some(true),
                italic: None,
                underline: None,
                color: Some("#CC0000".to_string()),
                highlight: Some(HighlightValue::Color("#FFFF00".to_string())),
                letter_spacing: None,
                char_ratio: None,
            },
            Hwp5EditStep::ShrinkCellFont {
                target: tbl0.clone(),
                row: 0,
                col: 0,
                size_hu: 800,
                min_hu: 700,
            },
        ],
    };
    let res1 = execute_hwp5_plan(None, &plan_multi).unwrap();
    let mut cfb1 = Hwp5Container::open_bytes(&res1.bytes).unwrap();
    let di1 = DocInfoDoc::parse(&cfb1.read_docinfo().unwrap()).unwrap();
    let sec1 = SectionDoc::parse(&cfb1.read_section(0).unwrap()).unwrap();
    let t1 = &sec1.enumerate_tables().unwrap()[0];
    let c1 = &t1.cells[0];
    let cs_rec1 = sec1.records[c1.list_header_rec_idx + 1..c1.end_rec_idx]
        .iter()
        .find(|r| r.tag == tag::PARA_CHAR_SHAPE)
        .unwrap();
    let runs1 = parse_para_char_shapes(&cs_rec1.body).unwrap();
    assert_eq!(runs1.len(), 3, "multi-run cell formatting must not be collapsed");
    let cs_mid = di1.get_char_shape(runs1[1].char_shape_id as u16).unwrap();
    let cs_end = di1.get_char_shape(runs1[2].char_shape_id as u16).unwrap();
    // Both runs shrunk to 8pt (800 HWPUNIT), while run 1 keeps bold (bit 1) + red (#CC0000 -> 0x000000CC BGR)
    assert_eq!(i32::from_le_bytes(cs_mid[42..46].try_into().unwrap()), 800);
    assert_eq!(i32::from_le_bytes(cs_end[42..46].try_into().unwrap()), 800);
    assert_ne!(u32::from_le_bytes(cs_mid[46..50].try_into().unwrap()) & 2, 0);
    assert_eq!(u32::from_le_bytes(cs_mid[52..56].try_into().unwrap()), 0x0000_00CC);
    assert_ne!(runs1[0].char_shape_id, runs1[1].char_shape_id);

    // Overwrite the styled cell with much shorter text "Hi" (3 wchars including EOP):
    // out-of-bounds PARA_CHAR_SHAPE runs and stale PARA_RANGE_TAG must be pruned.
    let plan_short = Hwp5EditPlan {
        is_new_document: false,
        steps: vec![Hwp5EditStep::SetCellText {
            target: tbl0,
            row: 0,
            col: 0,
            text: "Hi".to_string(),
            fit: false,
        }],
    };
    let res2 = execute_hwp5_plan(Some(&res1.bytes), &plan_short).unwrap();
    let mut cfb2 = Hwp5Container::open_bytes(&res2.bytes).unwrap();
    let sec2 = SectionDoc::parse(&cfb2.read_section(0).unwrap()).unwrap();
    let t2 = &sec2.enumerate_tables().unwrap()[0];
    let c2 = &t2.cells[0];
    let ph2 = sec2.records[c2.list_header_rec_idx + 1..c2.end_rec_idx]
        .iter()
        .find(|r| r.tag == tag::PARA_HEADER)
        .unwrap();
    let hdr2 = ParaHeaderRef::wrap(&ph2.body).unwrap();
    let cs_rec2 = sec2.records[c2.list_header_rec_idx + 1..c2.end_rec_idx]
        .iter()
        .find(|r| r.tag == tag::PARA_CHAR_SHAPE)
        .unwrap();
    let runs2 = parse_para_char_shapes(&cs_rec2.body).unwrap();
    assert_eq!(hdr2.char_count(), 3);
    assert!(runs2.iter().all(|r| r.char_pos < 3));
    assert_eq!(u16::from_le_bytes(ph2.body[12..14].try_into().unwrap()) as usize, runs2.len());
    let rt_count = sec2.records[c2.list_header_rec_idx + 1..c2.end_rec_idx]
        .iter()
        .filter(|r| r.tag == tag::PARA_RANGE_TAG)
        .count();
    assert_eq!(rt_count, 0, "stale out-of-bounds highlight tag must be dropped");
    assert_eq!(u16::from_le_bytes(ph2.body[14..16].try_into().unwrap()), 0);

    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("short_cell.hwp");
    std::fs::write(&p, &res2.bytes).unwrap();
    let node_ins = run_node_inspect(&p);
    assert_eq!(node_ins.tables.as_ref().unwrap()[0].cells[0].text.trim(), "Hi");
}

#[test]
fn regression_split_cell_2d_subgrid_and_spanning_row() {
    use hopclaw_hwp5::{tag, SectionDoc};

    let tbl0 = Hwp5TableTarget {
        table_index: 0,
        sec: None,
        para: None,
        ctrl: None,
    };

    // Case 1: 2x2 sub-grid split on cell (0,0) of a 2x2 table -> 3x3 table with 7 cells:
    // row 0: (0,0), (0,1), (0,2)[rowSpan=2]; row 1: (1,0), (1,1); row 2: (2,0)[colSpan=2], (2,2).
    let plan_2d = Hwp5EditPlan {
        is_new_document: true,
        steps: vec![
            Hwp5EditStep::AppendTable {
                headers: vec![],
                rows: vec![
                    vec!["A".into(), "B".into()],
                    vec!["C".into(), "D".into()],
                ],
            },
            Hwp5EditStep::SplitCell {
                target: tbl0.clone(),
                row: 0,
                col: 0,
                into_rows: 2,
                into_cols: 2,
            },
        ],
    };
    let res_2d = execute_hwp5_plan(None, &plan_2d).unwrap();
    let mut cfb = Hwp5Container::open_bytes(&res_2d.bytes).unwrap();
    let sec = SectionDoc::parse(&cfb.read_section(0).unwrap()).unwrap();
    let tbl = &sec.enumerate_tables().unwrap()[0];
    assert_eq!((tbl.rows, tbl.cols, tbl.cells.len()), (3, 3, 7));

    let tbody = &sec.records[tbl.table_rec_idx].body;
    assert_eq!(u16::from_le_bytes(tbody[18..20].try_into().unwrap()), 3);
    assert_eq!(u16::from_le_bytes(tbody[20..22].try_into().unwrap()), 2);
    assert_eq!(u16::from_le_bytes(tbody[22..24].try_into().unwrap()), 2);
    let _ = tag::TABLE;

    // Verify full table re-parses cleanly in Node extract_text.js
    let tmp = tempfile::tempdir().unwrap();
    let p1 = tmp.path().join("split_2d.hwp");
    std::fs::write(&p1, &res_2d.bytes).unwrap();
    let node_2d = run_node_inspect(&p1);
    assert_eq!(node_2d.cell_count, 7);

    // Case 2: cell (0,0) merged vertically over rows 0..=1, then split cell (1,1) into 2 rows:
    // external spanning cell (0,0) must grow row_span 2 -> 3.
    let plan_span = Hwp5EditPlan {
        is_new_document: true,
        steps: vec![
            Hwp5EditStep::AppendTable {
                headers: vec![],
                rows: vec![
                    vec!["SpanCol0".into(), "TopRight".into()],
                    vec!["".into(), "BotRight".into()],
                ],
            },
            Hwp5EditStep::MergeCells {
                target: tbl0.clone(),
                from_row: 0,
                from_col: 0,
                to_row: 1,
                to_col: 0,
            },
            Hwp5EditStep::SplitCell {
                target: tbl0,
                row: 1,
                col: 1,
                into_rows: 2,
                into_cols: 1,
            },
        ],
    };
    let res_span = execute_hwp5_plan(None, &plan_span).unwrap();
    let mut cfb2 = Hwp5Container::open_bytes(&res_span.bytes).unwrap();
    let sec2 = SectionDoc::parse(&cfb2.read_section(0).unwrap()).unwrap();
    let tbl2 = &sec2.enumerate_tables().unwrap()[0];
    assert_eq!((tbl2.rows, tbl2.cols, tbl2.cells.len()), (3, 2, 4));
    assert_eq!(tbl2.cells[0].attr.row_span, 3);

    let p2 = tmp.path().join("split_span.hwp");
    std::fs::write(&p2, &res_span.bytes).unwrap();
    let node_span = run_node_inspect(&p2);
    assert_eq!(node_span.cell_count, 4);
}

