use crate::frontmatter::parse_memory_sections;
use crate::types::{SectionDiff, SectionDiffOp, SectionDiffToken, FIXED_MEMORY_SECTIONS};

/// Computes a compact character-level LCS diff with run coalescing, requiring no
/// external diff crate. Coalesces consecutive equal/delete/insert ops into tokens
/// compatible with `SectionDiffToken { op: SectionDiffOp, text }`.
pub fn diff_text_tokens(before: &str, after: &str) -> Vec<SectionDiffToken> {
    if before == after {
        if before.is_empty() {
            return Vec::new();
        }
        return vec![SectionDiffToken {
            op: SectionDiffOp::Equal,
            text: before.to_string(),
        }];
    }

    let a: Vec<char> = before.chars().collect();
    let b: Vec<char> = after.chars().collect();

    let mut prefix = 0usize;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix < (a.len() - prefix)
        && suffix < (b.len() - prefix)
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut tokens: Vec<SectionDiffToken> = Vec::new();
    if prefix > 0 {
        tokens.push(SectionDiffToken {
            op: SectionDiffOp::Equal,
            text: a[..prefix].iter().collect(),
        });
    }

    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];

    if !a_mid.is_empty() || !b_mid.is_empty() {
        if a_mid.len() * b_mid.len() > 250_000 {
            if !a_mid.is_empty() {
                tokens.push(SectionDiffToken {
                    op: SectionDiffOp::Delete,
                    text: a_mid.iter().collect(),
                });
            }
            if !b_mid.is_empty() {
                tokens.push(SectionDiffToken {
                    op: SectionDiffOp::Insert,
                    text: b_mid.iter().collect(),
                });
            }
        } else {
            let m = a_mid.len();
            let n = b_mid.len();
            let mut dp = vec![vec![0u32; n + 1]; m + 1];
            for i in (0..m).rev() {
                for j in (0..n).rev() {
                    if a_mid[i] == b_mid[j] {
                        dp[i][j] = dp[i + 1][j + 1] + 1;
                    } else {
                        dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
                    }
                }
            }

            let mut i = 0usize;
            let mut j = 0usize;
            while i < m && j < n {
                if a_mid[i] == b_mid[j] {
                    push_token(&mut tokens, SectionDiffOp::Equal, a_mid[i]);
                    i += 1;
                    j += 1;
                } else if dp[i + 1][j] >= dp[i][j + 1] {
                    push_token(&mut tokens, SectionDiffOp::Delete, a_mid[i]);
                    i += 1;
                } else {
                    push_token(&mut tokens, SectionDiffOp::Insert, b_mid[j]);
                    j += 1;
                }
            }
            while i < m {
                push_token(&mut tokens, SectionDiffOp::Delete, a_mid[i]);
                i += 1;
            }
            while j < n {
                push_token(&mut tokens, SectionDiffOp::Insert, b_mid[j]);
                j += 1;
            }
        }
    }

    if suffix > 0 {
        let s: String = a[a.len() - suffix..].iter().collect();
        if let Some(last) = tokens.last_mut().filter(|t| t.op == SectionDiffOp::Equal) {
            last.text.push_str(&s);
        } else {
            tokens.push(SectionDiffToken {
                op: SectionDiffOp::Equal,
                text: s,
            });
        }
    }

    tokens
}

fn push_token(tokens: &mut Vec<SectionDiffToken>, op: SectionDiffOp, ch: char) {
    if let Some(last) = tokens.last_mut().filter(|t| t.op == op) {
        last.text.push(ch);
    } else {
        tokens.push(SectionDiffToken {
            op,
            text: ch.to_string(),
        });
    }
}

pub fn compute_section_diffs(before_body: &str, after_body: &str) -> Vec<SectionDiff> {
    let before_sections = parse_memory_sections(before_body);
    let after_sections = parse_memory_sections(after_body);

    FIXED_MEMORY_SECTIONS
        .iter()
        .map(|section_title| {
            let before_content = before_sections[*section_title].clone();
            let after_content = after_sections[*section_title].clone();
            let tokens = diff_text_tokens(&before_content, &after_content);
            let changed = before_content.trim() != after_content.trim();

            SectionDiff {
                section_title: (*section_title).to_string(),
                before_content,
                after_content,
                changed,
                tokens,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_tokens_reconstruct_before_and_after() {
        let before = "- 표준 공문서체";
        let after = "- 간결한 개조식 공문서체";
        let tokens = diff_text_tokens(before, after);

        let reconstructed_before: String = tokens
            .iter()
            .filter(|t| t.op != SectionDiffOp::Insert)
            .map(|t| t.text.as_str())
            .collect();
        let reconstructed_after: String = tokens
            .iter()
            .filter(|t| t.op != SectionDiffOp::Delete)
            .map(|t| t.text.as_str())
            .collect();

        assert_eq!(reconstructed_before, before);
        assert_eq!(reconstructed_after, after);
        assert!(tokens.iter().any(|t| t.op == SectionDiffOp::Insert));
    }
}
