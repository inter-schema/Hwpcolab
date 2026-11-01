import type { UnifiedOp } from "../types/operations.ts";
import type { ExcludedRegion } from "../core/privacyMasker.ts";
import type { LayoutReport } from "../core/layoutInspector.ts";
import type { TableCoordinateMeta } from "../types/hwp.ts";

export interface PolicyGuardContext {
  userPrompt: string;
  isNewDocument?: boolean;
  excludedRegions?: ExcludedRegion[];
  layoutReport?: LayoutReport | null;
  tablesMeta?: TableCoordinateMeta[];
}

export interface PolicyGuardResult {
  passed: boolean;
  rejectedOps: Array<{ op: UnifiedOp; reason: string }>;
  sanitizedOps: UnifiedOp[];
  warnings: string[];
}

/**
 * 사용자 프롬프트에서 명시적 부정 제약(예: "날짜 셀은 바꾸지 마", "단원명은 건드리지 마")을 추출합니다.
 */
export function extractNegativeConstraints(prompt: string): string[] {
  if (!prompt) return [];
  const constraints: string[] = [];
  const regexes = [
    /([가-힣a-zA-Z0-9\s]{1,15}?)(?:\s*셀|\s*칸|\s*행|\s*열)?(?:은|는|도)?\s*(?:변경하지|바꾸지|건드리지|수정하지)\s*마/g,
    /([가-힣a-zA-Z0-9\s]{1,15}?)(?:\s*셀|\s*칸|\s*행|\s*열)?(?:은|는|도)?\s*(?:그대로\s*(?:둬|두라|유지)|제외)/g,
  ];

  for (const reg of regexes) {
    let match: RegExpExecArray | null;
    while ((match = reg.exec(prompt)) !== null) {
      const keyword = match[1].trim();
      if (keyword.length > 0 && !["이", "그", "저", "해당", "모든"].includes(keyword)) {
        constraints.push(keyword);
      }
    }
  }

  return Array.from(new Set(constraints));
}

/**
 * 마크다운 별표 기호(**, *) 제거 정제
 */
function sanitizeMarkdown(text: string): string {
  if (!text) return text;
  return text.replace(/\*\*(.*?)\*\*/g, "$1").replace(/\*(.*?)\*/g, "$1");
}

/**
 * 정적 정책 가드 (G1 ~ G6)
 */
