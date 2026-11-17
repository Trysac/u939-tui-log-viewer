use std::fmt;
use std::str::FromStr;

use crate::error::ViewError;
use crate::view::Line;

const MAX_FILTER_ARGS: usize = 1024;

/// Severity level of a log entry, ordered from lowest ([`Level::Trace`]) to highest ([`Level::Fatal`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Fine-grained tracing entry.
    Trace,
    /// Diagnostic debugging entry.
    Debug,
    /// Normal informational entry.
    Info,
    /// Warning condition.
    Warn,
    /// Error condition.
    Error,
    /// Fatal / unrecoverable condition.
    Fatal,
}

impl Level {
    /// Parses a level name case-insensitively (`TRACE`, `DEBUG`, `INFO`, `WARN`, `WARNING`, `ERROR`, `FATAL`).
    #[must_use]
    pub fn parse_token(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("trace") {
            Some(Self::Trace)
        } else if s.eq_ignore_ascii_case("debug") {
            Some(Self::Debug)
        } else if s.eq_ignore_ascii_case("info") {
            Some(Self::Info)
        } else if s.eq_ignore_ascii_case("warn") || s.eq_ignore_ascii_case("warning") {
            Some(Self::Warn)
        } else if s.eq_ignore_ascii_case("error") {
            Some(Self::Error)
        } else if s.eq_ignore_ascii_case("fatal") {
            Some(Self::Fatal)
        } else {
            None
        }
    }

    /// Canonical uppercase name of the level.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Level {
    type Err = ViewError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_token(s)
            .ok_or_else(|| ViewError::invalid_filter(format!("unknown log level: {s}")))
    }
}

/// Detects the [`Level`] of a log line.
///
/// Checks in order:
/// 1. The first standalone uppercase/lowercase level token (`TRACE`, `DEBUG`, `INFO`, `WARN`, `WARNING`, `ERROR`, `FATAL`).
/// 2. Logfmt key-value pair `level=<value>` or `level="<value>"`.
/// 3. JSON property `"level":"<value>"` (with optional whitespace around `:`).
#[must_use]
pub fn detect_level(text: &str) -> Option<Level> {
    if let Some((_, _, level)) = find_first_standalone_level_token(text) {
        return Some(level);
    }
    if let Some((_, _, level)) = find_logfmt_level(text) {
        return Some(level);
    }
    if let Some((_, _, level)) = find_json_level(text) {
        return Some(level);
    }
    None
}

pub(crate) fn find_first_standalone_level_token(text: &str) -> Option<(usize, usize, Level)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let end = i;
            let prev_ok = start == 0 || is_token_boundary_before(bytes[start - 1]);
            let next_ok = end == bytes.len() || is_token_boundary_after(bytes[end]);
            if prev_ok
                && next_ok
                && let Some(level) = Level::parse_token(&text[start..end])
            {
                return Some((start, end, level));
            }
        } else {
            i += 1;
        }
    }
    None
}

fn is_token_boundary_before(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'[' | b'(' | b'<' | b'|' | b':')
}

fn is_token_boundary_after(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b']' | b')' | b'>' | b'|' | b':' | b',' | b';')
}

pub(crate) fn find_logfmt_level(text: &str) -> Option<(usize, usize, Level)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        let boundary_ok = i == 0 || bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b',';
        if boundary_ok
            && text[i..].len() >= 6
            && text.as_bytes()[i..i + 6].eq_ignore_ascii_case(b"level=")
        {
            let mut val_start = i + 6;
            let quoted = val_start < bytes.len() && bytes[val_start] == b'"';
            if quoted {
                val_start += 1;
            }
            let mut val_end = val_start;
            while val_end < bytes.len() && bytes[val_end].is_ascii_alphabetic() {
                val_end += 1;
            }
            let after_ok = if quoted {
                val_end < bytes.len() && bytes[val_end] == b'"'
            } else {
                val_end == bytes.len()
                    || bytes[val_end].is_ascii_whitespace()
                    || matches!(bytes[val_end], b',' | b';' | b']' | b')')
            };
            if after_ok
                && val_end > val_start
                && let Some(level) = Level::parse_token(&text[val_start..val_end])
            {
                return Some((val_start, val_end, level));
            }
        }
        i += 1;
    }
    None
}

