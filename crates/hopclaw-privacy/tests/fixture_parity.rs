use std::path::PathBuf;

use hopclaw_privacy::{
    compute_sha256_8, is_valid_biz_reg_checksum, is_valid_luhn_checksum, is_valid_rrn_checksum,
    mask_memory_source_text, scan_and_redact_pii, ExcludedRegion, PrivacySessionMasker,
    ScanAndRedactResult,
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
struct PrivacyMaskerFixture {
    mask_cases: Vec<MaskCase>,
    filter_cases: Vec<FilterCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaskCase {
    name: String,
    input: String,
    excluded_regions: Vec<ExcludedRegion>,
    auto_mask_pii: bool,
    masked: String,
    unmasked: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilterCase {
    name: String,
    seed_prompt: String,
    excluded_regions: Vec<ExcludedRegion>,
    ops: Vec<Value>,
    result_ops: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiiPatternsFixture {
    sha_cases: Vec<ShaCase>,
    scan_cases: Vec<ScanCase>,
    mask_memory_cases: Vec<ScanCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShaCase {
    input: String,
    #[serde(rename = "sha256_8")]
    sha256_8: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanCase {
    name: String,
    input: String,
    expected: ScanAndRedactResult,
}

#[test]
fn privacy_masker_fixture_parity() {
    let raw = std::fs::read_to_string(fixtures_dir().join("privacy_masker.json")).unwrap();
    let fixture: PrivacyMaskerFixture = serde_json::from_str(&raw).unwrap();

    for tc in fixture.mask_cases {
        let mut masker = PrivacySessionMasker::new();
        let masked = masker.mask_text_for_prompt(&tc.input, &tc.excluded_regions, tc.auto_mask_pii);
        assert_eq!(masked, tc.masked, "mask mismatch in case {}", tc.name);
        let unmasked = masker.unmask_text(&masked);
        assert_eq!(unmasked, tc.unmasked, "unmask mismatch in case {}", tc.name);
    }

    for fc in fixture.filter_cases {
        let mut masker = PrivacySessionMasker::new();
        let _ = masker.mask_text_for_prompt(&fc.seed_prompt, &[], true);
        let actual = masker.unmask_and_filter_json_ops(&fc.ops, &fc.excluded_regions);
        assert_eq!(actual, fc.result_ops, "filter mismatch in case {}", fc.name);
    }
}

#[test]
fn pii_patterns_fixture_parity() {
    let raw = std::fs::read_to_string(fixtures_dir().join("pii_patterns.json")).unwrap();
    let fixture: PiiPatternsFixture = serde_json::from_str(&raw).unwrap();

    for sc in fixture.sha_cases {
        assert_eq!(compute_sha256_8(&sc.input), sc.sha256_8);
    }

    for tc in fixture.scan_cases {
        let actual = scan_and_redact_pii(&tc.input);
        assert_eq!(actual, tc.expected, "scanAndRedactPii mismatch in case {}", tc.name);
    }

    for tc in fixture.mask_memory_cases {
        let actual = mask_memory_source_text(&tc.input);
        assert_eq!(
            actual, tc.expected,
            "maskMemorySourceText mismatch in case {}",
            tc.name
        );
    }
}

#[test]
fn checksum_helpers_validate_known_vectors() {
    // RRN: 900101-1000018 -> 2*9+3*0+4*0+5*1+6*0+7*1 + 8*1+9*0+2*0+3*0+4*0+5*1 = 18+5+7+8+5 = 43, 11 - (43 % 11) = 1 -> wait: 43 % 11 = 10, (11-10)%10 = 1
    assert!(is_valid_rrn_checksum("900101-1000011"));
    assert!(!is_valid_rrn_checksum("900101-1000019"));
    assert!(!is_valid_rrn_checksum("12345"));

    // Biz reg: 120-81-47521 (valid) vs 120-81-47522 (invalid)
    // weights [1,3,7,1,3,7,1,3,5]: 1+6+0+8+3+28+7+15+10 + floor(10/10)=79 => (10 - 9) % 10 = 1
    assert!(is_valid_biz_reg_checksum("120-81-47521"));
    assert!(!is_valid_biz_reg_checksum("120-81-47522"));

    // Luhn: 4111-1111-1111-1111
    assert!(is_valid_luhn_checksum("4111-1111-1111-1111"));
    assert!(!is_valid_luhn_checksum("4111-1111-1111-1112"));
}
