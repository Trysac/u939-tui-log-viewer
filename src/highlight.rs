use crate::filter::{Level, find_first_standalone_level_token, find_json_level, find_logfmt_level};

/// Classification of a highlighted span within a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// An RFC 3339 timestamp (`YYYY-MM-DDTHH:MM:SS` with optional fractional seconds and timezone).
    Timestamp,
    /// A detected severity level token.
    Level(Level),
    /// A standalone integer or decimal literal.
    Number,
    /// A double-quoted string literal (including surrounding quotes and escaped characters).
    Quoted,
}

/// Half-open byte range `[start, end)` within a line and its [`SpanKind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Inclusive starting byte offset.
    pub start: usize,
    /// Exclusive ending byte offset.
    pub end: usize,
    /// Syntax category of the span.
    pub kind: SpanKind,
}

/// Scans `text` and returns sorted, non-overlapping syntax [`Span`] items.
///
/// Recognizes:
/// - RFC 3339 timestamps (`YYYY-MM-DDTHH:MM:SS` with optional `.fraction` and `Z` or `±HH:MM`)
/// - Log level tokens (`TRACE`, `DEBUG`, `INFO`, `WARN`, `WARNING`, `ERROR`, `FATAL`)
/// - Double-quoted string literals with `\` escapes
/// - Standalone integer and decimal numbers (not embedded inside words; numbers after `=` count)
#[must_use]
pub fn highlight(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans: Vec<Span> = Vec::new();

    // 1. Collect double-quoted strings first so tokens inside quotes do not conflict,
    //    except when the line's level is inside logfmt/JSON quotes.
    let level_span = detect_level_span(text);

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    spans.push(Span {
                        start,
                        end: i,
                        kind: SpanKind::Quoted,
                    });
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }

    // 2. Collect RFC 3339 timestamps outside existing spans.
    let mut idx = 0;
    while idx + 19 <= bytes.len() {
        if let Some(end) = match_rfc3339(bytes, idx) {
            let candidate = Span {
                start: idx,
                end,
                kind: SpanKind::Timestamp,
            };
            if !overlaps_any(&spans, &candidate) {
                spans.push(candidate);
                idx = end;
                continue;
            }
        }
        idx += 1;
    }

    // 3. Add detected level token if non-overlapping.
    if let Some((l_start, l_end, lvl)) = level_span {
        let candidate = Span {
            start: l_start,
            end: l_end,
            kind: SpanKind::Level(lvl),
        };
        if !overlaps_any(&spans, &candidate) {
            spans.push(candidate);
        }
    }

    // 4. Collect standalone numbers (integers and decimals) outside existing spans.
    let mut n = 0;
    while n < bytes.len() {
        if is_number_start(bytes, n) {
            let start = n;
            if bytes[n] == b'-' {
                n += 1;
            }
            while n < bytes.len() && bytes[n].is_ascii_digit() {
                n += 1;
            }
            if n + 1 < bytes.len() && bytes[n] == b'.' && bytes[n + 1].is_ascii_digit() {
                n += 1;
                while n < bytes.len() && bytes[n].is_ascii_digit() {
                    n += 1;
                }
            }
            let end = n;
            if is_number_end_ok(bytes, end) {
                let candidate = Span {
                    start,
                    end,
                    kind: SpanKind::Number,
                };
                if !overlaps_any(&spans, &candidate) {
                    spans.push(candidate);
                }
            }
        } else {
            n += 1;
        }
    }

    spans.sort_by_key(|s| (s.start, s.end));
    spans
}

fn detect_level_span(text: &str) -> Option<(usize, usize, Level)> {
    if let Some(found) = find_first_standalone_level_token(text) {
        return Some(found);
    }
    if let Some(found) = find_logfmt_level(text) {
        return Some(found);
    }
    find_json_level(text)
}

fn overlaps_any(spans: &[Span], candidate: &Span) -> bool {
    spans
        .iter()
        .any(|s| candidate.start < s.end && s.start < candidate.end)
}

fn is_number_start(bytes: &[u8], idx: usize) -> bool {
    let prev_ok = idx == 0 || is_num_boundary_before(bytes[idx - 1]);
    if !prev_ok {
        return false;
    }
    if bytes[idx].is_ascii_digit() {
        true
    } else if bytes[idx] == b'-' {
        idx + 1 < bytes.len() && bytes[idx + 1].is_ascii_digit()
    } else {
        false
    }
}

fn is_num_boundary_before(b: u8) -> bool {
    !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}

fn is_number_end_ok(bytes: &[u8], end: usize) -> bool {
    if end >= bytes.len() {
        return true;
    }
    let b = bytes[end];
    !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}

fn match_rfc3339(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        return None;
    }
    if start + 19 > bytes.len() {
        return None;
    }
    // YYYY-MM-DDTHH:MM:SS
    let d = &bytes[start..];
    let digits_at = |pos: usize, len: usize| -> bool {
        d.len() >= pos + len && d[pos..pos + len].iter().all(u8::is_ascii_digit)
    };
    if !digits_at(0, 4)
        || d[4] != b'-'
        || !digits_at(5, 2)
        || d[7] != b'-'
        || !digits_at(8, 2)
        || (d[10] != b'T' && d[10] != b't')
        || !digits_at(11, 2)
        || d[13] != b':'
        || !digits_at(14, 2)
        || d[16] != b':'
        || !digits_at(17, 2)
    {
        return None;
    }

    let mut i = start + 19;
    // Optional fractional seconds
    if i < bytes.len() && bytes[i] == b'.' {
        let frac_start = i + 1;
        let mut frac_end = frac_start;
        while frac_end < bytes.len() && bytes[frac_end].is_ascii_digit() {
            frac_end += 1;
        }
        if frac_end == frac_start {
            return None;
        }
        i = frac_end;
    }

    // Mandatory Z or +/-HH:MM timezone
    if i < bytes.len() && (bytes[i] == b'Z' || bytes[i] == b'z') {
        i += 1;
    } else if i + 6 <= bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        if bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3] == b':'
            && bytes[i + 4].is_ascii_digit()
            && bytes[i + 5].is_ascii_digit()
        {
            i += 6;
        } else {
            return None;
        }
    } else {
        return None;
    }

    if i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        return None;
    }
    Some(i)
}