export function lintOps(
  ops: UnifiedOp[],
  context: PolicyGuardContext
): PolicyGuardResult {
  const rejectedOps: Array<{ op: UnifiedOp; reason: string }> = [];
  const sanitizedOps: UnifiedOp[] = [];
  const warnings: string[] = [];

  const userPrompt = context.userPrompt || "";
  const isNewDoc = Boolean(context.isNewDocument);
  const excludedRegions = context.excludedRegions || [];
  const negativeConstraints = extractNegativeConstraints(userPrompt);

  const promptComplainsClipping =
    /잘려|잘림|안보여|안\s*보여|겹쳐|겹침|넘침|오버플로/i.test(userPrompt);

  // 셀별 오버플로 이슈 Set 구축
  const overflowCellKeys = new Set<string>();
  if (context.layoutReport?.issues) {
    for (const iss of context.layoutReport.issues) {
      if (iss.cell) {
        overflowCellKeys.add(`${iss.cell.tableIndex}:${iss.cell.row}:${iss.cell.col}`);
      }
    }
  }

  // 이번 op 배치 내에서 행/열 치수를 확장하는 연산이 있는지 확인
  const hasDimensionExpansion = ops.some(
    (op) =>
      op.type === "set_row_height" ||
      op.type === "set_col_width" ||
      op.type === "fit_table_to_page"
  );

  // 제외된 셀 Set 구축
  const excludedCellKeys = new Set(
    excludedRegions
      .filter((r) => r.targetType === "cell")
      .map((r) => `${r.tableIndex}:${r.row}:${r.col}`)
  );

  for (const rawOp of ops) {
    let op = structuredClone(rawOp);

    // G4: 기존 문서에 setup_document 실행 금지
    if ((op as any).type === "setup_document" && !isNewDoc) {
      rejectedOps.push({
        op,
        reason: "G4 위반: 기존 문서에 setup_document를 실행하면 전체 서식이 초기화됩니다.",
      });
      continue;
    }

    // G2: 제외 영역 및 사용자 명시 부정 제약 보호
    let violatesG2 = false;
    let g2Reason = "";

    // 1) excludedRegions 셀 좌표 검사
    if ("tableIndex" in op && "row" in op && "col" in op) {
      const key = `${op.tableIndex}:${op.row}:${op.col}`;
      if (excludedCellKeys.has(key)) {
        violatesG2 = true;
        g2Reason = `G2 위반: 셀 (${op.tableIndex}번 표 [${op.row}, ${op.col}])은 사용자가 AI 수정 제외로 지정한 영역입니다.`;
      }
    }

    // 2) 사용자 명시 부정 제약 키워드 검사
    if (!violatesG2 && negativeConstraints.length > 0) {
      for (const keyword of negativeConstraints) {
        // op 내 텍스트 필드나 targetText에 금지 키워드가 포함되어 있는지 확인
        let matchesKeyword = false;
        if ("targetText" in op && typeof op.targetText === "string" && op.targetText.includes(keyword)) {
          matchesKeyword = true;
        } else if ("find" in op && typeof op.find === "string" && op.find.includes(keyword)) {
          matchesKeyword = true;
        } else if ("text" in op && typeof op.text === "string" && op.text.includes(keyword)) {
          matchesKeyword = true;
        } else if ("tableIndex" in op && "row" in op && "col" in op && context.tablesMeta) {
          const tMeta = context.tablesMeta[op.tableIndex];
          const cell = tMeta?.cells?.find((c) => c.row === op.row && c.col === op.col);
          if (cell?.text && cell.text.includes(keyword)) {
            matchesKeyword = true;
          }
        }

        if (matchesKeyword) {
          violatesG2 = true;
          g2Reason = `G2 위반: 사용자가 수정을 금지한 대상("${keyword}")이 포함된 연산입니다.`;
          break;
        }
      }
    }

    if (violatesG2) {
      rejectedOps.push({ op, reason: g2Reason });
      continue;
    }

    // G1: 오버플로 악화 방지 (Anti-Worsening Overflow)
    if (op.type === "apply_text_style") {
      const increasesFont = (op.sizePt !== undefined && op.sizePt > 11) || op.bold === true;
      if (increasesFont && (overflowCellKeys.size > 0 || promptComplainsClipping) && !hasDimensionExpansion) {
        rejectedOps.push({
          op,
          reason:
            "G1 위반: 이미 공간이 부족하거나 잘림 문제가 있는 셀의 글자 크기를 키우면 잘림이 악화됩니다. 대신 set_row_height, set_col_width, fit_table_to_page 또는 shrink_cell_font를 사용하세요.",
        });
        continue;
      }
    }

    // G6: 최소 폰트 크기 하한 (최소 8pt) 보정
    if (op.type === "apply_text_style" && op.sizePt !== undefined && op.sizePt < 8) {
      warnings.push(`G6 준수: apply_text_style의 글자 크기(${op.sizePt}pt)가 최소 하한(8pt) 미만이어서 8pt로 자동 보정되었습니다.`);
      op.sizePt = 8;
    }
    if (op.type === "shrink_cell_font") {
      if (op.sizePt < 8) {
        warnings.push(`G6 준수: shrink_cell_font의 글자 크기(${op.sizePt}pt)가 최소 하한(8pt) 미만이어서 8pt로 자동 보정되었습니다.`);
        op.sizePt = 8;
      }
      if (op.minPt !== undefined && op.minPt < 8) {
        op.minPt = 8;
      }
    }

    // G3: 마크다운 문법 정제 (텍스트 필드에서 ** 및 * 제거)
    if ("text" in op && typeof op.text === "string") {
      op.text = sanitizeMarkdown(op.text);
    }
    if ("replace" in op && typeof op.replace === "string") {
      op.replace = sanitizeMarkdown(op.replace);
    }
    if ("targetText" in op && typeof op.targetText === "string") {
      op.targetText = sanitizeMarkdown(op.targetText);
    }
    if ("anchorText" in op && typeof op.anchorText === "string") {
      op.anchorText = sanitizeMarkdown(op.anchorText);
    }
    if ("headers" in op && Array.isArray(op.headers)) {
      op.headers = op.headers.map((h: string) => sanitizeMarkdown(h));
    }
    if ("rows" in op && Array.isArray(op.rows)) {
      op.rows = op.rows.map((row: string[]) => row.map((c: string) => sanitizeMarkdown(c)));
    }
    if ("cells" in op && Array.isArray(op.cells)) {
      op.cells = op.cells.map((c: string) => sanitizeMarkdown(c));
    }

    sanitizedOps.push(op);
  }

  return {
    passed: rejectedOps.length === 0,
    rejectedOps,
    sanitizedOps,
    warnings,
  };
}
