use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use hopclaw_types::{ExcludedRegion, ExcludedRegionTargetType, UnifiedOp};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::regex_util::{has_js_word_boundaries, next_char, replace_with_captures};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiiTokenMap {
    pub token: String,
    pub raw_value: String,
}

static RE_PROMPT_RRN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{6}-[1-8][0-9]{6}").unwrap());
static RE_PROMPT_PHONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"01[016789]-[0-9]{3,4}-[0-9]{4}").unwrap());
static RE_PROMPT_EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap());
static RE_PROMPT_ACCOUNT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)((?:계좌(?:번호)?|통장(?:번호)?|bank\s*account(?:\s*(?:number|no\.?))?)\s*[:：#]?\s*)([0-9]{10,16}|[0-9]{2,8}(?:[- ][0-9]{2,8}){1,3})",
    )
    .unwrap()
});

/// Session-scoped in-memory pseudonymizer for LLM prompts and UnifiedOp restoration.
/// Token mappings never leave process memory and are never persisted to disk.
#[derive(Debug, Default)]
pub struct PrivacySessionMasker {
    token_to_raw: Vec<(String, String)>,
    raw_to_token: HashMap<String, String>,
    counter: usize,
}

impl PrivacySessionMasker {
    pub fn new() -> Self {
        Self::default()
    }

    fn register_token(&mut self, prefix: &str, raw_value: &str) -> String {
        if let Some(existing) = self.raw_to_token.get(raw_value) {
            return existing.clone();
        }
        self.counter += 1;
        let token = format!("[[{prefix}_{}]]", self.counter);
        self.token_to_raw
            .push((token.clone(), raw_value.to_string()));
        self.raw_to_token
            .insert(raw_value.to_string(), token.clone());
        token
    }

    /// Replaces user-excluded regions with `[[AI_EXCLUDED_N]]` and, when `auto_mask_pii`
    /// is enabled, pseudonymizes RRN, mobile phone, email, and labeled bank accounts.
    pub fn mask_text_for_prompt(
        &mut self,
        input_text: &str,
        excluded_regions: &[ExcludedRegion],
        auto_mask_pii: bool,
    ) -> String {
        let mut result = input_text.to_string();

        // 1. User-excluded regions
        for region in excluded_regions {
            if !region.raw_text.trim().is_empty() {
                let token = self.register_token("AI_EXCLUDED", &region.raw_text);
                result = result.replace(&region.raw_text, &token);
            }
        }

        // 2. Optional PII pseudonymization
        if auto_mask_pii {
            result = replace_with_captures(
                &result,
                &RE_PROMPT_RRN,
                |s, caps| {
                    let m = caps.get(0).unwrap();
                    has_js_word_boundaries(s, m.start(), m.end())
                },
                |_, caps| Some(self.register_token("PII_RRN", caps.get(0).unwrap().as_str())),
            );

            result = replace_with_captures(
                &result,
                &RE_PROMPT_PHONE,
                |s, caps| {
                    let m = caps.get(0).unwrap();
                    has_js_word_boundaries(s, m.start(), m.end())
                },
                |_, caps| Some(self.register_token("PII_PHONE", caps.get(0).unwrap().as_str())),
            );

            result = replace_with_captures(
                &result,
                &RE_PROMPT_EMAIL,
                |s, caps| {
                    let m = caps.get(0).unwrap();
                    has_js_word_boundaries(s, m.start(), m.end())
                },
                |_, caps| Some(self.register_token("PII_EMAIL", caps.get(0).unwrap().as_str())),
            );

            result = replace_with_captures(
                &result,
                &RE_PROMPT_ACCOUNT,
                |s, caps| {
                    let m = caps.get(0).unwrap();
                    !next_char(s, m.end()).is_some_and(|c| c.is_ascii_digit())
                },
                |_, caps| {
                    let label = caps.get(1).unwrap().as_str();
                    let account = caps.get(2).unwrap().as_str();
                    let digit_count = account.chars().filter(|c| c.is_ascii_digit()).count();
                    if !(10..=16).contains(&digit_count) {
                        return None;
                    }
                    let token = self.register_token("PII_ACCOUNT", account);
                    Some(format!("{label}{token}"))
                },
            );
        }

        result
    }

    /// Restores pseudonym tokens (`[[...]]`) in text back to their original raw strings.
    pub fn unmask_text(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let mut out = text.to_string();
        for (token, raw) in &self.token_to_raw {
            out = out.replace(token, raw);
        }
        out
    }

    /// Filters out ops targeting excluded cells and restores tokens in typed `UnifiedOp`s.
    pub fn unmask_and_filter_ops(
        &self,
        ops: &[UnifiedOp],
        excluded_regions: &[ExcludedRegion],
    ) -> Vec<UnifiedOp> {
        let excluded_cell_keys = excluded_cell_key_set(excluded_regions);
        ops.iter()
            .filter(|op| !op_targets_excluded_cell(op, &excluded_cell_keys))
            .map(|op| self.unmask_op(op))
            .collect()
    }