pub(crate) fn find_json_level(text: &str) -> Option<(usize, usize, Level)> {
    let bytes = text.as_bytes();
    let pat = b"\"level\"";
    let mut i = 0;
    while i + pat.len() < bytes.len() {
        if bytes[i..i + pat.len()].eq_ignore_ascii_case(pat) {
            let mut j = i + pat.len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b':' {
                j += 1;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'"' {
                    let val_start = j + 1;
                    let mut val_end = val_start;
                    while val_end < bytes.len() && bytes[val_end].is_ascii_alphabetic() {
                        val_end += 1;
                    }
                    if val_end < bytes.len()
                        && bytes[val_end] == b'"'
                        && val_end > val_start
                        && let Some(level) = Level::parse_token(&text[val_start..val_end])
                    {
                        return Some((val_start, val_end, level));
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Predicate for matching [`Line`] items by required substrings, excluded substrings, and minimum [`Level`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    grep: Vec<String>,
    exclude: Vec<String>,
    min_level: Option<Level>,
}

impl Filter {
    /// Creates an empty filter that matches every line.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses filter flags from command-line style arguments.
    ///
    /// Supported flags:
    /// - `--grep <substring>` (may be repeated; all must match)
    /// - `--exclude <substring>` (may be repeated; none may match)
    /// - `--level <level>` (minimum severity level: `trace`, `debug`, `info`, `warn`, `error`, `fatal`)
    ///
    /// # Errors
    ///
    /// Returns [`ViewError::InvalidFilter`] (code `"invalid_filter"`) on unknown flags, missing flag
    /// values, empty patterns, or unrecognized level names.
    pub fn parse<S: AsRef<str>>(args: &[S]) -> Result<Self, ViewError> {
        if args.len() > MAX_FILTER_ARGS {
            return Err(ViewError::invalid_filter("too many filter arguments"));
        }
        let mut filter = Self::new();
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_ref();
            match flag {
                "--grep" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or_else(|| ViewError::invalid_filter("missing value for --grep"))?
                        .as_ref();
                    if val.is_empty() {
                        return Err(ViewError::invalid_filter("--grep pattern cannot be empty"));
                    }
                    filter.grep.push(val.to_string());
                }
                "--exclude" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or_else(|| ViewError::invalid_filter("missing value for --exclude"))?
                        .as_ref();
                    if val.is_empty() {
                        return Err(ViewError::invalid_filter(
                            "--exclude pattern cannot be empty",
                        ));
                    }
                    filter.exclude.push(val.to_string());
                }
                "--level" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or_else(|| ViewError::invalid_filter("missing value for --level"))?
                        .as_ref();
                    let level = Level::from_str(val)?;
                    filter.min_level = Some(level);
                }
                other => {
                    return Err(ViewError::invalid_filter(format!(
                        "unknown filter flag: {other}"
                    )));
                }
            }
            i += 1;
        }
        Ok(filter)
    }

    /// Adds a required substring to the filter.
    #[must_use]
    pub fn with_grep(mut self, pattern: impl Into<String>) -> Self {
        self.grep.push(pattern.into());
        self
    }

    /// Adds an excluded substring to the filter.
    #[must_use]
    pub fn with_exclude(mut self, pattern: impl Into<String>) -> Self {
        self.exclude.push(pattern.into());
        self
    }

    /// Sets the minimum severity [`Level`] required to match.
    #[must_use]
    pub fn with_level(mut self, level: Level) -> Self {
        self.min_level = Some(level);
        self
    }

    /// Returns `true` if `line` satisfies all `--grep`, `--exclude`, and `--level` constraints.
    #[must_use]
    pub fn matches(&self, line: &Line) -> bool {
        if let Some(min_lvl) = self.min_level {
            let line_lvl = line.level.or_else(|| detect_level(&line.text));
            match line_lvl {
                Some(lvl) if lvl >= min_lvl => {}
                _ => return false,
            }
        }
        for pat in &self.grep {
            if !line.text.contains(pat.as_str()) {
                return false;
            }
        }
        for pat in &self.exclude {
            if line.text.contains(pat.as_str()) {
                return false;
            }
        }
        true
    }
}
