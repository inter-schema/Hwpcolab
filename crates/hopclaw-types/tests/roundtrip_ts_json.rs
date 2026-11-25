use hopclaw_types::*;
use serde_json::json;

#[test]
fn roundtrip_all_33_unified_ops_from_ts_json() {
    let ts_ops = json!([
        { "type": "replace_body_text", "find": "old", "replace": "new" },
        { "type": "set_paragraph_text", "paraIndex": 2, "text": "본문 수정", "oldTextForHwp": "기존" },
        { "type": "append_paragraph", "text": "추가 문단", "align": "JUSTIFY" },
        { "type": "append_heading", "level": 1, "text": "제1장 개요" },
        { "type": "set_cell_text", "tableIndex": 0, "sec": 0, "para": 3, "ctrl": 1, "row": 1, "col": 2, "text": "홍길동", "fit": true },
        { "type": "insert_table", "afterParaIndex": 0, "headers": ["항목", "내용"], "rows": [["A", "B"]] },
        { "type": "insert_nested_table", "tableIndex": 0, "row": 1, "col": 0, "rows": 2, "cols": 2, "data": [["x", "y"], ["1", "2"]], "headerRow": true, "widthMode": "fit_cell" },
        { "type": "insert_table_row", "tableIndex": 0, "row": 1, "position": "below", "cells": ["a", "b"] },
        { "type": "delete_table_row", "tableIndex": 0, "row": 1 },
        { "type": "insert_table_col", "tableIndex": 0, "col": 0, "position": "right", "cells": ["H", "V"] },
        { "type": "delete_table_col", "tableIndex": 0, "col": 1 },
        { "type": "merge_cells", "tableIndex": 0, "fromRow": 0, "fromCol": 0, "toRow": 1, "toCol": 1 },
        { "type": "unmerge_cells", "tableIndex": 0, "row": 0, "col": 0 },
        { "type": "split_cell", "tableIndex": 0, "row": 0, "col": 0, "intoRows": 2, "intoCols": 2 },
        { "type": "equalize_table", "tableIndex": 0, "mode": "both" },
        { "type": "set_cell_background", "tableIndex": 0, "row": 0, "col": 0, "color": "#EAEAEA" },
        { "type": "set_cell_align", "tableIndex": 0, "row": 0, "col": 0, "endRow": 0, "endCol": 2, "align": "center", "valign": "middle" },
        { "type": "set_cell_border", "tableIndex": 0, "row": 0, "col": 0, "sides": "all", "color": "#000000", "widthMm": 0.4, "lineType": "solid" },
        { "type": "apply_text_style", "targetText": "중요", "fontFamily": "함초롬바탕", "sizePt": 11.0, "bold": true, "italic": false, "underline": true, "color": "#FF0000", "highlight": "#FFFF00", "letterSpacing": -5.0, "charRatio": 100.0 },
        { "type": "apply_paragraph_style", "paraIndex": "last", "targetText": "1. 개요", "align": "DISTRIBUTE", "indentHu": 1000.0, "lineSpacingPercent": 160.0, "spacingBeforeHu": 200.0, "spacingAfterHu": 200.0 },
        { "type": "set_list_style", "paraIndex": 3, "targetText": "항목", "mode": "numbered" },
        { "type": "insert_image", "path": "stamp.png", "widthMm": 20.0, "heightMm": 20.0, "cell": { "tableIndex": 0, "row": 0, "col": 1 } },
        { "type": "place_seal", "anchorText": "(인)", "sealImagePath": "seal.png", "mode": "overlap" },
        { "type": "set_header_footer", "where": "header", "text": "대외비" },
        { "type": "insert_page_number", "where": "footer", "align": "center" },
        { "type": "insert_footnote", "anchorText": "주1", "text": "세부 설명", "paraIndex": 1 },
        { "type": "set_columns", "count": 2, "spacingMm": 8.0 },
        { "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 45.0, "keepTableWidth": true },
        { "type": "set_row_height", "tableIndex": 0, "row": 2, "heightMm": 16.0 },
        { "type": "set_cell_padding", "tableIndex": 0, "row": 1, "col": 1, "leftMm": 1.0, "rightMm": 1.0, "topMm": 1.0, "bottomMm": 1.0 },
        { "type": "fit_table_to_page", "tableIndex": 0, "targetWidthMm": 150.0 },
        { "type": "set_page_margin", "leftMm": 15.0, "rightMm": 15.0, "topMm": 20.0, "bottomMm": 15.0 },
        { "type": "shrink_cell_font", "tableIndex": 0, "row": 1, "col": 1, "sizePt": 9.0, "minPt": 8.0 }
    ]);

    let parsed: Vec<UnifiedOp> = serde_json::from_value(ts_ops).expect("all 33 UnifiedOp variants must parse");
    assert_eq!(parsed.len(), 33);

    // Also verify array BorderSides and boolean highlight round-trip
    let border_array = json!({
        "type": "set_cell_border",
        "tableIndex": 0,
        "row": 0,
        "col": 0,
        "sides": ["top", "bottom"],
        "lineType": "dashed"
    });
    let op: UnifiedOp = serde_json::from_value(border_array).unwrap();
    let reserialized = serde_json::to_value(&op).unwrap();
    assert_eq!(reserialized["sides"], json!(["top", "bottom"]));

    let reserialized_all = serde_json::to_value(&parsed).unwrap();
    let reparsed: Vec<UnifiedOp> = serde_json::from_value(reserialized_all).unwrap();
    assert_eq!(parsed, reparsed);
}

#[test]
fn roundtrip_diff_item_with_optional_base_revision() {
    let ts_diff = json!({
        "id": "diff-1",
        "status": "pending",
        "scope": "table_structure",
        "title": "표 1 행 추가",
        "beforeText": "",
        "afterText": "신규 행",
        "unifiedOps": [
            { "type": "insert_table_row", "tableIndex": 0, "row": 1, "position": "below", "cells": ["a", "b"] }
        ],
        "targetCoord": { "sec": 0, "para": 2, "tableIndex": 0, "row": 1, "col": 0 },
        "bbox": { "pageIndex": 0, "x": 10.5, "y": 20.5, "w": 120.0, "h": 24.0 },
        "verificationStatus": "warning_unresolved",
        "warnings": ["셀 여백 확인"],
        "metricsDelta": { "beforeOverflowMm": 4.2, "afterOverflowMm": 0.0, "heightAfterMm": 18.0 },
        "baseRevision": 3
    });
    let item: DiffItem = serde_json::from_value(ts_diff.clone()).unwrap();
    assert_eq!(item.base_revision, Some(3));
    assert_eq!(serde_json::to_value(&item).unwrap(), ts_diff);

    // Legacy TS payload without baseRevision must also deserialize
    let mut legacy = ts_diff;
    legacy.as_object_mut().unwrap().remove("baseRevision");
    let legacy_item: DiffItem = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy_item.base_revision, None);
}

