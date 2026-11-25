use regex::{Captures, Regex};

/// Returns the character immediately preceding byte offset `start`, if any.
#[inline]
pub(crate) fn prev_char(s: &str, start: usize) -> Option<char> {
    s[..start].chars().next_back()
}

/// Returns the character immediately following byte offset `end`, if any.
#[inline]
pub(crate) fn next_char(s: &str, end: usize) -> Option<char> {
    s[end..].chars().next()
}

/// JavaScript `\w` definition: ASCII `[A-Za-z0-9_]`.
#[inline]
pub(crate) fn is_js_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

/// Returns true if a match `[start..end]` starts on a JS `\b` word boundary.
#[inline]
pub(crate) fn has_js_word_boundary_start(s: &str, start: usize) -> bool {
    let first = match s[start..].chars().next() {
        Some(c) => c,
        None => return false,
    };
    let prev_is_word = prev_char(s, start).is_some_and(is_js_word_char);
    prev_is_word != is_js_word_char(first)
}

/// Returns true if a match `[start..end]` ends on a JS `\b` word boundary.
#[inline]
pub(crate) fn has_js_word_boundary_end(s: &str, start: usize, end: usize) -> bool {
    let last = match s[start..end].chars().next_back() {
        Some(c) => c,
        None => return false,
    };
    let next_is_word = next_char(s, end).is_some_and(is_js_word_char);
    is_js_word_char(last) != next_is_word
}

/// Returns true if a match `[start..end]` has JS `\b` word boundaries on both sides.
#[inline]
pub(crate) fn has_js_word_boundaries(s: &str, start: usize, end: usize) -> bool {
    has_js_word_boundary_start(s, start) && has_js_word_boundary_end(s, start, end)
}

/// Returns true if neither adjacent char is an ASCII digit (`(?<!\d)...(?!\d)`).
#[inline]
pub(crate) fn has_no_adjacent_ascii_digits(s: &str, start: usize, end: usize) -> bool {
    !prev_char(s, start).is_some_and(|c| c.is_ascii_digit())
        && !next_char(s, end).is_some_and(|c| c.is_ascii_digit())
}

/// Returns true if character is a modern Hangul syllable (`[가-힣]`).
#[inline]
pub(crate) fn is_hangul_syllable(ch: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&ch)
}

/// Emulates JavaScript `String.prototype.replace(regex, callback)` with a manual
/// boundary predicate (replacing JS lookarounds) so `regex` never needs `fancy-regex`.
///
/// When `boundary_ok(input, &caps)` is false, the match is rejected as a non-match
/// and scanning resumes at the next UTF-8 character after `m.start()`, preserving
/// exact JS RegExp `/g` advancement semantics.
pub(crate) fn replace_with_captures<F, B>(
    input: &str,
    re: &Regex,
    boundary_ok: B,
    mut replacer: F,
) -> String
where
    B: Fn(&str, &Captures<'_>) -> bool,
    F: FnMut(&str, &Captures<'_>) -> Option<String>,
{
    let mut out = String::with_capacity(input.len());
    let mut last_emitted = 0usize;
    let mut search_pos = 0usize;

    while search_pos <= input.len() {
        let Some(caps) = re.captures_at(input, search_pos) else {
            break;
        };
        let m = caps.get(0).expect("group 0 always present");

        if !boundary_ok(input, &caps) {
            // Advance past one character from m.start() to emulate JS engine backtracking/retry.
            let step = input[m.start()..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            search_pos = m.start() + step;
            continue;
        }

        out.push_str(&input[last_emitted..m.start()]);
        if let Some(replacement) = replacer(input, &caps) {
            out.push_str(&replacement);
        } else {
            out.push_str(m.as_str());
        }
        last_emitted = m.end();

        if m.end() > m.start() {
            search_pos = m.end();
        } else {
            let step = input[m.start()..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            search_pos = m.start() + step;
        }
    }

    out.push_str(&input[last_emitted..]);
    out
}