    /// Filters out ops targeting excluded cells and unmasks tokens inside JSON-encoded `UnifiedOp` values.
    pub fn unmask_and_filter_json_ops(
        &self,
        ops: &[Value],
        excluded_regions: &[ExcludedRegion],
    ) -> Vec<Value> {
        let excluded_cell_keys = excluded_cell_key_set(excluded_regions);

        ops.iter()
            .filter(|op| {
                if let (Some(ti), Some(r), Some(c)) = (
                    op.get("tableIndex").and_then(Value::as_u64),
                    op.get("row").and_then(Value::as_u64),
                    op.get("col").and_then(Value::as_u64),
                ) {
                    if excluded_cell_keys.contains(&(ti as usize, r as usize, c as usize)) {
                        return false;
                    }
                }
                true
            })
            .map(|op| {
                let mut cloned = op.clone();
                if let Some(obj) = cloned.as_object_mut() {
                    for key in ["text", "find", "replace", "targetText", "anchorText"] {
                        if let Some(Value::String(s)) = obj.get_mut(key) {
                            *s = self.unmask_text(s);
                        }
                    }
                    for arr_key in ["headers", "cells"] {
                        if let Some(Value::Array(arr)) = obj.get_mut(arr_key) {
                            for item in arr.iter_mut() {
                                if let Value::String(s) = item {
                                    *s = self.unmask_text(s);
                                }
                            }
                        }
                    }
                    for matrix_key in ["rows", "data"] {
                        if let Some(Value::Array(rows)) = obj.get_mut(matrix_key) {
                            for row in rows.iter_mut() {
                                if let Value::Array(cols) = row {
                                    for cell in cols.iter_mut() {
                                        if let Value::String(s) = cell {
                                            *s = self.unmask_text(s);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                cloned
            })
            .collect()
    }

    fn unmask_op(&self, op: &UnifiedOp) -> UnifiedOp {
        let mut cloned = op.clone();
        match &mut cloned {
            UnifiedOp::ReplaceBodyText { find, replace } => {
                *find = self.unmask_text(find);
                *replace = self.unmask_text(replace);
            }
            UnifiedOp::SetParagraphText { text, .. }
            | UnifiedOp::AppendParagraph { text, .. }
            | UnifiedOp::AppendHeading { text, .. }
            | UnifiedOp::SetCellText { text, .. }
            | UnifiedOp::SetHeaderFooter { text, .. } => {
                *text = self.unmask_text(text);
            }
            UnifiedOp::InsertTable { headers, rows, .. } => {
                for h in headers.iter_mut() {
                    *h = self.unmask_text(h);
                }
                for row in rows.iter_mut() {
                    for cell in row.iter_mut() {
                        *cell = self.unmask_text(cell);
                    }
                }
            }
            UnifiedOp::InsertNestedTable {
                data: Some(rows), ..
            } => {
                for row in rows.iter_mut() {
                    for cell in row.iter_mut() {
                        *cell = self.unmask_text(cell);
                    }
                }
            }
            UnifiedOp::InsertTableRow {
                cells: Some(cells), ..
            }
            | UnifiedOp::InsertTableCol {
                cells: Some(cells), ..
            } => {
                for c in cells.iter_mut() {
                    *c = self.unmask_text(c);
                }
            }
            UnifiedOp::ApplyTextStyle { target_text, .. } => {
                *target_text = self.unmask_text(target_text);
            }
            UnifiedOp::ApplyParagraphStyle { target_text, .. }
            | UnifiedOp::SetListStyle { target_text, .. } => {
                if let Some(t) = target_text.as_mut() {
                    *t = self.unmask_text(t);
                }
            }
            UnifiedOp::InsertImage { anchor_text, .. } => {
                if let Some(a) = anchor_text.as_mut() {
                    *a = self.unmask_text(a);
                }
            }
            UnifiedOp::PlaceSeal { anchor_text, .. } => {
                *anchor_text = self.unmask_text(anchor_text);
            }
            UnifiedOp::InsertFootnote {
                anchor_text, text, ..
            } => {
                *anchor_text = self.unmask_text(anchor_text);
                *text = self.unmask_text(text);
            }
            _ => {}
        }
        cloned
    }
}

fn excluded_cell_key_set(excluded_regions: &[ExcludedRegion]) -> HashSet<(usize, usize, usize)> {
    excluded_regions
        .iter()
        .filter(|r| r.target_type == ExcludedRegionTargetType::Cell)
        .filter_map(|r| Some((r.table_index?, r.row?, r.col?)))
        .collect()
}

fn op_targets_excluded_cell(op: &UnifiedOp, excluded: &HashSet<(usize, usize, usize)>) -> bool {
    let coord = match op {
        UnifiedOp::SetCellText {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::InsertNestedTable {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::UnmergeCells {
            table_index,
            row,
            col,
        }
        | UnifiedOp::SplitCell {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellBackground {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellAlign {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellBorder {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::SetCellPadding {
            table_index,
            row,
            col,
            ..
        }
        | UnifiedOp::ShrinkCellFont {
            table_index,
            row,
            col,
            ..
        } => Some((*table_index, *row, *col)),
        _ => None,
    };
    coord.is_some_and(|k| excluded.contains(&k))
}