#[test]
fn roundtrip_hwp_session_and_table_selection_types() {
    let meta_json = json!({
        "tableIndex": 1,
        "sec": 0,
        "para": 4,
        "ctrl": 0,
        "rowCount": 2,
        "colCount": 2,
        "parentTableIndex": 0,
        "parentCell": { "row": 1, "col": 0 },
        "depth": 1,
        "cells": [
            {
                "idx": 0,
                "row": 0,
                "col": 0,
                "rowSpan": 1,
                "colSpan": 1,
                "text": "중첩 셀",
                "bbox": {
                    "cellIdx": 0,
                    "row": 0,
                    "col": 0,
                    "rowSpan": 1,
                    "colSpan": 1,
                    "pageIndex": 0,
                    "x": 12.0,
                    "y": 34.0,
                    "w": 80.0,
                    "h": 24.0
                }
            }
        ]
    });
    let meta: TableCoordinateMeta = serde_json::from_value(meta_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&meta).unwrap(), meta_json);

    let sel_json = json!({
        "kind": "body_range",
        "sec": 0,
        "startPara": 1,
        "startOffset": 0,
        "endPara": 1,
        "endOffset": 5,
        "selectedText": "테스트",
        "rects": [{ "pageIndex": 0, "x": 10.0, "y": 15.0, "width": 50.0, "height": 14.0 }]
    });
    let sel: SelectionTarget = serde_json::from_value(sel_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&sel).unwrap(), sel_json);

    let session_json = json!({
        "original_path": "C:\\docs\\sample.hwpx",
        "staging_path": "C:\\cache\\hop-staging\\staging_1.hwpx",
        "file_name": "sample.hwpx",
        "is_hwpx": true,
        "bytes": [80, 75, 3, 4],
        "session_id": "sess-1",
        "revision": 0
    });
    let sess: OpenedDocSession = serde_json::from_value(session_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&sess).unwrap(), session_json);

    // Also accept camelCase sessionId alias
    let sess_alias: OpenedDocSession = serde_json::from_value(json!({
        "original_path": "",
        "staging_path": "s.hwp",
        "file_name": "새 문서.hwp",
        "is_hwpx": false,
        "bytes": [],
        "sessionId": "sess-2"
    }))
    .unwrap();
    assert_eq!(sess_alias.session_id.as_deref(), Some("sess-2"));
}

