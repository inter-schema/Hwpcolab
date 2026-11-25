use serde_json::json;

use crate::types::NeutralToolSpec;

pub fn inspect_layout_tool_spec() -> NeutralToolSpec {
    NeutralToolSpec {
        name: "inspect_layout".to_string(),
        description: "현재 문서(또는 시험 적용 중인 문서)의 표 및 셀 레이아웃 측정값, 오버플로/넘침 수치를 정밀 조회합니다.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "tableIndex": { "type": "number", "description": "조회할 표 인덱스 (0-based, 생략 시 전체 표)" },
                "row": { "type": "number", "description": "조회할 셀 행 (0-based)" },
                "col": { "type": "number", "description": "조회할 셀 열 (0-based)" }
            }
        }),
    }
}

pub fn read_document_tool_spec() -> NeutralToolSpec {
    NeutralToolSpec {
        name: "read_document".to_string(),
        description: "문서 본문의 특정 문단 범위 또는 특정 표의 셀 데이터를 부분 조회합니다. 긴 문서(12,000자 초과)에서 필요한 세부 내용을 읽을 때 사용합니다.".to_string(),
        parameters: json!({
            "type": "object",
            "required": ["kind"],
            "properties": {
                "kind": {
                    "type": "string",
                    "enum": ["paragraphs", "table"],
                    "description": "조회할 대상 종류 ('paragraphs' 문단 또는 'table' 표)"
                },
                "start": { "type": "number", "description": "조회 시작 인덱스 (0-based)" },
                "count": { "type": "number", "description": "조회할 문단 또는 행 수 (기본 5)" },
                "tableIndex": { "type": "number", "description": "kind가 'table'일 때 조회할 표 인덱스 (0-based)" }
            }
        }),
    }
}

pub fn apply_trial_edits_tool_spec() -> NeutralToolSpec {
    NeutralToolSpec {
        name: "apply_trial_edits".to_string(),
        description: "임시 복사본 파일에 시험 편집 연산을 시뮬레이션 적용하고 레이아웃 변화 및 넘침 해결 여부를 실측 비교합니다. 기본적으로 누적(cumulative) 적용되며, reset: true로 원본으로 복구할 수 있습니다. (set_cell_align 등 모든 UnifiedOp 지원)".to_string(),
        parameters: json!({
            "type": "object",
            "required": ["reason", "ops"],
            "properties": {
                "reason": { "type": "string", "description": "시험 적용 목적 및 가설" },
                "ops": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["type"],
                        "properties": {
                            "type": {
                                "type": "string",
                                "description": "연산 종류 (set_cell_align, set_col_width, set_row_height, set_cell_text 등)"
                            },
                            "tableIndex": { "type": "number", "description": "표 인덱스 (0-based)" },
                            "row": { "type": "number", "description": "행 인덱스 (0-based)" },
                            "col": { "type": "number", "description": "열 인덱스 (0-based)" },
                            "endRow": { "type": "number", "description": "set_cell_align 끝 행 (포함)" },
                            "endCol": { "type": "number", "description": "set_cell_align 끝 열 (포함)" },
                            "align": { "type": "string", "enum": ["LEFT", "CENTER", "RIGHT", "JUSTIFY"], "description": "가로 정렬" },
                            "valign": { "type": "string", "enum": ["TOP", "CENTER", "BOTTOM"], "description": "세로 정렬" }
                        }
                    },
                    "description": "시험 적용할 UnifiedOp 배열"
                },
                "reset": {
                    "type": "boolean",
                    "description": "true이면 이전 시험 변경사항을 모두 취소하고 원본 상태로 복귀 후 연산 적용"
                }
            }
        }),
    }
}

