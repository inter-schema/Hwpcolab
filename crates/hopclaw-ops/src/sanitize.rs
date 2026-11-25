use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};

static RE_BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^\n\r]*?)\*\*").unwrap());
static RE_ITALIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*([^\n\r]*?)\*").unwrap());

/// Removes Markdown emphasis markers (`**bold**`, `*italic*`) from document text.
pub fn sanitize_markdown_formatting(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let step1 = RE_BOLD.replace_all(text, "$1");
    RE_ITALIC.replace_all(&step1, "$1").into_owned()
}

pub(crate) fn sanitize_opt_string(opt: &Option<String>) -> Option<String> {
    opt.as_ref().map(|s| sanitize_markdown_formatting(s))
}

pub(crate) fn sanitize_vec_string(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|s| sanitize_markdown_formatting(s))
        .collect()
}

pub(crate) fn sanitize_matrix_string(rows: &[Vec<String>]) -> Vec<Vec<String>> {
    rows.iter().map(|r| sanitize_vec_string(r)).collect()
}

/// Converts a finite `f64` to a JSON integer when whole (matching JS `JSON.stringify`),
/// or JSON float otherwise.
pub(crate) fn json_number(v: f64) -> Value {
    if v.is_finite() && v.fract() == 0.0 && v >= (i64::MIN as f64) && v <= (i64::MAX as f64) {
        json!(v as i64)
    } else {
        json!(v)
    }
}