#[test]
fn roundtrip_layout_agent_memory_attachment_and_errors() {
    let report_json = json!({
        "timestamp": 1700000000000u64,
        "tableCount": 1,
        "issuesCount": 1,
        "issues": [{
            "id": "cell:0:1:0:2:v_overflow",
            "code": "CELL_OVERFLOW_V",
            "severity": "error",
            "message": "세로 넘침",
            "cell": { "tableIndex": 0, "sec": 0, "para": 1, "ctrl": 0, "row": 1, "col": 0, "cellIdx": 2 },
            "tableIndex": 0,
            "metrics": { "cellHeightMm": 10.0, "contentHeightMm": 14.5, "overflowMm": 4.5, "lines": 3 },
            "text": "긴 내용"
        }],
        "tables": [{
            "tableIndex": 0,
            "section": 0,
            "para": 1,
            "ctrl": 0,
            "cellCount": 1,
            "tableWidthMm": 140.0,
            "printableWidthMm": 150.0,
            "cells": [{
                "cellIdx": 2,
                "row": 1,
                "col": 0,
                "rowSpan": 1,
                "colSpan": 1,
                "widthMm": 50.0,
                "heightMm": 10.0,
                "availWidthMm": 47.0,
                "availHeightMm": 8.0,
                "estimatedLines": 3,
                "contentHeightMm": 14.5,
                "isVerticalOverflow": true,
                "isHorizontalOverflow": false,
                "overflowAmountMm": 4.5,
                "text": "긴 내용"
            }]
        }]
    });
    let report: LayoutReport = serde_json::from_value(report_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&report).unwrap(), report_json);

    let cmp_json = json!({
        "ok": true,
        "resolvedIssues": report_json["issues"],
        "remainingIssues": [],
        "newIssues": [],
        "summaryText": "해결된 이슈: 1건"
    });
    let cmp: LayoutComparison = serde_json::from_value(cmp_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&cmp).unwrap(), cmp_json);

    let step_json = json!({ "type": "verify", "message": "검증 완료", "detail": "0건" });
    let step: AgentStepEvent = serde_json::from_value(step_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&step).unwrap(), step_json);

    let mem_doc_json = json!({
        "meta": {
            "schemaVersion": 1,
            "id": "mem-1",
            "name": "행정기관 공문서",
            "description": "표준",
            "revision": 2,
            "createdAt": "2026-11-25T00:00:00Z",
            "updatedAt": "2026-11-25T01:00:00Z",
            "sourceDocs": [{ "name": "doc.hwpx", "sha256_8": "abcdef12" }],
            "masked": true
        },
        "body": "## 문서 유형·양식\n공문서"
    });
    let mem_doc: MemoryDoc = serde_json::from_value(mem_doc_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&mem_doc).unwrap(), mem_doc_json);
    assert_eq!(FIXED_MEMORY_SECTIONS.len(), 7);

    let sec_diff_json = json!({
        "sectionTitle": "어투·문체",
        "beforeContent": "이전",
        "afterContent": "이후",
        "changed": true,
        "tokens": [
            { "op": -1, "text": "이전" },
            { "op": 1, "text": "이후" },
            { "op": 0, "text": " 유지" }
        ]
    });
    let sec_diff: SectionDiff = serde_json::from_value(sec_diff_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&sec_diff).unwrap(), sec_diff_json);

    let att_json = json!({
        "id": "att-1",
        "fileName": "ref.xlsx",
        "filePath": "ref.xlsx",
        "kind": "excel",
        "markdownSummary": "| A | B |",
        "structuredData": { "sheets": 1 }
    });
    let att: ParsedAttachment = serde_json::from_value(att_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&att).unwrap(), att_json);

    let claw_json = json!({ "ok": true, "stdout_json": { "status": "success" }, "stderr": "" });
    let claw: ClawExecResult = serde_json::from_value(claw_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&claw).unwrap(), claw_json);

    let codes = [
        ErrorCode::E_DOC_INVALID,
        ErrorCode::E_TABLE_NOT_FOUND,
        ErrorCode::E_OP_UNSUPPORTED_FORMAT,
        ErrorCode::E_GUARD_REJECTED,
        ErrorCode::E_STALE_REVISION,
        ErrorCode::E_TIMEOUT,
        ErrorCode::E_CANCELLED,
        ErrorCode::E_LLM_HTTP,
        ErrorCode::E_ENDPOINT_POLICY,
        ErrorCode::E_ATTACHMENT_LIMIT,
        ErrorCode::E_INTERNAL_PANIC,
    ];
    for code in codes {
        let err = ErrorModel::new(code, "오류").with_detail("상세");
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], json!(code.as_str()));
        let back: ErrorModel = serde_json::from_value(v).unwrap();
        assert_eq!(back, err);
    }
}

