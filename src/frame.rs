use std::fmt::Write as _;

use crate::filter::Level;
use crate::highlight::{SpanKind, highlight};
use crate::view::Line;

const ANSI_CLEAR_HOME: &str = "\x1b[H\x1b[2J";
const ANSI_RESET: &str = "\x1b[0m";

/// Metadata displayed in the bottom status bar of a rendered terminal frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Status {
    /// Byte offset of the top visible line.
    pub top_offset: u64,
    /// Total byte length of the log source.
    pub total_bytes: u64,
    /// Whether follow mode is currently active.
    pub follow: bool,
}

/// Renders a single log line with ANSI SGR color sequences for highlighted spans.
#[must_use]
pub fn render_highlighted_line(text: &str) -> String {
    render_line_truncated(text, usize::MAX)
}

fn sgr_for_span(kind: SpanKind) -> Option<&'static str> {
    match kind {
        SpanKind::Timestamp => Some("\x1b[36m"),
        SpanKind::Level(Level::Error | Level::Fatal) => Some("\x1b[31m"),
        SpanKind::Level(Level::Warn) => Some("\x1b[33m"),
        SpanKind::Quoted => Some("\x1b[32m"),
        SpanKind::Level(Level::Trace | Level::Debug | Level::Info) | SpanKind::Number => None,
    }
}

fn render_line_truncated(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total_chars = text.chars().count();
    let (visible_text, needs_ellipsis) = if total_chars > max_chars {
        let take_chars = max_chars.saturating_sub(1);
        let end_byte = text
            .char_indices()
            .nth(take_chars)
            .map_or(text.len(), |(idx, _)| idx);
        (&text[..end_byte], true)
    } else {
        (text, false)
    };

    let spans = highlight(text);
    let vis_len = visible_text.len();
    let mut out = String::with_capacity(vis_len + 32);
    let mut cursor = 0;

    for span in spans {
        if span.start >= vis_len {
            break;
        }
        let clamped_end = span.end.min(vis_len);
        if span.start > cursor {
            out.push_str(&visible_text[cursor..span.start]);
        }
        if let Some(sgr) = sgr_for_span(span.kind) {
            out.push_str(sgr);
            out.push_str(&visible_text[span.start..clamped_end]);
            out.push_str(ANSI_RESET);
        } else {
            out.push_str(&visible_text[span.start..clamped_end]);
        }
        cursor = clamped_end;
    }
    if cursor < vis_len {
        out.push_str(&visible_text[cursor..vis_len]);
    }
    if needs_ellipsis {
        out.push('\u{2026}');
    }
    out
}

/// Renders a terminal frame of `width` columns by `height` rows using ECMA-48 ANSI CSI sequences.
///
/// - Starts with `\x1b[H\x1b[2J` (cursor home and clear screen).
/// - Renders up to `height.saturating_sub(1)` log lines, truncating lines longer than `width`
///   characters to `width - 1` characters followed by `…` (`U+2026`).
/// - Applies SGR color sequences (`\x1b[31m` error, `\x1b[33m` warn, `\x1b[36m` timestamp,
///   `\x1b[32m` quoted, `\x1b[0m` reset).
/// - Ends with a status row displaying the top offset percentage and follow state.
#[must_use]
pub fn render_frame(lines: &[Line], width: u16, height: u16, status: &Status) -> String {
    let w = usize::from(width);
    let h = usize::from(height);
    let mut out = String::from(ANSI_CLEAR_HOME);
    if h == 0 {
        return out;
    }

    let content_rows = h.saturating_sub(1);
    for row in 0..content_rows {
        if let Some(line) = lines.get(row) {
            out.push_str(&render_line_truncated(&line.text, w));
        }
        out.push('\n');
    }

    let pct = if status.total_bytes == 0 {
        100_u64
    } else {
        status
            .top_offset
            .saturating_mul(100)
            .checked_div(status.total_bytes)
            .unwrap_or(0)
            .min(100)
    };
    let follow_label = if status.follow { "FOLLOW" } else { "PAUSED" };
    let mut status_line = String::new();
    let _ = write!(
        status_line,
        "{pct}% ({}/{}) [{follow_label}]",
        status.top_offset, status.total_bytes
    );
    if w > 0 && status_line.chars().count() > w {
        status_line = status_line.chars().take(w).collect();
    }
    out.push_str(&status_line);
    out
}
