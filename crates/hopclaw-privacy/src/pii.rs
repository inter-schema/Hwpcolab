use std::collections::HashSet;
use std::sync::LazyLock;

use hopclaw_types::{PiiCategory, PiiRedactionHit, ScanAndRedactResult};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::masker::PrivacySessionMasker;
use crate::regex_util::{
    has_js_word_boundaries, has_js_word_boundary_end, has_no_adjacent_ascii_digits,
    is_hangul_syllable, next_char, prev_char, replace_with_captures,
};

const ALL_CATEGORIES_ORDERED: [PiiCategory; 12] = [
    PiiCategory::MaskToken,
    PiiCategory::Rrn,
    PiiCategory::CorpReg,
    PiiCategory::BizReg,
    PiiCategory::Card,
    PiiCategory::Email,
    PiiCategory::Phone,
    PiiCategory::Account,
    PiiCategory::PassportDriver,
    PiiCategory::Vehicle,
    PiiCategory::Address,
    PiiCategory::PersonName,
];

fn category_meta(cat: PiiCategory) -> (&'static str, &'static str) {
    match cat {
        PiiCategory::MaskToken => ("마스크 토큰", "<민감정보>"),
        PiiCategory::Rrn => ("주민/외국인등록번호", "<주민번호>"),
        PiiCategory::CorpReg => ("법인등록번호", "<법인번호>"),
        PiiCategory::BizReg => ("사업자등록번호", "<사업자번호>"),
        PiiCategory::Card => ("카드번호", "<카드번호>"),
        PiiCategory::Email => ("이메일", "<이메일>"),
        PiiCategory::Phone => ("전화번호", "<전화번호>"),
        PiiCategory::Account => ("계좌번호", "<계좌번호>"),
        PiiCategory::PassportDriver => ("여권/운전면허번호", "<여권/면허번호>"),
        PiiCategory::Vehicle => ("차량번호", "<차량번호>"),
        PiiCategory::Address => ("주소", "<주소>"),
        PiiCategory::PersonName => ("성명", "<이름>"),
    }
}

/// Computes the lowercase 8-character hex prefix of the SHA-256 digest of `text`.
pub fn compute_sha256_8(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(&digest[..4])
}

/// Validates a 13-digit Korean Resident/Alien Registration Number (RRN) check digit.
/// Note: `scan_and_redact_pii` intentionally matches syntactically without enforcing
/// checksums so synthetic test/redacted examples are still protected.
pub fn is_valid_rrn_checksum(digits13: &str) -> bool {
    let digits: Vec<u32> = digits13
        .chars()
        .filter(|c| c.is_ascii_digit())
        .filter_map(|c| c.to_digit(10))
        .collect();
    if digits.len() != 13 {
        return false;
    }
    let weights = [2, 3, 4, 5, 6, 7, 8, 9, 2, 3, 4, 5];
    let sum: u32 = weights.iter().zip(&digits[..12]).map(|(w, d)| w * d).sum();
    let expected = (11 - (sum % 11)) % 10;
    expected == digits[12]
}

/// Validates a 10-digit Korean Business Registration Number check digit.
pub fn is_valid_biz_reg_checksum(digits10: &str) -> bool {
    let digits: Vec<u32> = digits10
        .chars()
        .filter(|c| c.is_ascii_digit())
        .filter_map(|c| c.to_digit(10))
        .collect();
    if digits.len() != 10 {
        return false;
    }
    let weights = [1, 3, 7, 1, 3, 7, 1, 3, 5];
    let mut sum: u32 = weights.iter().zip(&digits[..9]).map(|(w, d)| w * d).sum();
    sum += (digits[8] * 5) / 10;
    let check = (10 - (sum % 10)) % 10;
    check == digits[9]
}

/// Validates a payment card number with the Luhn algorithm.
pub fn is_valid_luhn_checksum(number: &str) -> bool {
    let digits: Vec<u32> = number
        .chars()
        .filter(|c| c.is_ascii_digit())
        .filter_map(|c| c.to_digit(10))
        .collect();
    if digits.len() < 12 {
        return false;
    }
    let mut sum = 0u32;
    let mut double_it = false;
    for &d in digits.iter().rev() {
        let val = if double_it {
            let doubled = d * 2;
            if doubled > 9 { doubled - 9 } else { doubled }
        } else {
            d
        };
        sum += val;
        double_it = !double_it;
    }
    sum.is_multiple_of(10)
}

