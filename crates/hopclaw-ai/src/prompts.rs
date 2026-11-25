use std::path::Path;

pub const EMBEDDED_SYSTEM_PROMPT: &str = include_str!("../prompts/system_prompt.txt");

/// Loads the system prompt, checking `HOPCLAW_PROMPTS_DIR/system_prompt.txt`
/// for dev hot-reload when set, and falling back to the embedded prompt.
pub fn load_system_prompt() -> String {
    if let Ok(dir) = std::env::var("HOPCLAW_PROMPTS_DIR") {
        let candidate = Path::new(&dir).join("system_prompt.txt");
        if let Ok(text) = std::fs::read_to_string(candidate) {
            return text.replace("\r\n", "\n");
        }
    }
    EMBEDDED_SYSTEM_PROMPT.replace("\r\n", "\n")
}

pub fn inline_quick_action_prompt(action: &str) -> Option<&'static str> {
    match action {
        "official_tone" => Some(
            "선택된 영역의 문장을 대한민국 행정기관 공문서 표준 개조식 어투(~함, ~기 바람, ~조치 요망, ~추진코자 함 등)로 변환하라.
- 하위 항목이 나열된 경우 행정업무운영 편람 순서(1. -> 가. -> 1) -> 가) -> (1) -> (가))를 준수하라.
- 절대로 텍스트 내부에 **굵게** 같은 마크다운 기호를 넣지 마라.",
        ),
        "proofread" => Some(
            "선택된 영역의 맞춤법, 띄어쓰기, 어색한 비문을 문맥에 맞게 교정하라. 원래의 핵심 의미와 문서 서식 구조는 그대로 유지하라.",
        ),
        "summarize_bullet" => Some(
            "선택된 장문의 단락을 핵심 내용 위주의 개조식 글머리/번호 목록으로 간결하게 축약하라.",
        ),
        "fill_or_calc_table" => Some(
            "사용자가 선택한 표의 셀 영역을 채우거나 계산하라.
- 선택된 셀들의 행/열 머리글(Header)과 주변 셀 데이터, 그리고 첨부된 외부 참고자료(Excel/PDF/HWP)가 있다면 이를 대조하여 정확한 값을 각 셀(tableIndex, row, col)의 set_cell_text 연산으로 생성하라.
- 합계/평균/비율 칸인 경우 정확한 수치 계산 결과를 기입하고 기존 숫자 표기 형식(천 단위 콤마, 단위 등)을 유지하라.
- 각 셀의 변경은 사용자가 개별 수락할 수 있도록 셀마다 별도의 diffItem으로 분리하라.",
        ),
        _ => None,
    }
}
