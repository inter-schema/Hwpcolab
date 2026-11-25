use std::path::PathBuf;

use hopclaw_hwp5::{
    decode_para_text, encode_para_text, id_slot, parse_hex_color, parse_records, serialize_records,
    tag, BorderFillBuilder, BorderLineType, BorderSide, CharShapePatch, DiagonalDirection,
    DocInfoDoc, Hwp5Container, LineSpacingType, ParaAlignment, ParaShapePatch, SectionDoc,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture_paths() -> Vec<PathBuf> {
    let root = repo_root();
    vec![
        root.join("assets/templates/blank.hwp"),
        root.join("samples/demo_official_report.hwp"),
        root.join("samples/(날짜)(교시)(과목명)교수.학습과정안(이름).hwp"),
    ]
}

#[test]
fn roundtrip_records_are_byte_identical_across_samples() {
    for path in fixture_paths() {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut container = Hwp5Container::open_bytes(&bytes)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));

        let docinfo_plain = container.read_docinfo().expect("read DocInfo");
        let di_records = parse_records(&docinfo_plain).expect("parse DocInfo records");
        let di_reserialized = serialize_records(&di_records).expect("serialize DocInfo records");
        assert_eq!(
            di_reserialized,
            docinfo_plain,
            "DocInfo record round-trip mismatch in {}",
            path.display()
        );

        let sections = container.section_paths();
        assert!(!sections.is_empty(), "expected at least 1 section in {}", path.display());
        for sec_path in sections {
            let sec_plain = container.read_record_stream(&sec_path).expect("read section");
            let sec_records = parse_records(&sec_plain).expect("parse section records");
            let sec_reserialized =
                serialize_records(&sec_records).expect("serialize section records");
            assert_eq!(
                sec_reserialized,
                sec_plain,
                "{sec_path} record round-trip mismatch in {}",
                path.display()
            );
        }
    }
}

#[test]
fn container_rewrite_preserves_streams_and_does_not_inject_sheetjs() {
    let blank_path = repo_root().join("assets/templates/blank.hwp");
    let orig_bytes = std::fs::read(&blank_path).expect("read blank.hwp");

    let mut container = Hwp5Container::open_bytes(&orig_bytes).expect("open blank.hwp");
    assert!(container.file_header().is_compressed());
    assert!(!container.has_stream("/Sh33tJ5"));

    let orig_streams = container.stream_paths();
    let docinfo_plain = container.read_docinfo().expect("read DocInfo");
    let sec0_plain = container.read_section(0).expect("read Section0");

    // Re-write DocInfo and Section0 untouched (compressed at level 9) and serialize CFB.
    container.write_docinfo(&docinfo_plain).expect("write DocInfo");
    container.write_section(0, &sec0_plain).expect("write Section0");
    let rewritten_bytes = container.into_bytes().expect("into_bytes");

    let mut reopened = Hwp5Container::open_bytes(&rewritten_bytes).expect("reopen rewritten");
    assert!(!reopened.has_stream("/Sh33tJ5"));
    assert_eq!(reopened.stream_paths(), orig_streams);
    assert_eq!(reopened.read_docinfo().expect("re-read DocInfo"), docinfo_plain);
    assert_eq!(reopened.read_section(0).expect("re-read Section0"), sec0_plain);
}