static NON_NAME_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "담당", "총괄", "업무", "행정", "재무", "인사", "기획", "개발", "영업", "관리",
        "운영", "심사", "자문", "전문", "특별", "비상임", "상임", "외부", "내부", "주관",
        "협조", "책임", "수석", "선임", "전임", "신임", "후임", "해당", "관련", "소속",
        "기관", "부서", "학교", "병원", "센터", "교무", "연구", "작성", "검토", "결재",
        "기안", "수신", "발신", "대표", "신청", "민원", "보고",
    ])
});

// Step 1
static RE_TOKEN_RRN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[PII_RRN_[0-9]+\]\]").unwrap());
static RE_TOKEN_PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[PII_PHONE_[0-9]+\]\]").unwrap());
static RE_TOKEN_EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[PII_EMAIL_[0-9]+\]\]").unwrap());
static RE_TOKEN_ACCOUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[PII_ACCOUNT_[0-9]+\]\]").unwrap());
static RE_TOKEN_OTHER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[(?:PII|AI_EXCLUDED)_[A-Z0-9_]+\]\]").unwrap());

// Step 2
static RE_RRN_HYPHEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{6}\s*[-–~]\s*[1-8][0-9]{6}").unwrap());
static RE_CORP_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:법인(?:등록)?(?:번호)?)\s*[:：]?\s*$").unwrap());
static RE_RRN_NOHYPHEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([0-9]{2})(0[1-9]|1[0-2])(0[1-9]|[12][0-9]|3[01])[1-8][0-9]{6}").unwrap()
});

// Step 3..6
static RE_CORP_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{6}\s*[-–]\s*[0-9]{7}").unwrap());
static RE_BIZ_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{3}\s*[-–]\s*[0-9]{2}\s*[-–]\s*[0-9]{5}").unwrap());
static RE_CARD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9]{4}\s*[-– ]\s*[0-9]{4}\s*[-– ]\s*[0-9]{4}\s*[-– ]\s*[0-9]{4}").unwrap()
});
static RE_EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap());

// Step 7
static RE_PHONE_MOBILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"01[016789](?:[-.\s]?[0-9]{3,4})[-.\s]?[0-9]{4}").unwrap());
static RE_PHONE_LANDLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"0(?:2|3[1-3]|4[1-4]|5[1-5]|6[1-4]|70|80|50[0-9])[-.\s]?[0-9]{3,4}[-.\s]?[0-9]{4}")
        .unwrap()
});
static RE_PHONE_REP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"1(?:5|6|8)[0-9]{2}[-.\s][0-9]{4}").unwrap());

// Step 8
static RE_ACCOUNT_LABELED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"((?:계좌(?:번호)?|통장|입금계좌|환불계좌|은행|[가-힣]{2,6}은행|[가-힣]{2,6}금고|[가-힣]{2,6}뱅크)\s*[:：]?\s*)(?:[0-9][0-9\s\-–]{8,18}[0-9])",
    )
    .unwrap()
});
static RE_ACCOUNT_HYPHEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[0-9]{2,6}[-–][0-9]{2,6}[-–][0-9]{4,8}(?:[-–][0-9]{1,4})?").unwrap()
});
static RE_DATE_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{4}[-–][0-9]{2}[-–][0-9]{2}$").unwrap());
static RE_DRIVER_SHAPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[0-9]{2}|[가-힣]{2})[-–][0-9]{2}[-–][0-9]{6}[-–][0-9]{2}$").unwrap()
});
static RE_DRIVER_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:운전)?면허(?:번호)?\s*[:：]?\s*$").unwrap());
static RE_ACCOUNT_CONTINUOUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{11,16}").unwrap());

// Step 9
static RE_DRIVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[0-9]{2}|[가-힣]{2})[-–][0-9]{2}[-–][0-9]{6}[-–][0-9]{2}").unwrap()
});
static RE_PASSPORT_LABELED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)((?:여권(?:번호)?)\s*[:：]?\s*)[A-Z]{1,2}[0-9]{7,8}").unwrap()
});
static RE_PASSPORT_KR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[MSRG][0-9]{3}[A-Z0-9][0-9]{4}|[MSRG][0-9]{8}").unwrap());

// Step 10
static RE_VEHICLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[0-9]{2,3}[가나다라마거너더러머버서어저고노도로모보소오조구누두루무부수우주아바사자배하허호]\s?[0-9]{4}",
    )
    .unwrap()
});