pub fn propose_hwp_edits_tool_spec() -> NeutralToolSpec {
    NeutralToolSpec {
        name: "propose_hwp_edits".to_string(),
        description: "HWP/HWPX 문서에 대한 부분 수정 또는 생성 작업(DiffItem 배열)을 제안합니다. 표 생성(insert_table)이나 표 셀 범위 정렬(set_cell_align) 등 서로 연관된 일련의 작업은 하나의 논리적 diffItem(예: 표 삽입 1개 diffItem, 셀 범위 정렬 1~2개 diffItem)으로 묶어서 반환하십시오. 불필요하게 셀마다 개별 카드를 생성하지 마십시오.".to_string(),
        parameters: json!({
            "type": "object",
            "required": ["summary", "diffItems"],
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "사이드바에 표시할 전체 수정 사항 요약 메시지"
                },
                "diffItems": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["scope", "title", "beforeText", "afterText", "unifiedOps"],
                        "properties": {
                            "scope": {
                                "type": "string",
                                "enum": ["paragraph", "cell", "table_structure", "style", "document"]
                            },
                            "title": { "type": "string", "description": "변경 위치 요약 (예: '표 1 (2행 3열)', '문단 2', '표 1 정렬')" },
                            "beforeText": { "type": "string", "description": "변경 전 원본 텍스트 (신규 추가 시 빈 문자열)" },
                            "afterText": { "type": "string", "description": "변경 후 텍스트 (시각적 Diff 표시용)" },
                            "targetCoord": {
                                "type": "object",
                                "properties": {
                                    "sec": { "type": "number" },
                                    "para": { "type": "number" },
                                    "tableIndex": { "type": "number" },
                                    "row": { "type": "number" },
                                    "col": { "type": "number" }
                                }
                            },
                            "unifiedOps": {
                                "type": "array",
                                "description": "이 DiffItem을 실제로 반영하기 위한 UnifiedOp 배열 (set_cell_align 포함)",
                                "items": {
                                    "type": "object",
                                    "required": ["type"],
                                    "properties": {
                                        "type": {
                                            "type": "string",
                                            "description": "연산 종류 (set_cell_align, replace_body_text, set_cell_text, insert_table 등)"
                                        },
                                        "tableIndex": { "type": "number", "description": "표 인덱스 (0-based)" },
                                        "row": { "type": "number", "description": "행 인덱스 (0-based)" },
                                        "col": { "type": "number", "description": "열 인덱스 (0-based)" },
                                        "endRow": { "type": "number", "description": "set_cell_align 끝 행 (포함)" },
                                        "endCol": { "type": "number", "description": "set_cell_align 끝 열 (포함)" },
                                        "align": { "type": "string", "enum": ["LEFT", "CENTER", "RIGHT", "JUSTIFY"], "description": "가로 정렬" },
                                        "valign": { "type": "string", "enum": ["TOP", "CENTER", "BOTTOM"], "description": "세로 정렬" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    }
}

pub fn default_v2_tools() -> Vec<NeutralToolSpec> {
    vec![
        inspect_layout_tool_spec(),
        read_document_tool_spec(),
        apply_trial_edits_tool_spec(),
        propose_hwp_edits_tool_spec(),
    ]
}

/// Full `propose_hwp_edits` tool schema matching `PROPOSE_HWP_EDITS_TOOL` in
/// `src/services/aiOrchestrator.ts` (all 33 UnifiedOp types and parameter fields).
pub fn propose_hwp_edits_full_tool_spec() -> NeutralToolSpec {
    let parameters = serde_json::from_str(include_str!("../prompts/propose_hwp_edits_schema.json"))
        .expect("embedded propose_hwp_edits_schema.json must be valid JSON");
    NeutralToolSpec {
        name: "propose_hwp_edits".to_string(),
        description: "HWP/HWPX 문서에 대한 부분 수정 또는 생성 작업(DiffItem 배열)을 제안합니다. 표 생성(insert_table)이나 표 셀 범위 정렬(set_cell_align) 등 연관된 일련의 작업은 논리적 diffItem(예: 표 삽입 1개 diffItem, 셀 범위 정렬 1~2개 diffItem)으로 묶어서 반환하십시오. 불필요하게 셀마다 개별 카드를 생성하지 마십시오.".to_string(),
        parameters,
    }
}
