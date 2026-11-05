import { describe, expect, it } from "vitest";
import {
  computeSha256_8,
  maskMemorySourceText,
  scanAndRedactPii,
} from "./piiPatterns.ts";

describe("src/memory/piiPatterns", () => {
  it("computeSha256_8 returns 8-character hex sha256 prefix", () => {
    const hash = computeSha256_8("sample text");
    expect(hash).toHaveLength(8);
    expect(hash).toMatch(/^[0-9a-f]{8}$/);
    expect(computeSha256_8("sample text")).toBe(hash);
  });

  describe("PII Redaction Steps", () => {
    it("redacts Step 1 residual mask tokens [[PII_*]] and [[AI_EXCLUDED_*]]", () => {
      const text =
        "이전 마스크: [[PII_RRN_1]], [[PII_PHONE_2]], [[PII_EMAIL_3]], [[PII_ACCOUNT_4]], [[AI_EXCLUDED_5]]";
      const result = scanAndRedactPii(text);
      expect(result.text).toBe(
        "이전 마스크: <주민번호>, <전화번호>, <이메일>, <계좌번호>, <민감정보>"
      );
      expect(result.totalCount).toBe(5);
    });

    it("redacts Step 2 주민등록번호 (하이픈형 및 무하이픈 13자리)", () => {
      const hyphenRrn = "주민번호 900101-1234567 입니다.";
      const noHyphenRrn = "무하이픈 주민번호 9505052345678 입니다.";
      const res1 = scanAndRedactPii(hyphenRrn);
      const res2 = scanAndRedactPii(noHyphenRrn);
      expect(res1.text).toBe("주민번호 <주민번호> 입니다.");
      expect(res2.text).toBe("무하이픈 주민번호 <주민번호> 입니다.");
    });

    it("redacts Step 3 법인등록번호", () => {
      const corp = "법인번호 110111-1234567 확인";
      const res = scanAndRedactPii(corp);
      expect(res.text).toBe("법인번호 <법인번호> 확인");
    });

    it("redacts Step 4 사업자등록번호", () => {
      const biz = "사업자등록번호 123-45-67890 입니다.";
      const res = scanAndRedactPii(biz);
      expect(res.text).toBe("사업자등록번호 <사업자번호> 입니다.");
    });

    it("redacts Step 5 카드번호", () => {
      const card = "결제 카드: 1234-5678-9012-3456";
      const res = scanAndRedactPii(card);
      expect(res.text).toBe("결제 카드: <카드번호>");
    });

    it("redacts Step 6 이메일", () => {
      const email = "문의: user.name+tag@example.co.kr";
      const res = scanAndRedactPii(email);
      expect(res.text).toBe("문의: <이메일>");
    });

    it("redacts Step 7 전화번호 (휴대전화, 지역번호 02/031, 인터넷 070, 대표 1588)", () => {
      const mobile1 = "연락처 010-1234-5678";
      const mobile2 = "연락처 010 1234 5678";
      const seoul = "서울 사무소 02-123-4567";
      const gyeonggi = "경기 지사 031-987-6543";
      const voip = "고객지원 070-1234-5678";
      const rep = "대표번호 1588-1234";

      expect(scanAndRedactPii(mobile1).text).toBe("연락처 <전화번호>");
      expect(scanAndRedactPii(mobile2).text).toBe("연락처 <전화번호>");
      expect(scanAndRedactPii(seoul).text).toBe("서울 사무소 <전화번호>");
      expect(scanAndRedactPii(gyeonggi).text).toBe("경기 지사 <전화번호>");
      expect(scanAndRedactPii(voip).text).toBe("고객지원 <전화번호>");
      expect(scanAndRedactPii(rep).text).toBe("대표번호 <전화번호>");
    });

    it("redacts Step 8 계좌번호 but NEVER redacts dates like 2026-11-05", () => {
      const dateText = "시행일자: 2026-11-05 및 2025-01-20";
      const dateRes = scanAndRedactPii(dateText);
      expect(dateRes.text).toBe("시행일자: 2026-11-05 및 2025-01-20");
      expect(dateRes.totalCount).toBe(0);

      const labeledAccount = "국민은행: 123-456789-01-012";
      const hyphenAccount = "계좌 123-456-7890123";
      const unhyphenAccount = "입금 110123456789";

      expect(scanAndRedactPii(labeledAccount).text).toBe("국민은행: <계좌번호>");
      expect(scanAndRedactPii(hyphenAccount).text).toBe("계좌 <계좌번호>");
      expect(scanAndRedactPii(unhyphenAccount).text).toBe("입금 <계좌번호>");
    });

    it("redacts Step 9 운전면허번호 및 여권번호", () => {
      const driver = "면허: 11-02-123456-78";
      const passportLabel = "여권번호: M12345678";
      const passportFormat = "여권 M123A4567 소지";

      expect(scanAndRedactPii(driver).text).toBe("면허: <여권/면허번호>");
      expect(scanAndRedactPii(passportLabel).text).toBe("여권번호: <여권/면허번호>");
      expect(scanAndRedactPii(passportFormat).text).toBe("여권 <여권/면허번호> 소지");
    });

    it("redacts Step 10 차량번호 (12가 3456 탐지, 12개월 3456 미탐지)", () => {
      const car = "등록차량 12가 3456 진입";
      const notCar = "계약기간 12개월 3456 번호";

      expect(scanAndRedactPii(car).text).toBe("등록차량 <차량번호> 진입");
      expect(scanAndRedactPii(notCar).text).toBe("계약기간 12개월 3456 번호");
    });

    it("redacts Step 11 주소 (도로명, 지번, 아파트 동호수)", () => {
      const roadAddr = "서울특별시 종로구 세종대로 209 정부서울청사";
      const jibunAddr = "역삼동 123-45 101동 202호";
      const aptAddr = "반포자이아파트 102동 1503호";

      expect(scanAndRedactPii(roadAddr).text).toContain("<주소>");
      expect(scanAndRedactPii(jibunAddr).text).toContain("<주소>");
      expect(scanAndRedactPii(aptAddr).text).toContain("<주소>");
    });

    it("redacts Step 12 성명 (라벨형, 서명형, 직함형, 불용어 제외)", () => {
      const labelName = "담당자: 홍길동";
      const signName = "김철수 (인)";
      const titleName = "박영희 과장";
      const nonName = "업무 과장";

      expect(scanAndRedactPii(labelName).text).toBe("담당자: <이름>");
      expect(scanAndRedactPii(signName).text).toBe("<이름> (인)");
      expect(scanAndRedactPii(titleName).text).toBe("<이름> 과장");
      expect(scanAndRedactPii(nonName).text).toBe("업무 과장");
    });
  });

  describe("maskMemorySourceText", () => {
    it("runs PrivacySessionMasker and then permanently redacts all tokens", () => {
      const text =
        "기안자: 홍길동\n주민번호: 880101-1234567\n전화: 010-9999-8888\n2026-11-05 작성";
      const result = maskMemorySourceText(text);

      expect(result.text).not.toContain("홍길동");
      expect(result.text).not.toContain("880101-1234567");
      expect(result.text).not.toContain("010-9999-8888");
      expect(result.text).not.toContain("[[PII_");
      expect(result.text).toContain("2026-11-05");
      expect(result.text).toContain("<이름>");
      expect(result.text).toContain("<주민번호>");
      expect(result.text).toContain("<전화번호>");
    });
  });
});