// Step 11
static RE_ADDR_METRO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?:(?:서울|부산|대구|인천|광주|대전|울산|세종)(?:특별시|광역시|특별자치시)?|(?:경기|강원|충북|충남|전북|전남|경북|경남|제주|충청북도|충청남도|전라북도|전라남도|경상북도|경상남도|강원특별자치도|전북특별자치도|제주특별자치도))\s+[가-힣0-9]+(?:시|군|구)(?:\s+[가-힣0-9]+(?:구|읍|면|동|가|리))?(?:\s+[가-힣0-9.\-]+(?:로|길|번길)\s*[0-9]+(?:-[0-9]+)?)?(?:\s+[0-9]+(?:-[0-9]+)?)?(?:\s*(?:,|\s)\s*[가-힣0-9A-Za-z\-]+(?:아파트|빌라|타워|오피스텔|빌딩|센터)?\s*(?:[0-9]+동)?\s*(?:[0-9]+호|[0-9]+층)?)?",
    )
    .unwrap()
});
static RE_ADDR_ROAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[가-힣0-9]{2,12}(?:로|길|번길)\s+[0-9]{1,4}(?:-[0-9]{1,4})?(?:\s+[가-힣0-9A-Za-z\-]+(?:아파트|빌라|타워|오피스텔|빌딩|센터|관))?(?:\s+[0-9]{1,4}동)?(?:\s+[0-9]{1,4}(?:호|층))?",
    )
    .unwrap()
});
static RE_ADDR_JIBUN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[가-힣]{2,10}(?:동|읍|면|리)\s+[0-9]{1,4}-[0-9]{1,4}(?:\s+[0-9]{1,4}(?:동|호|층))*")
        .unwrap()
});
static RE_ADDR_APT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[가-힣A-Za-z0-9]{2,15}아파트\s*[0-9]{1,4}동\s*[0-9]{1,4}호").unwrap()
});

// Step 12
static RE_NAME_LABELED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"((?:성\s*명|이\s*름|담당(?:자)?|작성자|기안자|검토자|결재자|수신자|발신자|대표자|신청인|민원인|보고자|참고인|교사명|지도교사)\s*[:：]\s*)([가-힣]{2,4})",
    )
    .unwrap()
});
static RE_NAME_SIGN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([가-힣]{2,4})(\s*\((?:인|서명|印)\))").unwrap());
static RE_NAME_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"([가-힣]{2,4})(\s*(?:과장|부장|팀장|차장|대리|사원|주임|계장|국장|실장|처장|청장|원장|관장|교장|교감|교수|선생님|연구원|위원장|위원|주무관|사무관|서기관|이사관))",
    )
    .unwrap()
});

fn replace_simple(text: &str, re: &Regex, placeholder: &str, counter: &mut usize) -> String {
    replace_with_captures(
        text,
        re,
        |_, _| true,
        |_, _| {
            *counter += 1;
            Some(placeholder.to_string())
        },
    )
}

fn replace_no_adj_digits(text: &str, re: &Regex, placeholder: &str, counter: &mut usize) -> String {
    replace_with_captures(
        text,
        re,
        |s, caps| {
            let m = caps.get(0).unwrap();
            has_no_adjacent_ascii_digits(s, m.start(), m.end())
        },
        |_, _| {
            *counter += 1;
            Some(placeholder.to_string())
        },
    )
}

