import type { DiffItem, UnifiedOp } from "../types/operations.ts";
import type { TableCoordinateMeta, SelectionTarget } from "../types/hwp.ts";
import { type ExcludedRegion, PrivacySessionMasker } from "../core/privacyMasker.ts";
import {
  inspectDocumentBytes,
  formatLayoutReportForPrompt,
  compareLayoutReports,
  type LayoutReport,
  type LayoutIssue,
} from "../core/layoutInspector.ts";
import { OpTranslator } from "../core/opTranslator.ts";
import { runtimeInvoke } from "../core/runtimeInvoke.ts";
import { lintOps, type PolicyGuardResult } from "./policyGuard.ts";
import { callClaudeApi } from "./providers/claudeProvider.ts";
import {
  callOpenAiAgentTurn,
  callOpenAiCompatibleApi,
  type ReasoningEffortOption,
} from "./providers/openaiProvider.ts";
import { callGeminiApi } from "./providers/geminiProvider.ts";
import { callOllamaApi } from "./providers/ollamaProvider.ts";

export type ReasoningEffort = "none" | "low" | "medium" | "high" | "xhigh";

export interface AgentStepEvent {
  type: "inspect" | "trial" | "verify" | "guard" | "thinking" | "propose";
  message: string;
  detail?: string;
}

export const PROPOSE_HWP_EDITS_TOOL = {
  name: "propose_hwp_edits",
  description:
    "HWP/HWPX 문서에 대한 부분 수정 또는 생성 작업(DiffItem 배열)을 제안합니다. 각 변경 단위는 사용자가 화면에서 개별적으로 [✔ 적용] 또는 [✖ 되돌리기]를 선택할 수 있도록 문단 또는 표 셀 단위로 분리하여 반환하세요.",
  input_schema: {
    type: "object",
    required: ["summary", "diffItems"],
    properties: {
      summary: {
        type: "string",
        description: "사이드바에 표시할 전체 수정 사항 요약 메시지",
      },
      diffItems: {
        type: "array",
        items: {
          type: "object",
          required: ["scope", "title", "beforeText", "afterText", "unifiedOps"],
          properties: {
            scope: {
              type: "string",
              enum: ["paragraph", "cell", "table_structure", "style", "document"],
            },
            title: { type: "string", description: "변경 위치 요약 (예: '표 1 (2행 3열)', '문단 2')" },
            beforeText: { type: "string", description: "변경 전 원본 텍스트 (신규 추가 시 빈 문자열)" },
            afterText: { type: "string", description: "변경 후 텍스트 (시각적 Diff 표시용)" },
            targetCoord: {
              type: "object",
              properties: {
                sec: { type: "number" },
                para: { type: "number" },
                tableIndex: { type: "number" },
                row: { type: "number" },
                col: { type: "number" },
              },
            },
            unifiedOps: {
              type: "array",
              description: "이 DiffItem을 실제로 반영하기 위한 UnifiedOp 배열",
              items: {
                type: "object",
                required: ["type"],
                properties: {
                  type: {
                    type: "string",
                    enum: [
                      "replace_body_text",
                      "set_paragraph_text",
                      "append_paragraph",
                      "append_heading",
                      "set_cell_text",
                      "insert_table",
                      "insert_table_row",
                      "delete_table_row",
                      "insert_table_col",
                      "delete_table_col",
                      "merge_cells",
                      "unmerge_cells",
                      "split_cell",
                      "equalize_table",
                      "set_cell_background",
                      "set_cell_border",
                      "apply_text_style",
                      "apply_paragraph_style",
                      "set_list_style",
                      "insert_image",
                      "place_seal",
                      "set_header_footer",
                      "insert_page_number",
                      "insert_footnote",
                      "set_columns",
                      // 6종 신규 레이아웃 치수 제어 연산
                      "set_col_width",
                      "set_row_height",
                      "set_cell_padding",
                      "fit_table_to_page",
                      "set_page_margin",
                      "shrink_cell_font",
                    ],
                  },
                  find: { type: "string", description: "replace_body_text용 찾을 원본 문자열" },
                  replace: { type: "string", description: "replace_body_text용 바꿀 새 문자열" },
                  paraIndex: { type: "number", description: "문단 인덱스 (0-based)" },
                  oldTextForHwp: { type: "string", description: "set_paragraph_text용 .hwp 원본 문단 문자열" },
                  text: { type: "string", description: "문단, 제목, 표 셀, 머리말/꼬리말, 각주 텍스트" },
                  level: { type: "number", enum: [1, 2, 3, 4], description: "append_heading 개요 단계 (1~4)" },
                  tableIndex: { type: "number", description: "표 인덱스 (0-based)" },
                  row: { type: "number", description: "행 인덱스 (0-based)" },
                  col: { type: "number", description: "열 인덱스 (0-based)" },
                  fit: { type: "boolean", description: "서명란/공백 패딩 길이 유지 여부" },
                  headers: { type: "array", items: { type: "string" }, description: "insert_table 표 머리글 배열" },
                  rows: {
                    type: "array",
                    items: { type: "array", items: { type: "string" } },
                    description: "insert_table 2차원 행 데이터 배열",
                  },
                  position: { type: "string", enum: ["above", "below", "left", "right"] },
                  cells: { type: "array", items: { type: "string" } },
                  fromRow: { type: "number" },
                  fromCol: { type: "number" },
                  toRow: { type: "number" },
                  toCol: { type: "number" },
                  intoRows: { type: "number" },
                  intoCols: { type: "number" },
                  mode: { type: "string" },
                  color: { type: "string", description: "HEX 색상 코드 (예: #EAEAEA)" },
                  sides: {
                    oneOf: [
                      { type: "string", enum: ["all"] },
                      { type: "array", items: { type: "string", enum: ["top", "bottom", "left", "right"] } },
                    ],
                  },
                  widthMm: { type: "number", description: "너비 (mm)" },
                  heightMm: { type: "number", description: "높이 (mm)" },
                  keepTableWidth: { type: "boolean", description: "set_col_width 시 표 전체 너비를 유지하기 위해 나머지 열을 비례 조정할지 여부" },
                  leftMm: { type: "number", description: "좌측 여백 (mm)" },
                  rightMm: { type: "number", description: "우측 여백 (mm)" },
                  topMm: { type: "number", description: "상단 여백 (mm)" },
                  bottomMm: { type: "number", description: "하단 여백 (mm)" },
                  marginMm: { type: "number", description: "상하좌우 통합 여백 (mm)" },
                  targetWidthMm: { type: "number", description: "fit_table_to_page 목표 표 너비 (mm)" },
                  minPt: { type: "number", description: "shrink_cell_font 최소 글자 크기 하한 (pt, 기본 8)" },
                  lineType: { type: "string", enum: ["solid", "dashed", "dotted", "double"] },
                  targetText: { type: "string", description: "서식을 적용할 대상 텍스트" },
                  fontFamily: { type: "string", description: "한컴 A-set 폰트명 (함초롬바탕, 맑은 고딕, Pretendard 등)" },
                  sizePt: { type: "number", description: "글자 크기 (pt 단위, 예: 11)" },
                  bold: { type: "boolean" },
                  italic: { type: "boolean" },
                  underline: { type: "boolean" },
                  highlight: { type: "string" },
                  letterSpacing: { type: "number" },
                  charRatio: { type: "number" },
                  align: { type: "string", enum: ["LEFT", "CENTER", "RIGHT", "JUSTIFY", "DISTRIBUTE"] },
                  indentHu: { type: "number", description: "들여쓰기 HWPUNIT (1pt = 100)" },
                  lineSpacingPercent: { type: "number", description: "줄간격 % (기본 160)" },
                  path: { type: "string" },
                  anchorText: { type: "string" },
                  sealImagePath: { type: "string" },
                  where: { type: "string", enum: ["header", "footer"] },
                  count: { type: "number", enum: [1, 2, 3] },
                },
              },
            },
          },
        },
      },
    },
  },
};

