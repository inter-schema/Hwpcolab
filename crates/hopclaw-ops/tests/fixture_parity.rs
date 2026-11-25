use std::path::PathBuf;

use hopclaw_ops::{
    build_official_doc_standard_ops, compile_edit_plan, extract_negative_constraints, lint_ops,
    lint_raw_json_ops, OpTranslator, PolicyGuardContext, RawPolicyGuardResult,
};
use hopclaw_types::{
    mm_to_hwpunit, pt_to_hwpunit, DocFormat, EditPlan, ErrorCode, Hwp5EditStep, HwpxEditStep,
    TableCoordinateMeta, UnifiedOp,
};
use serde::Deserialize;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("ops-guard-privacy")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpTranslatorFixture {
    payload_cases: Vec<PayloadCase>,
    error_cases: Vec<ErrorCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadCase {
    name: String,
    staging_file_path: String,
    is_hwpx: bool,
    ops: Vec<UnifiedOp>,
    tables_meta: Vec<TableCoordinateMeta>,
    is_new_document: bool,
    expected: ExpectedPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedPayload {
    is_hwpx: bool,
    payload: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ErrorCase {
    name: String,
    staging_file_path: String,
    is_hwpx: bool,
    ops: Vec<UnifiedOp>,
    tables_meta: Vec<TableCoordinateMeta>,
    is_new_document: bool,
    expected_error: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyGuardFixture {
    negative_constraint_cases: Vec<NegativeConstraintCase>,
    lint_cases: Vec<LintCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NegativeConstraintCase {
    prompt: String,
    expected: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LintCase {
    name: String,
    ops: Vec<Value>,
    context: PolicyGuardContext,
    expected: RawPolicyGuardResult,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialDocFixture {
    cases: Vec<OfficialDocCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialDocCase {
    name: String,
    paragraphs: Vec<String>,
    expected: Vec<UnifiedOp>,
}

#[test]
fn op_translator_fixture_parity() {
    let raw = std::fs::read_to_string(fixtures_dir().join("op_translator.json")).unwrap();
    let fixture: OpTranslatorFixture = serde_json::from_str(&raw).unwrap();

    for tc in fixture.payload_cases {
        let actual = OpTranslator::to_engine_payload(
            &tc.staging_file_path,
            tc.is_hwpx,
            &tc.ops,
            &tc.tables_meta,
            tc.is_new_document,
        )
        .unwrap_or_else(|e| panic!("unexpected error in {}: {e}", tc.name));

        assert_eq!(actual.is_hwpx, tc.expected.is_hwpx, "isHwpx mismatch in {}", tc.name);
        let actual_json: Value = serde_json::from_str(&actual.stdin_json).unwrap();
        assert_eq!(
            actual_json, tc.expected.payload,
            "payload JSON mismatch in {}",
            tc.name
        );
    }

    for ec in fixture.error_cases {
        let err = OpTranslator::to_engine_payload(
            &ec.staging_file_path,
            ec.is_hwpx,
            &ec.ops,
            &ec.tables_meta,
            ec.is_new_document,
        )
        .expect_err(&format!("expected error in {}", ec.name));
        assert_eq!(err.code, ErrorCode::E_OP_UNSUPPORTED_FORMAT);
        assert_eq!(err.message, ec.expected_error);
    }
}

#[test]
fn policy_guard_fixture_parity() {
    let raw = std::fs::read_to_string(fixtures_dir().join("policy_guard.json")).unwrap();
    let fixture: PolicyGuardFixture = serde_json::from_str(&raw).unwrap();

    for nc in fixture.negative_constraint_cases {
        let actual = extract_negative_constraints(&nc.prompt);
        assert_eq!(
            actual, nc.expected,
            "negative constraint mismatch for '{}'",
            nc.prompt
        );
    }

    for lc in fixture.lint_cases {
        let actual = lint_raw_json_ops(&lc.ops, &lc.context);
        assert_eq!(actual, lc.expected, "lint_raw_json_ops mismatch in {}", lc.name);

        // Also test strongly-typed `lint_ops` when all ops deserialize into UnifiedOp.
        if let Ok(typed_ops) = lc
            .ops
            .iter()
            .cloned()
            .map(serde_json::from_value::<UnifiedOp>)
            .collect::<Result<Vec<_>, _>>()
        {
            let typed_res = lint_ops(&typed_ops, &lc.context);
            assert_eq!(typed_res.passed, lc.expected.passed, "typed passed mismatch in {}", lc.name);
            assert_eq!(
                typed_res.warnings, lc.expected.warnings,
                "typed warnings mismatch in {}",
                lc.name
            );
            assert_eq!(
                typed_res.rejected_ops.len(),
                lc.expected.rejected_ops.len(),
                "typed rejected len mismatch in {}",
                lc.name
            );
            assert_eq!(
                typed_res.sanitized_ops.len(),
                lc.expected.sanitized_ops.len(),
                "typed sanitized len mismatch in {}",
                lc.name
            );
        }
    }
}

#[test]
fn official_doc_rules_fixture_parity() {
    let raw = std::fs::read_to_string(fixtures_dir().join("official_doc_rules.json")).unwrap();
    let fixture: OfficialDocFixture = serde_json::from_str(&raw).unwrap();

    for tc in fixture.cases {
        let actual = build_official_doc_standard_ops(&tc.paragraphs);
        assert_eq!(
            actual, tc.expected,
            "official_doc_rules mismatch in {}",
            tc.name
        );
    }
}

#[test]
fn compile_edit_plan_enforces_table_lookup_format_and_hwpunit_conversions() {
    let raw = std::fs::read_to_string(fixtures_dir().join("op_translator.json")).unwrap();
    let fixture: OpTranslatorFixture = serde_json::from_str(&raw).unwrap();

    // Compile all 33 ops for HWPX and all 32 ops for HWP5.
    let hwpx_case = fixture
        .payload_cases
        .iter()
        .find(|c| c.name == "hwpx_existing_all_33_ops")
        .unwrap();
    let hwpx_plan = compile_edit_plan(
        DocFormat::Hwpx,
        &hwpx_case.ops,
        &hwpx_case.tables_meta,
        false,
    )
    .unwrap();
    let EditPlan::Hwpx(hwpx) = hwpx_plan else {
        panic!("expected HwpxEditPlan");
    };
    assert!(hwpx.steps.iter().any(|s| matches!(
        s,
        HwpxEditStep::SetColWidth { width_hu, .. } if *width_hu == mm_to_hwpunit(40.0)
    )));
    assert!(hwpx.steps.iter().any(|s| matches!(
        s,
        HwpxEditStep::ShrinkCellFont { size_hu, min_hu, .. }
            if *size_hu == pt_to_hwpunit(9.0) && *min_hu == pt_to_hwpunit(8.0)
    )));

    let hwp5_case = fixture
        .payload_cases
        .iter()
        .find(|c| c.name == "hwp_new_all_32_ops")
        .unwrap();
    let hwp5_plan = compile_edit_plan(
        DocFormat::Hwp5,
        &hwp5_case.ops,
        &hwp5_case.tables_meta,
        true,
    )
    .unwrap();
    let EditPlan::Hwp5(hwp5) = hwp5_plan else {
        panic!("expected Hwp5EditPlan");
    };
    assert!(matches!(hwp5.steps.first(), Some(Hwp5EditStep::SetupDocument { .. })));

    // Missing tableIndex -> E_TABLE_NOT_FOUND (never fabricates 0,0,0)
    let missing_table_op = UnifiedOp::SetCellText {
        table_index: 5,
        sec: None,
        para: None,
        ctrl: None,
        row: 0,
        col: 0,
        text: "X".into(),
        fit: None,
    };
    let err = compile_edit_plan(
        DocFormat::Hwp5,
        &[missing_table_op],
        &hwpx_case.tables_meta,
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::E_TABLE_NOT_FOUND);

    // .hwp insert_nested_table -> E_OP_UNSUPPORTED_FORMAT
    let nested_op = UnifiedOp::InsertNestedTable {
        table_index: 0,
        row: 0,
        col: 0,
        rows: 2,
        cols: 2,
        data: None,
        header_row: None,
        width_mode: None,
    };
    let err = compile_edit_plan(
        DocFormat::Hwp5,
        &[nested_op],
        &hwpx_case.tables_meta,
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::E_OP_UNSUPPORTED_FORMAT);

    // Newly appended table in same batch can be targeted without tablesMeta
    let batch = vec![
        UnifiedOp::InsertTable {
            after_para_index: None,
            headers: vec!["A".into()],
            rows: vec![vec!["B".into()]],
        },
        UnifiedOp::SetCellText {
            table_index: 0,
            sec: None,
            para: None,
            ctrl: None,
            row: 0,
            col: 0,
            text: "Updated".into(),
            fit: None,
        },
    ];
    let batch_plan = compile_edit_plan(DocFormat::Hwp5, &batch, &[], false).unwrap();
    let EditPlan::Hwp5(batch_hwp5) = batch_plan else {
        panic!("expected Hwp5EditPlan");
    };
    assert_eq!(batch_hwp5.steps.len(), 2);
}