struct MockWalker(Vec<TableCoordinateMeta>);

impl TableWalker for MockWalker {
    fn walk_tables(&self) -> Result<Vec<TableCoordinateMeta>, ErrorModel> {
        Ok(self.0.clone())
    }
}

#[test]
fn edit_plan_units_and_table_walker_contract() {
    assert_eq!(mm_to_hwpunit(10.0), 2835);
    assert_eq!(pt_to_hwpunit(11.0), 1100);

    let plan = EditPlan::Hwpx(HwpxEditPlan {
        is_new_document: false,
        steps: vec![HwpxEditStep::SetColWidth {
            table_index: 0,
            col: 1,
            width_hu: mm_to_hwpunit(45.0),
            keep_table_width: true,
        }],
    });
    let v = serde_json::to_value(&plan).unwrap();
    assert_eq!(v["format"], "hwpx");
    let back: EditPlan = serde_json::from_value(v).unwrap();
    assert_eq!(back, plan);

    let walker = MockWalker(vec![
        TableCoordinateMeta {
            table_index: 0,
            sec: 0,
            para: 1,
            ctrl: 0,
            row_count: 2,
            col_count: 2,
            parent_table_index: None,
            parent_cell: None,
            depth: Some(0),
            cells: vec![],
        },
        TableCoordinateMeta {
            table_index: 1,
            sec: 0,
            para: 1,
            ctrl: 0,
            row_count: 2,
            col_count: 2,
            parent_table_index: Some(0),
            parent_cell: Some(ParentCellCoord { row: 1, col: 0 }),
            depth: Some(1),
            cells: vec![],
        },
    ]);

    let walked = walker.walk_tables().unwrap();
    validate_preorder_coords(&walked).unwrap();
    let coord1 = walker.resolve_table_coord(1).unwrap();
    assert_eq!(coord1.parent_table_index, Some(0));
    assert_eq!(coord1.depth, 1);

    let missing = walker.resolve_table_coord(99).unwrap_err();
    assert_eq!(missing.code, ErrorCode::E_TABLE_NOT_FOUND);
}