/// Scans `input` across all 12 Korean PII categories (covering 14+ patterns) and replaces
/// detected items with irreversible Korean placeholders.
pub fn scan_and_redact_pii(input: &str) -> ScanAndRedactResult {
    let mut text = input.to_string();
    let mut counts = [0usize; 12];

    let idx = |cat: PiiCategory| -> usize {
        match cat {
            PiiCategory::MaskToken => 0,
            PiiCategory::Rrn => 1,
            PiiCategory::CorpReg => 2,
            PiiCategory::BizReg => 3,
            PiiCategory::Card => 4,
            PiiCategory::Email => 5,
            PiiCategory::Phone => 6,
            PiiCategory::Account => 7,
            PiiCategory::PassportDriver => 8,
            PiiCategory::Vehicle => 9,
            PiiCategory::Address => 10,
            PiiCategory::PersonName => 11,
        }
    };

    // Step 1 (mask_token)
    text = replace_simple(
        &text,
        &RE_TOKEN_RRN,
        "<주민번호>",
        &mut counts[idx(PiiCategory::MaskToken)],
    );
    text = replace_simple(
        &text,
        &RE_TOKEN_PHONE,
        "<전화번호>",
        &mut counts[idx(PiiCategory::MaskToken)],
    );
    text = replace_simple(
        &text,
        &RE_TOKEN_EMAIL,
        "<이메일>",
        &mut counts[idx(PiiCategory::MaskToken)],
    );
    text = replace_simple(
        &text,
        &RE_TOKEN_ACCOUNT,
        "<계좌번호>",
        &mut counts[idx(PiiCategory::MaskToken)],
    );
    text = replace_simple(
        &text,
        &RE_TOKEN_OTHER,
        "<민감정보>",
        &mut counts[idx(PiiCategory::MaskToken)],
    );

    // Step 2 (rrn)
    text = replace_with_captures(
        &text,
        &RE_RRN_HYPHEN,
        |s, caps| {
            let m = caps.get(0).unwrap();
            has_no_adjacent_ascii_digits(s, m.start(), m.end())
        },
        |s, caps| {
            let m = caps.get(0).unwrap();
            let prefix = &s[..m.start()];
            if RE_CORP_PREFIX.is_match(prefix) {
                return None;
            }
            counts[idx(PiiCategory::Rrn)] += 1;
            Some("<주민번호>".to_string())
        },
    );
    text = replace_no_adj_digits(
        &text,
        &RE_RRN_NOHYPHEN,
        "<주민번호>",
        &mut counts[idx(PiiCategory::Rrn)],
    );

    // Step 3 (corp_reg)
    text = replace_no_adj_digits(
        &text,
        &RE_CORP_REG,
        "<법인번호>",
        &mut counts[idx(PiiCategory::CorpReg)],
    );

    // Step 4 (biz_reg)
    text = replace_no_adj_digits(
        &text,
        &RE_BIZ_REG,
        "<사업자번호>",
        &mut counts[idx(PiiCategory::BizReg)],
    );

    // Step 5 (card)
    text = replace_no_adj_digits(
        &text,
        &RE_CARD,
        "<카드번호>",
        &mut counts[idx(PiiCategory::Card)],
    );

    // Step 6 (email)
    text = replace_simple(
        &text,
        &RE_EMAIL,
        "<이메일>",
        &mut counts[idx(PiiCategory::Email)],
    );

    // Step 7 (phone)
    text = replace_no_adj_digits(
        &text,
        &RE_PHONE_MOBILE,
        "<전화번호>",
        &mut counts[idx(PiiCategory::Phone)],
    );
    text = replace_no_adj_digits(
        &text,
        &RE_PHONE_LANDLINE,
        "<전화번호>",
        &mut counts[idx(PiiCategory::Phone)],
    );
    text = replace_no_adj_digits(
        &text,
        &RE_PHONE_REP,
        "<전화번호>",
        &mut counts[idx(PiiCategory::Phone)],
    );

    // Step 8 (account)
    text = replace_with_captures(
        &text,
        &RE_ACCOUNT_LABELED,
        |_, _| true,
        |_, caps| {
            counts[idx(PiiCategory::Account)] += 1;
            let prefix = caps.get(1).unwrap().as_str();
            Some(format!("{prefix}<계좌번호>"))
        },
    );
    text = replace_with_captures(
        &text,
        &RE_ACCOUNT_HYPHEN,
        |s, caps| {
            let m = caps.get(0).unwrap();
            has_no_adjacent_ascii_digits(s, m.start(), m.end())
        },
        |s, caps| {
            let m = caps.get(0).unwrap();
            let mat_str = m.as_str();
            if RE_DATE_ONLY.is_match(mat_str) || RE_DRIVER_SHAPE.is_match(mat_str) {
                return None;
            }
            let prefix = &s[..m.start()];
            if RE_DRIVER_PREFIX.is_match(prefix) {
                return None;
            }
            let digit_count = mat_str.chars().filter(|c| c.is_ascii_digit()).count();
            if (10..=16).contains(&digit_count) {
                counts[idx(PiiCategory::Account)] += 1;
                Some("<계좌번호>".to_string())
            } else {
                None
            }
        },
    );
    text = replace_no_adj_digits(
        &text,
        &RE_ACCOUNT_CONTINUOUS,
        "<계좌번호>",
        &mut counts[idx(PiiCategory::Account)],
    );

    // Step 9 (passport_driver)
    text = replace_no_adj_digits(
        &text,
        &RE_DRIVER,
        "<여권/면허번호>",
        &mut counts[idx(PiiCategory::PassportDriver)],
    );
    text = replace_with_captures(
        &text,
        &RE_PASSPORT_LABELED,
        |s, caps| {
            let m = caps.get(0).unwrap();
            has_js_word_boundary_end(s, m.start(), m.end())
        },
        |_, caps| {
            counts[idx(PiiCategory::PassportDriver)] += 1;
            let prefix = caps.get(1).unwrap().as_str();
            Some(format!("{prefix}<여권/면허번호>"))
        },
    );
    text = replace_with_captures(
        &text,
        &RE_PASSPORT_KR,
        |s, caps| {
            let m = caps.get(0).unwrap();
            has_js_word_boundaries(s, m.start(), m.end())
        },
        |_, _| {
            counts[idx(PiiCategory::PassportDriver)] += 1;
            Some("<여권/면허번호>".to_string())
        },
    );

    // Step 10 (vehicle)
    text = replace_no_adj_digits(
        &text,
        &RE_VEHICLE,
        "<차량번호>",
        &mut counts[idx(PiiCategory::Vehicle)],
    );

    // Step 11 (address)
    text = replace_simple(
        &text,
        &RE_ADDR_METRO,
        "<주소>",
        &mut counts[idx(PiiCategory::Address)],
    );
    text = replace_simple(
        &text,
        &RE_ADDR_ROAD,
        "<주소>",
        &mut counts[idx(PiiCategory::Address)],
    );
    text = replace_simple(
        &text,
        &RE_ADDR_JIBUN,
        "<주소>",
        &mut counts[idx(PiiCategory::Address)],
    );
    text = replace_simple(
        &text,
        &RE_ADDR_APT,
        "<주소>",
        &mut counts[idx(PiiCategory::Address)],
    );

    // Step 12 (person_name)
    text = replace_with_captures(
        &text,
        &RE_NAME_LABELED,
        |s, caps| {
            let m = caps.get(0).unwrap();
            !next_char(s, m.end()).is_some_and(is_hangul_syllable)
        },
        |_, caps| {
            let prefix = caps.get(1).unwrap().as_str();
            let name = caps.get(2).unwrap().as_str();
            if !NON_NAME_WORDS.contains(name) {
                counts[idx(PiiCategory::PersonName)] += 1;
                Some(format!("{prefix}<이름>"))
            } else {
                None
            }
        },
    );
    text = replace_with_captures(
        &text,
        &RE_NAME_SIGN,
        |_, _| true,
        |_, caps| {
            let name = caps.get(1).unwrap().as_str();
            let suffix = caps.get(2).unwrap().as_str();
            if !NON_NAME_WORDS.contains(name) {
                counts[idx(PiiCategory::PersonName)] += 1;
                Some(format!("<이름>{suffix}"))
            } else {
                None
            }
        },
    );
    text = replace_with_captures(
        &text,
        &RE_NAME_TITLE,
        |s, caps| {
            let m = caps.get(0).unwrap();
            !prev_char(s, m.start()).is_some_and(is_hangul_syllable)
                && !next_char(s, m.end()).is_some_and(is_hangul_syllable)
        },
        |_, caps| {
            let name = caps.get(1).unwrap().as_str();
            let title = caps.get(2).unwrap().as_str();
            if !NON_NAME_WORDS.contains(name) {
                counts[idx(PiiCategory::PersonName)] += 1;
                Some(format!("<이름>{title}"))
            } else {
                None
            }
        },
    );

    let mut hits = Vec::new();
    let mut hit_details = Vec::new();
    let mut total_count = 0usize;

    for cat in ALL_CATEGORIES_ORDERED {
        let count = counts[idx(cat)];
        if count > 0 {
            let (label, placeholder) = category_meta(cat);
            total_count += count;
            hits.push(format!("{label} {count}건 -> {placeholder}"));
            hit_details.push(PiiRedactionHit {
                category: cat,
                placeholder: placeholder.to_string(),
                count,
            });
        }
    }

    ScanAndRedactResult {
        text,
        hits,
        hit_details,
        total_count,
    }
}

/// Runs `PrivacySessionMasker` with `auto_mask_pii = true` and then permanently
/// redacts all residual tokens and PII via `scan_and_redact_pii`.
pub fn mask_memory_source_text(raw_text: &str) -> ScanAndRedactResult {
    let mut masker = PrivacySessionMasker::new();
    let step1 = masker.mask_text_for_prompt(raw_text, &[], true);
    scan_and_redact_pii(&step1)
}