export const INLINE_QUICK_ACTION_PROMPTS: Record<string, string> = {
  official_tone: `선택된 영역의 문장을 대한민국 행정기관 공문서 표준 개조식 어투(~함, ~기 바람, ~조치 요망, ~추진코자 함 등)로 변환하라.
- 하위 항목이 나열된 경우 행정업무운영 편람 순서(1. -> 가. -> 1) -> 가) -> (1) -> (가))를 준수하라.
- 절대로 텍스트 내부에 **굵게** 같은 마크다운 기호를 넣지 마라.`,

  proofread: `선택된 영역의 맞춤법, 띄어쓰기, 어색한 비문을 문맥에 맞게 교정하라. 원래의 핵심 의미와 문서 서식 구조는 그대로 유지하라.`,

  summarize_bullet: `선택된 장문의 단락을 핵심 내용 위주의 개조식 글머리/번호 목록으로 간결하게 축약하라.`,

  fill_or_calc_table: `사용자가 선택한 표의 셀 영역을 채우거나 계산하라.
- 선택된 셀들의 행/열 머리글(Header)과 주변 셀 데이터, 그리고 첨부된 외부 참고자료(Excel/PDF/HWP)가 있다면 이를 대조하여 정확한 값을 각 셀(tableIndex, row, col)의 set_cell_text 연산으로 생성하라.
- 합계/평균/비율 칸인 경우 정확한 수치 계산 결과를 기입하고 기존 숫자 표기 형식(천 단위 콤마, 단위 등)을 유지하라.
- 각 셀의 변경은 사용자가 개별 수락할 수 있도록 셀마다 별도의 diffItem으로 분리하라.`,
};

