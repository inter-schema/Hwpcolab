import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";
import { PrivacySessionMasker } from "../core/privacyMasker.ts";
import type {
  PiiCategory,
  PiiRedactionHit,
  ScanAndRedactResult,
} from "./types.ts";

export function computeSha256_8(text: string): string {
  return bytesToHex(sha256(new TextEncoder().encode(text))).slice(0, 8);
}

const NON_NAME_WORDS = new Set([
  "담당",
  "총괄",
  "업무",
  "행정",
  "재무",
  "인사",
  "기획",
  "개발",
  "영업",
  "관리",
  "운영",
  "심사",
  "자문",
  "전문",
  "특별",
  "비상임",
  "상임",
  "외부",
  "내부",
  "주관",
  "협조",
  "책임",
  "수석",
  "선임",
  "전임",
  "신임",
  "후임",
  "해당",
  "관련",
  "소속",
  "기관",
  "부서",
  "학교",
  "병원",
  "센터",
  "교무",
  "연구",
  "작성",
  "검토",
  "결재",
  "기안",
  "수신",
  "발신",
  "대표",
  "신청",
  "민원",
  "보고",
]);

export function scanAndRedactPii(input: string): ScanAndRedactResult {
  let text = input;
  const counts: Record<PiiCategory, number> = {
    mask_token: 0,
    rrn: 0,
    corp_reg: 0,
    biz_reg: 0,
    card: 0,
    phone: 0,
    email: 0,
    account: 0,
    passport_driver: 0,
    vehicle: 0,
    address: 0,
    person_name: 0,
  };

  // Step 1 (mask_token)
  text = text.replace(/\[\[PII_RRN_\d+\]\]/g, () => {
    counts.mask_token += 1;
    return "<주민번호>";
  });
  text = text.replace(/\[\[PII_PHONE_\d+\]\]/g, () => {
    counts.mask_token += 1;
    return "<전화번호>";
  });
  text = text.replace(/\[\[PII_EMAIL_\d+\]\]/g, () => {
    counts.mask_token += 1;
    return "<이메일>";
  });
  text = text.replace(/\[\[PII_ACCOUNT_\d+\]\]/g, () => {
    counts.mask_token += 1;
    return "<계좌번호>";
  });
  text = text.replace(/\[\[(?:PII|AI_EXCLUDED)_[A-Z0-9_]+\]\]/g, () => {
    counts.mask_token += 1;
    return "<민감정보>";
  });

  // Step 2 (rrn, 라벨: "주민/외국인등록번호", 플레이스홀더: <주민번호>)
  // 하이픈형 (단, '법인', '법인번호', '법인등록번호' 라벨이 선행하는 경우 Step 3 법인등록번호로 처리)
  text = text.replace(
    /(?<!\d)\d{6}\s*[-–~]\s*[1-8]\d{6}(?!\d)/g,
    (match, offset, fullStr) => {
      const prefix = fullStr.slice(0, offset);
      if (/(?:법인(?:등록)?(?:번호)?)\s*[:：]?\s*$/.test(prefix)) {
        return match;
      }
      counts.rrn += 1;
      return "<주민번호>";
    }
  );
  // 무하이픈 13자리형
  text = text.replace(
    /(?<!\d)(\d{2})(0[1-9]|1[0-2])(0[1-9]|[12]\d|3[01])[1-8]\d{6}(?!\d)/g,
    () => {
      counts.rrn += 1;
      return "<주민번호>";
    }
  );

  // Step 3 (corp_reg, 라벨: "법인등록번호", 플레이스홀더: <법인번호>)
  text = text.replace(/(?<!\d)\d{6}\s*[-–]\s*\d{7}(?!\d)/g, () => {
    counts.corp_reg += 1;
    return "<법인번호>";
  });

  // Step 4 (biz_reg, 라벨: "사업자등록번호", 플레이스홀더: <사업자번호>)
  text = text.replace(/(?<!\d)\d{3}\s*[-–]\s*\d{2}\s*[-–]\s*\d{5}(?!\d)/g, () => {
    counts.biz_reg += 1;
    return "<사업자번호>";
  });

  // Step 5 (card, 라벨: "카드번호", 플레이스홀더: <카드번호>)
  text = text.replace(/(?<!\d)\d{4}\s*[-– ]\s*\d{4}\s*[-– ]\s*\d{4}\s*[-– ]\s*\d{4}(?!\d)/g, () => {
    counts.card += 1;
    return "<카드번호>";
  });

  // Step 6 (email, 라벨: "이메일", 플레이스홀더: <이메일>)
  text = text.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, () => {
    counts.email += 1;
    return "<이메일>";
  });

  // Step 7 (phone, 라벨: "전화번호", 플레이스홀더: <전화번호>)
  // 휴대전화
  text = text.replace(/(?<!\d)01[016789](?:[-.\s]?\d{3,4})[-.\s]?\d{4}(?!\d)/g, () => {
    counts.phone += 1;
    return "<전화번호>";
  });
  // 지역번호/인터넷전화
  text = text.replace(/(?<!\d)0(?:2|3[1-3]|4[1-4]|5[1-5]|6[1-4]|70|80|50\d)[-.\s]?\d{3,4}[-.\s]?\d{4}(?!\d)/g, () => {
    counts.phone += 1;
    return "<전화번호>";
  });
  // 대표번호
  text = text.replace(/(?<!\d)1(?:5|6|8)\d{2}[-.\s]\d{4}(?!\d)/g, () => {
    counts.phone += 1;
    return "<전화번호>";
  });

  // Step 8 (account, 라벨: "계좌번호", 플레이스홀더: <계좌번호>)
  // 라벨 동반형
  text = text.replace(
    /((?:계좌(?:번호)?|통장|입금계좌|환불계좌|은행|[가-힣]{2,6}은행|[가-힣]{2,6}금고|[가-힣]{2,6}뱅크)\s*[:：]?\s*)(?:\d[\d\s-–]{8,18}\d)/g,
    (_match, prefix) => {
      counts.account += 1;
      return `${prefix}<계좌번호>`;
    }
  );
  // 무라벨 하이픈 계좌형(날짜 제외, 운전면허번호 제외, 총 숫자 10~16자리)
  text = text.replace(
    /(?<!\d)\d{2,6}[-–]\d{2,6}[-–]\d{4,8}(?:[-–]\d{1,4})?(?!\d)/g,
    (match, offset, fullStr) => {
      // 날짜(YYYY-MM-DD 등) 제외
      if (/^\d{4}[-–]\d{2}[-–]\d{2}$/.test(match)) {
        return match;
      }
      // 운전면허번호 포맷(XX-XX-XXXXXX-XX) 또는 선행 면허 라벨 제외 -> Step 9에서 처리
      if (/^(?:\d{2}|[가-힣]{2})[-–]\d{2}[-–]\d{6}[-–]\d{2}$/.test(match)) {
        return match;
      }
      const prefix = fullStr.slice(0, offset);
      if (/(?:운전)?면허(?:번호)?\s*[:：]?\s*$/.test(prefix)) {
        return match;
      }

      const digitsOnly = match.replace(/\D/g, "");
      if (digitsOnly.length >= 10 && digitsOnly.length <= 16) {
        counts.account += 1;
        return "<계좌번호>";
      }
      return match;
    }
  );
  // 무라벨 연속 숫자형(11~16자리)
  text = text.replace(/(?<!\d)\d{11,16}(?!\d)/g, () => {
    counts.account += 1;
    return "<계좌번호>";
  });

  // Step 9 (passport_driver, 라벨: "여권/운전면허번호", 플레이스홀더: <여권/면허번호>)
  // 운전면허
  text = text.replace(
    /(?<!\d)(?:\d{2}|[가-힣]{2})[-–]\d{2}[-–]\d{6}[-–]\d{2}(?!\d)/g,
    () => {
      counts.passport_driver += 1;
      return "<여권/면허번호>";
    }
  );
  // 여권(라벨형)
  text = text.replace(
    /((?:여권(?:번호)?)\s*[:：]?\s*)[A-Z]{1,2}\d{7,8}\b/gi,
    (_match, prefix) => {
      counts.passport_driver += 1;
      return `${prefix}<여권/면허번호>`;
    }
  );
  // 여권(한국 전자여권 포맷)
  text = text.replace(/\b[MSRG]\d{3}[A-Z0-9]\d{4}\b|\b[MSRG]\d{8}\b/g, () => {
    counts.passport_driver += 1;
    return "<여권/면허번호>";
  });

  // Step 10 (vehicle, 라벨: "차량번호", 플레이스홀더: <차량번호>)
  text = text.replace(
    /(?<!\d)\d{2,3}[가나다라마거너더러머버서어저고노도로모보소오조구누두루무부수우주아바사자배하허호]\s?\d{4}(?!\d)/g,
    () => {
      counts.vehicle += 1;
      return "<차량번호>";
    }
  );

  // Step 11 (address, 라벨: "주소", 플레이스홀더: <주소>)
  // 광역+시군구형
  text = text.replace(
    /(?:(?:서울|부산|대구|인천|광주|대전|울산|세종)(?:특별시|광역시|특별자치시)?|(?:경기|강원|충북|충남|전북|전남|경북|경남|제주|충청북도|충청남도|전라북도|전라남도|경상북도|경상남도|강원특별자치도|전북특별자치도|제주특별자치도))\s+[가-힣0-9]+(?:시|군|구)(?:\s+[가-힣0-9]+(?:구|읍|면|동|가|리))?(?:\s+[가-힣0-9.-]+(?:로|길|번길)\s*\d+(?:-\d+)?)?(?:\s+\d+(?:-\d+)?)?(?:\s*(?:,|\s)\s*[가-힣0-9A-Za-z-]+(?:아파트|빌라|타워|오피스텔|빌딩|센터)?\s*(?:\d+동)?\s*(?:\d+호|\d+층)?)?/g,
    () => {
      counts.address += 1;
      return "<주소>";
    }
  );
  // 도로명 단독형
  text = text.replace(
    /[가-힣0-9]{2,12}(?:로|길|번길)\s+\d{1,4}(?:-\d{1,4})?(?:\s+[가-힣0-9A-Za-z-]+(?:아파트|빌라|타워|오피스텔|빌딩|센터|관))?(?:\s+\d{1,4}동)?(?:\s+\d{1,4}(?:호|층))?/g,
    () => {
      counts.address += 1;
      return "<주소>";
    }
  );
  // 지번/아파트 동호수 단독형
  text = text.replace(
    /[가-힣]{2,10}(?:동|읍|면|리)\s+\d{1,4}-\d{1,4}(?:\s+\d{1,4}(?:동|호|층))*/g,
    () => {
      counts.address += 1;
      return "<주소>";
    }
  );
  text = text.replace(
    /[가-힣A-Za-z0-9]{2,15}아파트\s*\d{1,4}동\s*\d{1,4}호/g,
    () => {
      counts.address += 1;
      return "<주소>";
    }
  );

  // Step 12 (person_name, 라벨: "성명", 플레이스홀더: <이름>)
  // 라벨 동반 성명
  text = text.replace(
    /((?:성\s*명|이\s*름|담당(?:자)?|작성자|기안자|검토자|결재자|수신자|발신자|대표자|신청인|민원인|보고자|참고인|교사명|지도교사)\s*[:：]\s*)([가-힣]{2,4})(?![가-힣])/g,
    (_match, prefix, name) => {
      if (!NON_NAME_WORDS.has(name)) {
        counts.person_name += 1;
        return `${prefix}<이름>`;
      }
      return _match;
    }
  );
  // 서명/날인 병기 성명
  text = text.replace(
    /([가-힣]{2,4})(\s*\((?:인|서명|印)\))/g,
    (_match, name, suffix) => {
      if (!NON_NAME_WORDS.has(name)) {
        counts.person_name += 1;
        return `<이름>${suffix}`;
      }
      return _match;
    }
  );
  // 직함 동반 성명
  text = text.replace(
    /(?<![가-힣])([가-힣]{2,4})(\s*(?:과장|부장|팀장|차장|대리|사원|주임|계장|국장|실장|처장|청장|원장|관장|교장|교감|교수|선생님|연구원|위원장|위원|주무관|사무관|서기관|이사관))(?![가-힣])/g,
    (_match, name, title) => {
      if (!NON_NAME_WORDS.has(name)) {
        counts.person_name += 1;
        return `<이름>${title}`;
      }
      return _match;
    }
  );

  const categoryMeta: Record<PiiCategory, { label: string; placeholder: string }> = {
    mask_token: { label: "마스크 토큰", placeholder: "<민감정보>" },
    rrn: { label: "주민/외국인등록번호", placeholder: "<주민번호>" },
    corp_reg: { label: "법인등록번호", placeholder: "<법인번호>" },
    biz_reg: { label: "사업자등록번호", placeholder: "<사업자번호>" },
    card: { label: "카드번호", placeholder: "<카드번호>" },
    email: { label: "이메일", placeholder: "<이메일>" },
    phone: { label: "전화번호", placeholder: "<전화번호>" },
    account: { label: "계좌번호", placeholder: "<계좌번호>" },
    passport_driver: { label: "여권/운전면허번호", placeholder: "<여권/면허번호>" },
    vehicle: { label: "차량번호", placeholder: "<차량번호>" },
    address: { label: "주소", placeholder: "<주소>" },
    person_name: { label: "성명", placeholder: "<이름>" },
  };

  const categories: PiiCategory[] = [
    "mask_token",
    "rrn",
    "corp_reg",
    "biz_reg",
    "card",
    "email",
    "phone",
    "account",
    "passport_driver",
    "vehicle",
    "address",
    "person_name",
  ];

  const hits: string[] = [];
  const hitDetails: PiiRedactionHit[] = [];
  let totalCount = 0;

  for (const cat of categories) {
    const count = counts[cat];
    if (count > 0) {
      const meta = categoryMeta[cat];
      totalCount += count;
      hits.push(`${meta.label} ${count}건 -> ${meta.placeholder}`);
      hitDetails.push({
        category: cat,
        placeholder: meta.placeholder,
        count,
      });
    }
  }

  return {
    text,
    hits,
    hitDetails,
    totalCount,
  };
}

export function maskMemorySourceText(rawText: string): ScanAndRedactResult {
  const masker = new PrivacySessionMasker();
  const step1 = masker.maskTextForPrompt(rawText, [], true);
  return scanAndRedactPii(step1);
}