#[test]
fn docinfo_allocates_and_syncs_borderfill_charshape_parashape_facename() {
    for path in fixture_paths() {
        let bytes = std::fs::read(&path).expect("read hwp");
        let mut container = Hwp5Container::open_bytes(&bytes).expect("open");
        let di_plain = container.read_docinfo().expect("DocInfo");
        let mut di = DocInfoDoc::parse(&di_plain).expect("parse DocInfo");

        // 1. BorderFill: deduplicate & append (1-based ID)
        let bf_before = di.border_fills().len();
        let base_bf = di.get_border_fill(1).unwrap_or(&[]).to_vec();
        let blue = parse_hex_color("#1E88E5").expect("hex");
        let with_fill = BorderFillBuilder::merge_solid_fill(&base_bf, blue, true);
        let with_border = BorderFillBuilder::merge_borders(
            &with_fill,
            &[BorderSide::Top, BorderSide::Bottom],
            BorderLineType::Solid as u8,
            1,
            0x0000_0000,
        );
        let final_bf = BorderFillBuilder::merge_diagonal(
            &with_border,
            DiagonalDirection::Slash,
            BorderLineType::Solid as u8,
            1,
            parse_hex_color("#FF0000").unwrap(),
        );
        let bf_id_1 = di.find_or_add_border_fill(final_bf.clone()).expect("add BF");
        let bf_id_2 = di.find_or_add_border_fill(final_bf).expect("dedup BF");
        assert_eq!(bf_id_1, bf_id_2, "duplicate BorderFill must reuse ID");
        assert_eq!(bf_id_1 as usize, bf_before + 1);
        assert_eq!(
            di.id_mapping_count(id_slot::BORDER_FILL).unwrap() as usize,
            bf_before + 1
        );

        // 2. CharShape: preserves 70 vs 74-byte size + updates ID_MAPPINGS
        let cs_before = di.char_shapes().len();
        let base_cs = di.get_char_shape(0).expect("base CS 0").to_vec();
        let face_ids = di.find_or_add_face_name("Malgun Gothic Test").expect("face");
        let patched_cs = CharShapePatch {
            face_ids: Some(face_ids),
            size_pt: Some(13.5),
            bold: Some(true),
            text_color_bgr: Some(parse_hex_color("#223344").unwrap()),
            ..Default::default()
        }
        .apply_to(&base_cs)
        .expect("patch CS");
        assert_eq!(patched_cs.len(), base_cs.len());
        let cs_id_1 = di.find_or_add_char_shape(patched_cs.clone()).expect("add CS");
        let cs_id_2 = di.find_or_add_char_shape(patched_cs).expect("dedup CS");
        assert_eq!(cs_id_1, cs_id_2);
        assert_eq!(cs_id_1 as usize, cs_before);
        assert_eq!(
            di.id_mapping_count(id_slot::CHAR_SHAPE).unwrap() as usize,
            cs_before + 1
        );

        // 3. ParaShape: supports >=46-byte bases and emits 58 bytes + updates ID_MAPPINGS
        let ps_before = di.para_shapes().len();
        let base_ps = di.get_para_shape(0).expect("base PS 0").to_vec();
        let patched_ps = ParaShapePatch {
            alignment: Some(ParaAlignment::Center),
            line_spacing_type: Some(LineSpacingType::Percent),
            line_spacing: Some(150),
            border_fill_id: Some(bf_id_1),
            ..Default::default()
        }
        .apply_to(&base_ps)
        .expect("patch PS");
        assert_eq!(patched_ps.len(), 58);
        let ps_id_1 = di.find_or_add_para_shape(patched_ps.clone()).expect("add PS");
        let ps_id_2 = di.find_or_add_para_shape(patched_ps).expect("dedup PS");
        assert_eq!(ps_id_1, ps_id_2);
        assert_eq!(ps_id_1 as usize, ps_before);
        assert_eq!(
            di.id_mapping_count(id_slot::PARA_SHAPE).unwrap() as usize,
            ps_before + 1
        );

        // 4. Write DocInfo back to CFB and verify re-parse consistency
        let di_updated_bytes = di.serialize().expect("serialize DocInfo");
        container.write_docinfo(&di_updated_bytes).expect("write DocInfo");
        let out_bytes = container.into_bytes().expect("into_bytes");
        let mut reopened = Hwp5Container::open_bytes(&out_bytes).expect("reopen");
        let di2 = DocInfoDoc::parse(&reopened.read_docinfo().expect("read back"))
            .expect("reparse DocInfo");
        assert_eq!(di2.border_fills().len(), bf_before + 1);
        assert_eq!(di2.char_shapes().len(), cs_before + 1);
        assert_eq!(di2.para_shapes().len(), ps_before + 1);
    }
}

#[test]
fn section_traversal_inspects_blank_and_sample_tables() {
    // 1. Blank template: 1 section, 1 para, 0 tables, portrait A4 PageDef
    let blank_bytes = std::fs::read(repo_root().join("assets/templates/blank.hwp")).unwrap();
    let mut blank_cfb = Hwp5Container::open_bytes(&blank_bytes).unwrap();
    let blank_sec = SectionDoc::parse(&blank_cfb.read_section(0).unwrap()).unwrap();
    assert_eq!(blank_sec.top_level_para_indices().len(), 1);
    assert!(blank_sec.enumerate_tables().unwrap().is_empty());
    let pd = blank_sec.page_def().unwrap().expect("blank PAGE_DEF");
    assert_eq!(pd.width, 59527);
    assert_eq!(pd.height, 84188);
    assert!(!pd.is_landscape());

    // 2. Sample document with tables
    let report_bytes =
        std::fs::read(repo_root().join("samples/demo_official_report.hwp")).unwrap();
    let mut report_cfb = Hwp5Container::open_bytes(&report_bytes).unwrap();
    let mut report_sec = SectionDoc::parse(&report_cfb.read_section(0).unwrap()).unwrap();
    let tables = report_sec.enumerate_tables().unwrap();
    assert!(!tables.is_empty(), "demo_official_report.hwp must have tables");
    let cell00 = report_sec.locate_cell_by_grid(0, 0, 0).unwrap();
    assert_eq!(cell00.attr.row, 0);
    assert_eq!(cell00.attr.col, 0);

    // Check PARA_TEXT encode/decode round-trip + extended control skip behavior
    let (encoded_pt, wchar_len) = encode_para_text("한글 테스트\n둘째 줄");
    let decoded = decode_para_text(&encoded_pt).unwrap();
    assert_eq!(decoded.text, "한글 테스트\n둘째 줄");
    assert_eq!(decoded.wchar_len as u32, wchar_len);

    // Ensure normalize_last_para_flags leaves serialized stream valid
    report_sec.normalize_last_para_flags().unwrap();
    let sec_bytes = report_sec.serialize().unwrap();
    let reparsed = parse_records(&sec_bytes).unwrap();
    assert!(!reparsed.is_empty());
    assert!(reparsed.iter().any(|r| r.tag == tag::TABLE));
}