const SYSTEM_PROMPT = `당신은 대한민국 공문서 및 한컴오피스(HWP/HWPX) 전문 편집 AI 협업 어시스턴트입니다.
사용자의 요청과 첨부 문서, 선택 영역, 레이아웃 실측 보고서를 종합 분석하여 propose_hwp_edits 도구를 호출하십시오.
사용자가 요약·질문·확인만 요청한 경우에는 [문서 전체 마크다운 내용]과 [현재 문서 구조 요약]에 실제로 들어 있는 내용을 근거로 summary에 직접 답하고 diffItems는 빈 배열로 반환하십시오.

[절대 불변 원칙 - 어기면 한컴오피스 호환성 파괴]
1. 절대로 셀 텍스트나 문단 텍스트 내부에 마크다운 문법(**굵게**, *기울임*, # 제목)을 문자열로 넣지 마십시오. 한컴오피스는 마크다운 렌더러가 아니므로 별표(**)가 문서에 그대로 인쇄됩니다. 글자 스타일 변경은 반드시 apply_text_style 연산을 사용하십시오.
2. 표의 빈칸 placeholder(예: '전화번호 (     )', '성명 : ______ (인)')를 채울 때는 기존 라벨과 괄호/서명 마커를 보존한 완성된 전체 문자열을 전달하거나 fit: true를 설정하십시오.
3. 기존 문서를 수정할 때는 절대로 setup_document 연산을 호출하지 마십시오(기존 양식이 빈 문서로 덮어써집니다).
4. 공문서 항목 기호 체계: 1. -> 가. -> 1) -> 가) -> (1) -> (가) -> ① -> ㉮
5. 기본 폰트는 반드시 한컴 A-set 폰트('함초롬바탕', '맑은 고딕', 'Pretendard')를 사용하십시오.
6. 사용자가 특정 셀들이나 문단을 개별적으로 [✔ 적용] 또는 [✖ 되돌리기]할 수 있도록, diffItems 배열에 각 수정 단위를 분리하여 전달하십시오.

[레이아웃 및 넘침(Overflow) 해결 엄격 지침]
- 사용자가 글자가 잘리거나 겹친다고 하거나('잘 안보여', '잘려', '겹쳐') 여백/표 조정을 요청할 때는, 절대로 글자 크기를 키우지 마십시오(글자 확대로 오버플로를 악화시키는 행위는 정책 가드 G1에 의해 자동 거부됩니다).
- 대신 레이아웃 실측 보고서를 확인하고 다음 6종 레이아웃 연산을 적극 활용하십시오:
  * fit_table_to_page: 표가 본문 인쇄 폭(약 150mm)을 초과한 경우 표 전체를 본문 폭에 맞게 자동 비례 축소 { "type": "fit_table_to_page", "tableIndex": 0, "targetWidthMm": 150 }
  * set_row_height: 행 높이가 부족하여 내용이 잘릴 때 행 높이 확장 { "type": "set_row_height", "tableIndex": 0, "row": 2, "heightMm": 16.0 }
  * set_col_width: 열 너비가 좁아 줄바꿈이 빈번할 때 열 확장 { "type": "set_col_width", "tableIndex": 0, "col": 1, "widthMm": 45.0, "keepTableWidth": true }
  * set_cell_padding: 셀 내부 여백 축소로 가용 공간 확보 { "type": "set_cell_padding", "tableIndex": 0, "row": 1, "col": 1, "leftMm": 1.0, "rightMm": 1.0, "topMm": 1.0, "bottomMm": 1.0 }
  * shrink_cell_font: 셀 내 폰트 축소 (최소 8pt 이상 보장) { "type": "shrink_cell_font", "tableIndex": 0, "row": 1, "col": 1, "sizePt": 9.0, "minPt": 8.0 }
  * set_page_margin: 페이지 여백 조정 { "type": "set_page_margin", "leftMm": 15.0, "rightMm": 15.0 }
- 사용자가 직전 수정사항이 잘 해결되었는지 확인/검수('해결되었는지 검수해줘')를 요청할 때는, 레이아웃 실측 보고서와 대화 이력을 바탕으로 넘침 문제가 실제로 해소되었는지를 정량적으로 판정하십시오.

[주요 UnifiedOp 스키마 규칙 (0-based 인덱스 사용)]
- 본문 텍스트 치환: { "type": "replace_body_text", "find": "원본 문자열", "replace": "새 문자열" }
- 문단 추가: { "type": "append_paragraph", "text": "내용", "align": "JUSTIFY" }
- 제목 추가: { "type": "append_heading", "level": 1, "text": "제목 텍스트" }
- 표 셀 값 설정: { "type": "set_cell_text", "tableIndex": 0, "row": 1, "col": 2, "text": "셀 값", "fit": false }
- 표 생성: { "type": "insert_table", "headers": ["열1", "열2"], "rows": [["값1", "값2"]] }
- 표 행/열 추가·삭제: { "type": "insert_table_row", "tableIndex": 0, "row": 1, "position": "below", "cells": ["a", "b"] }, { "type": "delete_table_row", "tableIndex": 0, "row": 1 }
- 셀 병합/분할: { "type": "merge_cells", "tableIndex": 0, "fromRow": 0, "fromCol": 0, "toRow": 0, "toCol": 1 }, { "type": "split_cell", "tableIndex": 0, "row": 0, "col": 0, "intoRows": 2, "intoCols": 1 }
- 셀 배경/테두리: { "type": "set_cell_background", "tableIndex": 0, "row": 0, "col": 0, "color": "#EAEAEA" }, { "type": "set_cell_border", "tableIndex": 0, "row": 0, "col": 0, "sides": "all", "color": "#000000", "widthMm": 0.4 }
- 글자 서식: { "type": "apply_text_style", "targetText": "대상 어구", "fontFamily": "함초롬바탕", "sizePt": 11, "bold": true, "color": "#000000" }
- 문단 서식: { "type": "apply_paragraph_style", "paraIndex": 0, "targetText": "문단 앞부분 텍스트", "align": "JUSTIFY", "indentHu": 1000, "lineSpacingPercent": 160 }`;

export interface AiRequestOptions {
  provider: "claude" | "openai" | "gemini" | "ollama" | "baze";
  apiKey: string;
  model: string;
  customEndpoint?: string;
  docMarkdown: string;
  tablesMeta: TableCoordinateMeta[];
  selection: SelectionTarget | null;
  excludedRegions: ExcludedRegion[];
  autoMaskPii: boolean;
  userPrompt: string;
  attachmentsContext?: string;
  // Multi-turn Agentic Loop Track 2 Options
  stagingPath?: string;
  fileBytes?: Uint8Array;
  isHwpx?: boolean;
  chatHistory?: Array<{ role: "user" | "assistant" | "tool" | "system"; content: string }>;
  reasoningEffort?: ReasoningEffort;
  requestId?: string;
  timeoutMs?: number;
  maxIterations?: number;
  onStep?: (event: AgentStepEvent) => void;
  sessionMasker?: PrivacySessionMasker;
  masker?: PrivacySessionMasker;
}

