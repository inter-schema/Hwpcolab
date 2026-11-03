import type { UnifiedOp } from "../types/operations.ts";
import type { ExcludedRegion } from "../types/privacy.ts";

export { type ExcludedRegion };

export class PrivacySessionMasker {
  private tokenToRaw = new Map<string, string>();
  private rawToToken = new Map<string, string>();
  private counter = 0;

  private registerToken(prefix: string, rawValue: string): string {
    const existing = this.rawToToken.get(rawValue);
    if (existing) return existing;
    this.counter += 1;
    const token = `[[${prefix}_${this.counter}]]`;
    this.tokenToRaw.set(token, rawValue);
    this.rawToToken.set(rawValue, token);
    return token;
  }

  /**
   * LLM에 전송할 문서 마크다운/선택 텍스트에서:
   * 1) 사용자가 지정한 ExcludedRegion을 [[AI_EXCLUDED_N]]으로 치환하고,
   * 2) autoMaskPii가 켜져 있으면 주민번호/전화번호/이메일/계좌번호를 가명 토큰으로 치환합니다.
   */
  public maskTextForPrompt(
    inputText: string,
    excludedRegions: ExcludedRegion[] = [],
    autoMaskPii = false
  ): string {
    let result = inputText;
    const safeRegions = Array.isArray(excludedRegions) ? excludedRegions : [];

    // 1. 사용자 지정 전송 제외 영역 마스킹
    for (const region of safeRegions) {
      if (region.rawText && region.rawText.trim().length > 0) {
        const token = this.registerToken("AI_EXCLUDED", region.rawText);
        result = result.split(region.rawText).join(token);
      }
    }

    // 2. 선택적 PII 정규식 가명화 (사용자가 토글을 켠 경우에만 동작)
    if (autoMaskPii) {
      // 주민등록번호 / 외국인등록번호
      result = result.replace(/\b\d{6}-[1-8]\d{6}\b/g, (match) =>
        this.registerToken("PII_RRN", match)
      );
      // 휴대전화 및 일반전화번호
      result = result.replace(/\b01[016789]-\d{3,4}-\d{4}\b/g, (match) =>
        this.registerToken("PII_PHONE", match)
      );
      // 이메일
      result = result.replace(/\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g, (match) =>
        this.registerToken("PII_EMAIL", match)
      );
      // Korean bank account numbers: require an explicit account label to
      // avoid masking student IDs, document numbers and unrelated digit groups.
      result = result.replace(
        /((?:계좌(?:번호)?|통장(?:번호)?|bank\s*account(?:\s*(?:number|no\.?))?)\s*[:：#]?\s*)(\d{10,16}|\d{2,8}(?:[- ]\d{2,8}){1,3})(?!\d)/gi,
        (match, label: string, account: string) => {
          const digitCount = account.replace(/\D/g, '').length;
          if (digitCount < 10 || digitCount > 16) return match;
          return label + this.registerToken("PII_ACCOUNT", account);
        }
      );
    }

    return result;
  }

  /**
   * 문자열 내부의 가명 토큰([[...]])을 원본 텍스트로 복원합니다.
   * 다중 턴 세션 동안 누적된 매핑 테이블을 사용하여 요약문, 도구 인자, 피드백 문구를 복원할 때 사용합니다.
   */
  public unmaskText(text: string): string {
    if (!text) return text;
    let out = text;
    for (const [token, raw] of this.tokenToRaw.entries()) {
      out = out.split(token).join(raw);
    }
    return out;
  }

  /**
   * AI가 반환한 UnifiedOp[] 내부의 가명 토큰을 로컬 원본 텍스트로 복원합니다.
   * 또한 사용자가 [AI 전송 제외]로 잠근 셀을 AI가 수정하려 시도하면 자동 필터링(차단)합니다.
   * 신규 6종 레이아웃 op(set_col_width, set_row_height, set_cell_padding, fit_table_to_page, set_page_margin, shrink_cell_font)도 지원합니다.
   */
  public unmaskAndFilterOps(
    ops: UnifiedOp[],
    excludedRegions: ExcludedRegion[] = []
  ): UnifiedOp[] {
    const safeRegions = Array.isArray(excludedRegions) ? excludedRegions : [];
    const excludedCellKeys = new Set(
      safeRegions
        .filter((r) => r.targetType === "cell")
        .map((r) => `${r.tableIndex}:${r.row}:${r.col}`)
    );

    return ops
      .filter((op) => {
        if ("tableIndex" in op && "row" in op && "col" in op) {
          const key = `${op.tableIndex}:${op.row}:${op.col}`;
          if (excludedCellKeys.has(key)) {
            console.warn(`[PrivacyMasker] Blocked AI edit on excluded cell ${key} (op: ${op.type})`);
            return false;
          }
        }
        return true;
      })
      .map((op) => {
        const cloned = structuredClone(op) as any;
        for (const k of ["text", "find", "replace", "targetText", "anchorText"]) {
          if (typeof cloned[k] === "string") {
            cloned[k] = this.unmaskText(cloned[k]);
          }
        }
        if (Array.isArray(cloned.headers)) {
          cloned.headers = cloned.headers.map((h: string) => this.unmaskText(h));
        }
        if (Array.isArray(cloned.rows)) {
          cloned.rows = cloned.rows.map((r: string[]) => r.map((c: string) => this.unmaskText(c)));
        }
        if (Array.isArray(cloned.cells)) {
          cloned.cells = cloned.cells.map((c: string) => this.unmaskText(c));
        }
        return cloned as UnifiedOp;
      });
  }
}