export interface OrchestrateAiEditResult {
  summary: string;
  diffItems: DiffItem[];
  steps?: string[];
  docChecks?: number;
  trialsRun?: number;
  verificationPassed?: boolean;
}

const AGENT_TOOLS = [
  {
    type: "function",
    function: {
      name: "inspect_layout",
      description: "현재 문서(또는 시험 적용 중인 문서)의 표 및 셀 레이아웃 측정값, 오버플로/넘침 수치를 정밀 조회합니다.",
      parameters: {
        type: "object",
        properties: {
          tableIndex: { type: "number", description: "조회할 표 인덱스 (0-based, 생략 시 전체 표)" },
          row: { type: "number", description: "조회할 셀 행 (0-based)" },
          col: { type: "number", description: "조회할 셀 열 (0-based)" },
        },
      },
    },
  },
  {
    type: "function",
    function: {
      name: "apply_trial_edits",
      description: "임시 복사본 파일에 시험 편집 연산을 시뮬레이션 적용하고 레이아웃 변화 및 넘침 해결 여부를 실측 비교합니다.",
      parameters: {
        type: "object",
        required: ["reason", "ops"],
        properties: {
          reason: { type: "string", description: "시험 적용 목적 및 가설" },
          ops: {
            type: "array",
            items: { type: "object", required: ["type"] },
            description: "시험 적용할 UnifiedOp 배열",
          },
        },
      },
    },
  },
  {
    type: "function",
    function: {
      name: PROPOSE_HWP_EDITS_TOOL.name,
      description: PROPOSE_HWP_EDITS_TOOL.description,
      parameters: PROPOSE_HWP_EDITS_TOOL.input_schema,
    },
  },
];

export async function orchestrateAiEdit(options: AiRequestOptions): Promise<OrchestrateAiEditResult> {
  const masker = options.sessionMasker || options.masker || new PrivacySessionMasker();
  const recordedSteps: string[] = [];
  let trialsRun = 0;
  let verificationPassed = false;

  const emitStep = (event: AgentStepEvent) => {
    const formatted = event.detail ? `${event.message} (${event.detail})` : event.message;
    recordedSteps.push(formatted);
    options.onStep?.(event);
  };

  // 1. Initial Layout Inspection (WASM rhwp)
  let baseBytes = options.fileBytes;
  if (!baseBytes && options.stagingPath) {
    try {
      const rawBytes = await runtimeInvoke<number[]>("read_file_bytes", {
        filePath: options.stagingPath,
      });
      if (rawBytes && rawBytes.length > 0) {
        baseBytes = new Uint8Array(rawBytes);
      }
    } catch (e) {
      console.warn("[aiOrchestrator] Failed to read base file bytes for inspector:", e);
    }
  }

  let initialReport: LayoutReport | null = null;
  let currentReport: LayoutReport | null = null;

  if (baseBytes && baseBytes.length > 0) {
    emitStep({ type: "inspect", message: "문서 레이아웃 실측 정밀 진단 중..." });
    try {
      initialReport = await inspectDocumentBytes(baseBytes);
      currentReport = initialReport;
      const issCount = initialReport.issuesCount;
      emitStep({
        type: "inspect",
        message: issCount > 0 ? `레이아웃 진단 완료: 이슈 ${issCount}건 발견` : "레이아웃 진단 완료: 정상",
        detail: issCount > 0 ? initialReport.issues.map((i) => i.message).slice(0, 3).join("; ") : undefined,
      });
    } catch (e: any) {
      console.warn("[aiOrchestrator] Initial layout inspection failed:", e);
      emitStep({ type: "inspect", message: "레이아웃 실측 진단 건너뜀 (파서 경고)", detail: e?.message });
    }
  }

  // 2. Build Context
  const contextParts: string[] = [];
  const tablesSummary = options.tablesMeta
    .map(
      (t) =>
        `- 표 ${t.tableIndex} (${t.rowCount}행 x ${t.colCount}열): ` +
        t.cells
          .slice(0, 30)
          .map((c) => `[${c.row},${c.col}]="${c.text.replace(/\n/g, " ")}"`)
          .join(", ")
    )
    .join("\n");
  contextParts.push(
    `### [현재 문서 구조 요약]\n표 개수: ${options.tablesMeta.length}${tablesSummary ? "\n" + tablesSummary : ""}`
  );

  if (initialReport) {
    contextParts.push(`### [문서 레이아웃 실측 진단 보고서 (WASM Inspector)]\n${formatLayoutReportForPrompt(initialReport)}`);
  }

  if (options.selection) {
    if (options.selection.kind === "table_cells") {
      contextParts.push(
        `### [사용자가 마우스로 블록 지정한 표 셀 영역]\n` +
        `표 인덱스: ${options.selection.tableIndex} (${options.selection.startRow + 1}~${options.selection.endRow + 1}행, ${options.selection.startCol + 1}~${options.selection.endCol + 1}열)\n` +
        `선택된 셀 데이터:\n` +
        options.selection.selectedCells.map((c) => `- [${c.row}행 ${c.col}열]: "${c.text}"`).join("\n")
      );
    } else if (options.selection.kind === "body_range") {
      contextParts.push(
        `### [사용자가 마우스로 드래그 선택한 본문 텍스트]\n` +
        `문단 범위: ${options.selection.startPara + 1} ~ ${options.selection.endPara + 1}\n` +
        `내용:\n${options.selection.selectedText}`
      );
    }
  }

  if (options.docMarkdown) {
    contextParts.push(`### [문서 전체 마크다운 내용]\n${options.docMarkdown}`);
  }

  if (options.attachmentsContext) {
    contextParts.push(`### [사용자가 첨부한 로컬 참고자료 (Excel / PDF / HWP 등)]\n${options.attachmentsContext}`);
  }

  const rawUserContent = `${contextParts.join("\n\n")}\n\n### [사용자 지시사항]\n${options.userPrompt}`;

  // 3. Apply Privacy Masking
  const maskedPrompt = masker.maskTextForPrompt(rawUserContent, options.excludedRegions, options.autoMaskPii);

  // 4. Assemble Multi-turn Messages
  const messages: Array<{
    role: "user" | "assistant" | "tool" | "system";
    content: string | null;
    tool_calls?: any[];
    tool_call_id?: string;
  }> = [];

  if (options.chatHistory && options.chatHistory.length > 0) {
    // Keep last 6 chat history turns
    for (const h of options.chatHistory.slice(-6)) {
      messages.push({
        role: h.role as any,
        content: masker.maskTextForPrompt(h.content, options.excludedRegions, options.autoMaskPii),
      });
    }
  }

  messages.push({ role: "user", content: maskedPrompt });

  let rawResult: { summary: string; diffItems: any[] } = { summary: "", diffItems: [] };
  const isAgenticProvider = options.provider === "baze" || options.provider === "openai";
  const maxIterations = Math.min(6, Math.max(1, options.maxIterations ?? 5));

  if (isAgenticProvider) {
    // --- Claude-CLI Style Multi-Turn Agentic Loop ---
    let endpointUrl = options.customEndpoint;
    if (options.provider === "baze" && !endpointUrl) {
      endpointUrl = "https://api.baze.co/v1/gateway/chat/completions";
    } else if (options.provider === "openai" && !endpointUrl) {
      endpointUrl = "https://api.openai.com/v1/chat/completions";
    }

    let loopFinished = false;

    for (let attempt = 0; attempt < maxIterations; attempt++) {
      if (loopFinished) break;

      // Maintain high reasoning effort across turns (turn 1 may be inspect_layout, turn 2 deep editing)
      const currentEffort: ReasoningEffortOption =
        (options.reasoningEffort as ReasoningEffortOption) || "high";

      emitStep({
        type: "thinking",
        message: `AI 추론 및 편집안 생성 중... (${attempt + 1}/${maxIterations}회차, effort: ${currentEffort})`,
      });

      const turnResult = await callOpenAiAgentTurn({
        endpointUrl: endpointUrl!,
        apiKey: options.apiKey,
        model: options.model || (options.provider === "baze" ? "gpt-6.1-terra" : "gpt-4o"),
        systemPrompt: SYSTEM_PROMPT,
        messages,
        tools: AGENT_TOOLS,
        toolChoice: "auto",
        reasoningEffort: currentEffort,
        requestId: options.requestId,
        timeoutMs: options.timeoutMs ?? 180_000,
      });

      // Append assistant's turn to messages
      messages.push({
        role: "assistant",
        content: turnResult.content,
        tool_calls: turnResult.toolCalls.length > 0 ? turnResult.toolCalls : undefined,
      });

      if (turnResult.toolCalls.length === 0) {
        // Fallback: check if content contains json block
        const textContent = turnResult.content || "";
        const jsonMatch = textContent.match(/```json\s*([\s\S]*?)\s*```/) || textContent.match(/\{[\s\S]*"diffItems"[\s\S]*\}/);
        if (jsonMatch) {
          try {
            const parsed = JSON.parse(jsonMatch[1] || jsonMatch[0]);
            if (Array.isArray(parsed.diffItems)) {
              rawResult = parsed;
              loopFinished = true;
              break;
            }
          } catch {}
        }
        // Direct answer to question/summary
        rawResult = { summary: textContent, diffItems: [] };
        loopFinished = true;
        break;
      }

      // Process tool calls in this turn
      for (const call of turnResult.toolCalls) {
        const fnName = call.function?.name;
        let callArgs: any = {};
        try {
          callArgs = JSON.parse(call.function?.arguments || "{}");
        } catch {
          callArgs = {};
        }

        if (fnName === "inspect_layout") {
          emitStep({ type: "inspect", message: "도구 호출: 레이아웃 세부 수치 조회" });
          let toolReply = "";
          if (currentReport) {
            if (typeof callArgs.tableIndex === "number") {
              const tbl = currentReport.tables.find((t) => t.tableIndex === callArgs.tableIndex);
              if (tbl) {
                if (typeof callArgs.row === "number" && typeof callArgs.col === "number") {
                  const cell = tbl.cells.find((c) => c.row === callArgs.row && c.col === callArgs.col);
                  toolReply = JSON.stringify({ cell, issues: currentReport.issues.filter((i) => i.cell?.tableIndex === callArgs.tableIndex && i.cell?.row === callArgs.row && i.cell?.col === callArgs.col) });
                } else {
                  toolReply = JSON.stringify({ tableWidthMm: tbl.tableWidthMm, printableWidthMm: tbl.printableWidthMm, cellCount: tbl.cellCount, issues: currentReport.issues.filter((i) => i.tableIndex === callArgs.tableIndex) });
                }
              } else {
                toolReply = JSON.stringify({ error: `Table ${callArgs.tableIndex} not found` });
              }
            } else {
              toolReply = formatLayoutReportForPrompt(currentReport);
            }
          } else {
            toolReply = "레이아웃 리포트를 사용할 수 없습니다.";
          }

          messages.push({
            role: "tool",
            tool_call_id: call.id,
            content: toolReply,
          });
        } else if (fnName === "apply_trial_edits") {
          trialsRun += 1;
          emitStep({ type: "trial", message: `도구 호출: 시험 적용 (${callArgs.reason || "시뮬레이션"})` });
          const trialOps: UnifiedOp[] = Array.isArray(callArgs.ops) ? callArgs.ops : [];

          // 1. Lint via Policy Guard
          const guardContext = {
            userPrompt: options.userPrompt,
            isNewDocument: false,
            excludedRegions: options.excludedRegions,
            layoutReport: currentReport,
            tablesMeta: options.tablesMeta,
          };
          const guardRes = lintOps(trialOps, guardContext);

          if (!guardRes.passed) {
            emitStep({ type: "guard", message: "시험 적용 정책 가드 위반", detail: guardRes.rejectedOps.map((r) => r.reason).join("; ") });
            messages.push({
              role: "tool",
              tool_call_id: call.id,
              content: JSON.stringify({
                ok: false,
                guardRejected: guardRes.rejectedOps,
                feedback: `정책 가드 위반으로 시험 적용이 취소되었습니다:\n${guardRes.rejectedOps.map((r) => r.reason).join("\n")}`,
              }),
            });
            continue;
          }

          // 2. Execute on trial copy
          if (baseBytes && options.stagingPath) {
            try {
              const isHwpx = Boolean(options.isHwpx ?? (options.stagingPath.endsWith(".hwpx") || (baseBytes[0] === 0x50 && baseBytes[1] === 0x4b)));
              const ext = isHwpx ? "hwpx" : "hwp";
              const trialPath = options.stagingPath.replace(/[^\\/]+$/, `trial_${Date.now()}.${ext}`);

              await runtimeInvoke("write_file_bytes", { filePath: trialPath, bytes: Array.from(baseBytes) });
              const unmaskedTrialOps = masker.unmaskAndFilterOps(guardRes.sanitizedOps, options.excludedRegions);
              const payload = OpTranslator.toEnginePayload(trialPath, isHwpx, unmaskedTrialOps, options.tablesMeta, false);
              const clawRes = await runtimeInvoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
                isHwpx,
                payloadJson: payload.stdinJson,
              });

              if (clawRes.ok) {
                const patchedBytesArr = await runtimeInvoke<number[]>("read_file_bytes", { filePath: trialPath });
                const trialReport = await inspectDocumentBytes(new Uint8Array(patchedBytesArr));
                const comp = compareLayoutReports(initialReport || trialReport, trialReport);
                currentReport = trialReport;

                emitStep({ type: "verify", message: `시험 적용 실측: ${comp.summaryText}` });
                messages.push({
                  role: "tool",
                  tool_call_id: call.id,
                  content: JSON.stringify({
                    ok: true,
                    summary: comp.summaryText,
                    resolvedCount: comp.resolvedIssues.length,
                    remainingCount: comp.remainingIssues.length,
                    newCount: comp.newIssues.length,
                    details: formatLayoutReportForPrompt(trialReport),
                  }),
                });
              } else {
                messages.push({
                  role: "tool",
                  tool_call_id: call.id,
                  content: JSON.stringify({ ok: false, error: clawRes.stderr || "Claw execution error" }),
                });
              }
            } catch (err: any) {
              messages.push({
                role: "tool",
                tool_call_id: call.id,
                content: JSON.stringify({ ok: false, error: err.message }),
              });
            }
          } else {
            messages.push({
              role: "tool",
              tool_call_id: call.id,
              content: JSON.stringify({ ok: true, note: "파일 바이너리 부재로 정적 가드만 통과했습니다." }),
            });
          }
        } else if (fnName === "propose_hwp_edits") {
          emitStep({ type: "propose", message: "제안안 도출 검수 및 정책 가드 검사 중..." });
          const proposalSummary = callArgs.summary || "";
          const diffItems = Array.isArray(callArgs.diffItems) ? callArgs.diffItems : [];
          const allOps = diffItems.flatMap((d: any) => d.unifiedOps || []);

          // 1. Policy Guard (G1-G6) Check
          const guardContext = {
            userPrompt: options.userPrompt,
            isNewDocument: false,
            excludedRegions: options.excludedRegions,
            layoutReport: currentReport || initialReport,
            tablesMeta: options.tablesMeta,
          };
          const guardRes = lintOps(allOps, guardContext);

          if (!guardRes.passed && attempt < maxIterations - 1) {
            emitStep({
              type: "guard",
              message: "정책 가드 위반 감지, 모델 자가 교정 피드백 전달",
              detail: guardRes.rejectedOps.map((r) => r.reason).join("; "),
            });
            messages.push({
              role: "tool",
              tool_call_id: call.id,
              content: `정책 가드 검사 실패:\n${guardRes.rejectedOps.map((r) => r.reason).join("\n")}\n\n위반 사항을 교정(예: 폰트 확대 대신 set_row_height, set_col_width, fit_table_to_page 또는 shrink_cell_font 사용)하여 다시 propose_hwp_edits를 호출하세요.`,
            });
            continue; // Continue agent loop to let model self-correct
          }

          // 2. Auto-Verification Gate (Trial Apply + Layout Re-measurement)
          let afterReport: LayoutReport | null = null;
          let verificationOk = true;
          let compSummary = "";

          if (baseBytes && options.stagingPath && allOps.length > 0) {
            trialsRun += 1;
            emitStep({ type: "trial", message: "제안된 연산 복사본 시험 적용 및 재측정 검증 중..." });
            try {
              const isHwpx = Boolean(options.isHwpx ?? (options.stagingPath.endsWith(".hwpx") || (baseBytes[0] === 0x50 && baseBytes[1] === 0x4b)));
              const ext = isHwpx ? "hwpx" : "hwp";
              const verifyPath = options.stagingPath.replace(/[^\\/]+$/, `trial_verify_${Date.now()}.${ext}`);

              await runtimeInvoke("write_file_bytes", { filePath: verifyPath, bytes: Array.from(baseBytes) });
              const unmaskedOps = masker.unmaskAndFilterOps(guardRes.sanitizedOps, options.excludedRegions);
              const payload = OpTranslator.toEnginePayload(verifyPath, isHwpx, unmaskedOps, options.tablesMeta, false);
              const clawRes = await runtimeInvoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
                isHwpx,
                payloadJson: payload.stdinJson,
              });

              if (clawRes.ok) {
                const patchedBytesArr = await runtimeInvoke<number[]>("read_file_bytes", { filePath: verifyPath });
                afterReport = await inspectDocumentBytes(new Uint8Array(patchedBytesArr));
                const comparison = compareLayoutReports(initialReport || afterReport, afterReport);
                compSummary = comparison.summaryText;

                // If target overflow issues still remain or new issues introduced AND attempt < maxIterations - 1, feed back!
                if (!comparison.ok && comparison.remainingIssues.length > 0 && attempt < maxIterations - 1) {
                  emitStep({
                    type: "verify",
                    message: "오버플로 미완전 해소 감지, 모델 정밀 교정 피드백 전달",
                    detail: comparison.summaryText,
                  });
                  messages.push({
                    role: "tool",
                    tool_call_id: call.id,
                    content: `시험 검증 결과: ${comparison.summaryText}\n잔여 이슈 목록:\n${comparison.remainingIssues.map((i) => i.message).join("\n")}\n\n행 높이나 열 너비를 더 확장하거나 셀 여백을 줄여 오버플로를 완전히 해소한 뒤 다시 propose_hwp_edits를 호출하세요.`,
                  });
                  continue; // Continue loop to refine
                }

                verificationOk = comparison.ok;
                verificationPassed = comparison.ok;
              }
            } catch (err: any) {
              console.warn("[aiOrchestrator] Auto-verification error:", err);
            }
          }

          // Attach verification and metric annotations to diffItems
          for (const item of diffItems) {
            item.verificationStatus = verificationOk ? "verified" : "warning_unresolved";
            item.warnings = [...(guardRes.warnings || [])];

            if (initialReport && afterReport && item.targetCoord && typeof item.targetCoord.tableIndex === "number") {
              const tc = item.targetCoord;
              const cellBefore = initialReport.tables[tc.tableIndex]?.cells.find((c) => c.row === tc.row && c.col === tc.col);
              const cellAfter = afterReport.tables[tc.tableIndex]?.cells.find((c) => c.row === tc.row && c.col === tc.col);
              if (cellBefore || cellAfter) {
                item.metricsDelta = {
                  beforeOverflowMm: cellBefore?.overflowAmountMm,
                  afterOverflowMm: cellAfter?.overflowAmountMm,
                  widthBeforeMm: cellBefore?.widthMm,
                  widthAfterMm: cellAfter?.widthMm,
                  heightBeforeMm: cellBefore?.heightMm,
                  heightAfterMm: cellAfter?.heightMm,
                };
              }
            }
          }

          rawResult = {
            summary: proposalSummary,
            diffItems,
          };
          emitStep({
            type: "verify",
            message: verificationOk ? "검증 완료: 모든 레이아웃 이슈 해소" : `검증 완료: ${compSummary || "일부 잔여 이슈 보존"}`,
          });
          loopFinished = true;
          break;
        }
      }
    }
  } else {
    // Other providers (Claude / Gemini / Ollama)
    emitStep({ type: "thinking", message: `AI 모델(${options.provider}/${options.model}) 호출 중...` });
    switch (options.provider) {
      case "claude":
        rawResult = await callClaudeApi(options.apiKey, options.model, SYSTEM_PROMPT, messages as any);
        break;
      case "gemini":
        rawResult = await callGeminiApi(options.apiKey, options.model, SYSTEM_PROMPT, messages as any);
        break;
      case "ollama":
        rawResult = await callOllamaApi(options.customEndpoint || "", options.model, SYSTEM_PROMPT, messages as any);
        break;
      default:
        rawResult = await callOpenAiCompatibleApi(
          options.customEndpoint || "https://api.openai.com/v1/chat/completions",
          options.apiKey,
          options.model,
          SYSTEM_PROMPT,
          messages as any
        );
        break;
    }

    // Run Policy Guard & Trial Verification on result
    const allOps = (rawResult.diffItems || []).flatMap((d: any) => d.unifiedOps || []);
    const guardContext = {
      userPrompt: options.userPrompt,
      isNewDocument: false,
      excludedRegions: options.excludedRegions,
      layoutReport: initialReport,
      tablesMeta: options.tablesMeta,
    };
    const guardRes = lintOps(allOps, guardContext);

    if (baseBytes && options.stagingPath && allOps.length > 0) {
      trialsRun += 1;
      emitStep({ type: "trial", message: "제안된 연산 복사본 시험 적용 및 재측정 검증 중..." });
      try {
        const isHwpx = Boolean(options.isHwpx ?? (options.stagingPath.endsWith(".hwpx") || (baseBytes[0] === 0x50 && baseBytes[1] === 0x4b)));
        const ext = isHwpx ? "hwpx" : "hwp";
        const verifyPath = options.stagingPath.replace(/[^\\/]+$/, `trial_verify_${Date.now()}.${ext}`);

        await runtimeInvoke("write_file_bytes", { filePath: verifyPath, bytes: Array.from(baseBytes) });
        const unmaskedOps = masker.unmaskAndFilterOps(guardRes.sanitizedOps, options.excludedRegions);
        const payload = OpTranslator.toEnginePayload(verifyPath, isHwpx, unmaskedOps, options.tablesMeta, false);
        const clawRes = await runtimeInvoke<{ ok: boolean; stdout_json: any; stderr: string }>("execute_claw_ops", {
          isHwpx,
          payloadJson: payload.stdinJson,
        });

        if (clawRes.ok) {
          const patchedBytesArr = await runtimeInvoke<number[]>("read_file_bytes", { filePath: verifyPath });
          const afterReport = await inspectDocumentBytes(new Uint8Array(patchedBytesArr));
          const comparison = compareLayoutReports(initialReport || afterReport, afterReport);
          verificationPassed = comparison.ok;

          for (const item of rawResult.diffItems) {
            item.verificationStatus = comparison.ok ? "verified" : (comparison.newIssues.length > 0 ? "warning_new_issues" : "warning_unresolved");
            item.warnings = [...(guardRes.warnings || [])];
          }
        }
      } catch (err: any) {
        console.warn("[aiOrchestrator] Verification error:", err);
      }
    }
  }

  // 5. Unmask and filter operations for UI DiffItems
  const processedDiffItems: DiffItem[] = (rawResult.diffItems || []).map((item: any) => {
    const unmaskedOps = masker.unmaskAndFilterOps(item.unifiedOps as UnifiedOp[], options.excludedRegions);

    // Calculate bbox and infer targetCoord for diff item
    let targetCoord = item.targetCoord;
    if (!targetCoord) {
      const cellOp = unmaskedOps.find(
        (op): op is Extract<UnifiedOp, { tableIndex: number; row: number; col: number }> =>
          "tableIndex" in op && "row" in op && "col" in op
      );
      if (cellOp) {
        targetCoord = {
          tableIndex: cellOp.tableIndex,
          row: cellOp.row,
          col: cellOp.col,
        };
      }
    }

    let bbox: DiffItem["bbox"] = undefined;
    if (targetCoord && typeof targetCoord.tableIndex === "number") {
      const table = options.tablesMeta[targetCoord.tableIndex];
      if (table) {
        const cell = table.cells.find(
          (c) => c.row === targetCoord.row && c.col === targetCoord.col
        );
        if (cell?.bbox) {
          bbox = {
            pageIndex: cell.bbox.pageIndex,
            x: cell.bbox.x,
            y: cell.bbox.y,
            w: cell.bbox.w,
            h: cell.bbox.h,
          };
        }
      }
    } else if (options.selection?.kind === "table_cells" && item.scope === "cell") {
      for (const op of unmaskedOps) {
        if ("tableIndex" in op && "row" in op && "col" in op) {
          const match = options.selection.selectedCells.find((c) => c.row === op.row && c.col === op.col);
          if (match?.bbox) {
            bbox = {
              pageIndex: match.bbox.pageIndex,
              x: match.bbox.x,
              y: match.bbox.y,
              w: match.bbox.w,
              h: match.bbox.h,
            };
            break;
          }
        }
      }
    } else if (options.selection?.kind === "body_range" && options.selection.rects.length > 0) {
      const r = options.selection.rects[0];
      bbox = {
        pageIndex: r.pageIndex,
        x: r.x,
        y: r.y,
        w: r.width,
        h: r.height,
      };
    }

    return {
      id: crypto.randomUUID(),
      status: "pending",
      scope: item.scope || "paragraph",
      title: masker.unmaskText(item.title || "수정 제안"),
      beforeText: masker.unmaskText(item.beforeText || ""),
      afterText: masker.unmaskText(item.afterText || ""),
      unifiedOps: unmaskedOps,
      targetCoord,
      bbox,
      verificationStatus: item.verificationStatus || (verificationPassed ? "verified" : "unverified"),
      warnings: item.warnings,
      metricsDelta: item.metricsDelta,
    };
  });

  const finalSummary = masker.unmaskText(rawResult.summary || (processedDiffItems.length > 0 ? "수정 사항이 제안되었습니다." : "문서 확인이 완료되었습니다."));

  return {
    summary: finalSummary,
    diffItems: processedDiffItems,
    steps: recordedSteps,
    docChecks: recordedSteps.length,
    trialsRun,
    verificationPassed,
  };
}
