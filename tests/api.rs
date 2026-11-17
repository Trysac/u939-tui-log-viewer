//! Integration and property tests for `tui_log_viewer`.

use std::fmt::Write as _;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use proptest::prelude::*;

use tui_log_viewer::{
    Action, Change, FileSource, Filter, Key, Level, Line, LogView, MemSource, Source, Span,
    SpanKind, Status, ViewError, ViewOptions, Viewer, cli, detect_level, highlight, render_frame,
};

const SYNTHETIC_TOTAL_BYTES: u64 = 5_368_709_120;
const SYNTHETIC_LINE_LEN: u64 = 64;

/// Synthetic 5 GiB source where line `i` is `INFO request ` + 50-digit zero-padded `i` + `\n` (64 bytes).
struct Synthetic5GiBSource {
    total_len: u64,
    bytes_read: Arc<AtomicU64>,
}

impl Synthetic5GiBSource {
    fn new(total_len: u64) -> (Self, Arc<AtomicU64>) {
        let counter = Arc::new(AtomicU64::new(0));
        (
            Self {
                total_len,
                bytes_read: Arc::clone(&counter),
            },
            counter,
        )
    }

    fn render_line(line_idx: u64) -> [u8; 64] {
        let mut s = String::with_capacity(64);
        let _ = writeln!(s, "INFO request {line_idx:050}");
        debug_assert_eq!(s.len(), 64);
        let mut arr = [0_u8; 64];
        arr.copy_from_slice(s.as_bytes());
        arr
    }
}

impl Source for Synthetic5GiBSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.total_len)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.total_len || buf.is_empty() {
            return Ok(0);
        }
        let avail = (self.total_len - offset).min(buf.len() as u64);
        let count = usize::try_from(avail).unwrap_or(0);
        let mut written = 0;
        while written < count {
            let cur_off = offset + (written as u64);
            let line_idx = cur_off / SYNTHETIC_LINE_LEN;
            let byte_in_line = usize::try_from(cur_off % SYNTHETIC_LINE_LEN).unwrap_or(0);
            let line_bytes = Self::render_line(line_idx);
            let take = (64 - byte_in_line).min(count - written);
            buf[written..written + take]
                .copy_from_slice(&line_bytes[byte_in_line..byte_in_line + take]);
            written += take;
        }
        self.bytes_read.fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }
}

#[test]
fn synthetic_5gib_tail_bounds_and_offsets() {
    let (src, counter) = Synthetic5GiBSource::new(5_368_709_120);
    let mut view = LogView::open(src, ViewOptions::default()).expect("open 5 GiB source");

    let last_one = view.tail(1).expect("tail(1)");
    assert_eq!(last_one.len(), 1);
    assert_eq!(last_one[0].offset, 5_368_709_056);
    assert_eq!(last_one[0].level, Some(Level::Info));
    assert!(!last_one[0].truncated);

    // Reset counter to measure open + tail(50) specifically
    let (src2, counter2) = Synthetic5GiBSource::new(5_368_709_120);
    let mut view2 = LogView::open(src2, ViewOptions::default()).expect("open 5 GiB source");
    let tail_50 = view2.tail(50).expect("tail(50)");
    assert_eq!(tail_50.len(), 50);
    for pair in tail_50.windows(2) {
        assert_eq!(pair[0].offset + 64, pair[1].offset);
    }
    assert_eq!(tail_50.last().unwrap().offset, 5_368_709_056);
    assert!(counter.load(Ordering::Relaxed) <= 1_048_576);
    assert!(counter2.load(Ordering::Relaxed) <= 1_048_576);
}

#[test]
fn synthetic_5gib_lines_from_offset() {
    let (src, counter) = Synthetic5GiBSource::new(SYNTHETIC_TOTAL_BYTES);
    let mut view = LogView::open(src, ViewOptions::default()).expect("open");

    let lines = view
        .lines_from_offset(2_684_354_590, 20)
        .expect("lines_from_offset mid");
    assert_eq!(lines.len(), 20);
    assert_eq!(lines[0].offset, 2_684_354_624);
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(line.offset, 2_684_354_624 + (i as u64) * 64);
    }
    assert!(counter.load(Ordering::Relaxed) <= 1_048_576);

    let from_start = view.lines_from_offset(0, 3).expect("lines_from_offset 0");
    let offsets: Vec<u64> = from_start.iter().map(|l| l.offset).collect();
    assert_eq!(offsets, vec![0, 64, 128]);
}

#[test]
fn filter_parsing_and_matching_levels_and_substrings() {
    let filter = Filter::parse(&["--grep", "timeout", "--level", "warn"]).expect("valid filter");
    let l1 = Line::new(0, "2026-11-17T10:00:00Z ERROR db timeout", false);
    let l2 = Line::new(40, "2026-11-17T10:00:01Z INFO timeout ok", false);
    let l3 = Line::new(80, "2026-11-17T10:00:02Z WARN slow", false);

    assert!(filter.matches(&l1));
    assert!(!filter.matches(&l2));
    assert!(!filter.matches(&l3));

    // Logfmt and JSON level detection
    let logfmt_line = Line::new(0, "ts=1 level=warn msg=timeout", false);
    assert_eq!(logfmt_line.level, Some(Level::Warn));
    assert!(filter.matches(&logfmt_line));

    let json_line = Line::new(0, r#"{"level":"warn","msg":"timeout"}"#, false);
    assert_eq!(json_line.level, Some(Level::Warn));
    assert!(filter.matches(&json_line));

    // Exclude flag
    let filter_excl = Filter::parse(&[
        "--grep",
        "timeout",
        "--exclude",
        "ignore",
        "--level",
        "info",
    ])
    .expect("valid exclude filter");
    assert!(filter_excl.matches(&l2));
    assert!(!filter_excl.matches(&Line::new(0, "INFO timeout ignore", false)));

    // Invalid filter level
    let _ = "--level nope";
    let err = Filter::parse(&["--level", "nope"]).expect_err("invalid level must fail");
    assert_eq!(err.code(), "invalid_filter");

    // Adversarial neighbours for level tokens
    assert_eq!(detect_level("INFORMATIONAL message"), None);
    assert_eq!(detect_level("MYERROR token"), None);
    assert_eq!(detect_level("[WARNING] disk full"), Some(Level::Warn));
    assert_eq!(detect_level("FATAL: crash"), Some(Level::Fatal));
    assert_eq!(detect_level("TRACE step 1"), Some(Level::Trace));
    assert_eq!(detect_level("DEBUG step 2"), Some(Level::Debug));
}

#[test]
fn highlight_spans_exact_and_adversarial() {
    let input = "2026-11-17T10:00:00Z ERROR code=42 \"bad input\"";
    let spans = highlight(input);
    assert_eq!(
        spans,
        vec![
            Span {
                start: 0,
                end: 20,
                kind: SpanKind::Timestamp
            },
            Span {
                start: 21,
                end: 26,
                kind: SpanKind::Level(Level::Error)
            },
            Span {
                start: 32,
                end: 34,
                kind: SpanKind::Number
            },
            Span {
                start: 35,
                end: 46,
                kind: SpanKind::Quoted
            },
        ]
    );

    // Fractional timestamp with offset, decimal number, escaped quote, and identifier containing digits
    let complex = r#"2026-11-17T10:00:00.123+02:00 WARN req_id=v123 latency=-12.5 "say \"hi\"""#;
    let c_spans = highlight(complex);
    assert_eq!(c_spans[0].kind, SpanKind::Timestamp);
    assert_eq!(
        &complex[c_spans[0].start..c_spans[0].end],
        "2026-11-17T10:00:00.123+02:00"
    );
    assert_eq!(c_spans[1].kind, SpanKind::Level(Level::Warn));
    assert_eq!(c_spans[2].kind, SpanKind::Number);
    assert_eq!(&complex[c_spans[2].start..c_spans[2].end], "-12.5");
    assert_eq!(c_spans[3].kind, SpanKind::Quoted);
    assert_eq!(
        &complex[c_spans[3].start..c_spans[3].end],
        r#""say \"hi\"""#
    );
}

#[test]
fn render_frame_ansi_truncation_and_status() {
    let lines = vec![
        Line::new(50, "ERROR fail", false),
        Line::new(60, "012345678901234567890123456789", false),
    ];
    let status = Status {
        top_offset: 50,
        total_bytes: 100,
        follow: true,
    };
    let frame = render_frame(&lines, 20, 3, &status);
    assert!(frame.starts_with("\x1b[H\x1b[2J"));
    assert!(frame.contains("\x1b[31mERROR\x1b[0m fail"));
    assert!(frame.contains("0123456789012345678\u{2026}"));
    let last_row = frame.lines().last().expect("status row");
    assert!(last_row.contains("50%"));
    assert!(last_row.contains("FOLLOW"));
}

#[test]
fn follow_growth_partial_line_and_truncation() {
    let mem = MemSource::new(b"a\nb\n".to_vec());
    let mut view = LogView::open(mem.clone(), ViewOptions::default()).expect("open mem");
    assert_eq!(view.refresh().expect("unchanged"), Change::Unchanged);

    mem.append(b"c\n");
    assert_eq!(view.refresh().expect("grown"), Change::Grown(2));
    let new_lines: Vec<String> = view.new_lines().into_iter().map(|l| l.text).collect();
    assert_eq!(new_lines, vec!["c".to_string()]);

    // Partial line "d" held until newline arrives
    mem.append(b"d");
    assert_eq!(view.refresh().expect("grown partial"), Change::Grown(1));
    assert_eq!(view.new_lines(), []);

    mem.append(b"\n");
    assert_eq!(view.refresh().expect("completed d"), Change::Grown(1));
    let completed: Vec<String> = view.new_lines().into_iter().map(|l| l.text).collect();
    assert_eq!(completed, vec!["d".to_string()]);

    // Truncate and replace with "x\n"
    mem.set(b"x\n");
    assert_eq!(view.refresh().expect("truncated"), Change::Truncated);
    let after_trunc: Vec<String> = view.new_lines().into_iter().map(|l| l.text).collect();
    assert_eq!(after_trunc, vec!["x".to_string()]);
}

#[test]
fn long_line_truncation_invalid_utf8_and_cli() {
    // Line longer than 65_536 bytes
    let mut raw = vec![b'A'; 70_000];
    raw.push(b'\n');
    let mem = MemSource::new(raw);
    let mut view = LogView::open(mem, ViewOptions::default()).expect("open");
    let tailed = view.tail(1).expect("tail long line");
    assert_eq!(tailed.len(), 1);
    assert_eq!(tailed[0].text.len(), 65_536);
    assert!(tailed[0].truncated);

    let paged = view.lines_from_offset(0, 1).expect("page long line");
    assert_eq!(paged.len(), 1);
    assert_eq!(paged[0].text.len(), 65_536);
    assert!(paged[0].truncated);

    // Invalid UTF-8 becomes U+FFFD
    let bad_utf8 = MemSource::new(vec![b'h', b'i', 0xFF, b'\n']);
    let mut bad_view = LogView::open(bad_utf8, ViewOptions::default()).expect("open bad utf8");
    let bad_lines = bad_view.tail(1).expect("tail bad utf8");
    assert_eq!(bad_lines[0].text, "hi\u{FFFD}");

    // CLI tests using a temporary file
    let tmp_dir = std::env::temp_dir();
    let log_path = tmp_dir.join(format!("tui_log_viewer_test_{}.log", std::process::id()));
    std::fs::write(&log_path, b"INFO first\nWARN second\nERROR third\n").expect("write temp log");
    let path_str = log_path.to_str().unwrap().to_string();

    // Also verify from the temp directory so literal ["tail", "-n", "2", "app.log"] works
    let app_log_path = tmp_dir.join("app.log");
    std::fs::write(&app_log_path, b"line1\nline2\nline3\n").expect("write app.log");
    let prev_dir = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&tmp_dir).expect("chdir temp");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = cli::run(&["tail", "-n", "2", "app.log"], &mut out, &mut err);
    std::env::set_current_dir(prev_dir).expect("restore cwd");
    let _ = std::fs::remove_file(&app_log_path);
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8(out).unwrap(), "line2\nline3\n");

    // CLI with --color and --level
    let mut out2 = Vec::new();
    let mut err2 = Vec::new();
    let code2 = cli::run(
        &["tail", "-n", "2", "--level", "error", "--color", &path_str],
        &mut out2,
        &mut err2,
    );
    assert_eq!(code2, 0);
    let out2_str = String::from_utf8(out2).unwrap();
    assert!(out2_str.contains("\x1b[31mERROR\x1b[0m third"));

    // CLI page subcommand
    let mut out_page = Vec::new();
    let mut err_page = Vec::new();
    let code_page = cli::run(
        &["page", &path_str, "--offset", "0", "-n", "2"],
        &mut out_page,
        &mut err_page,
    );
    assert_eq!(code_page, 0);
    assert_eq!(
        String::from_utf8(out_page).unwrap(),
        "INFO first\nWARN second\n"
    );

    // FileSource direct check & cleanup
    let fs_src = FileSource::open(&log_path).expect("open FileSource");
    assert!(!fs_src.is_empty().expect("is_empty"));
    let _ = std::fs::remove_file(&log_path);

    // Missing file returns 2
    let mut out_missing = Vec::new();
    let mut err_missing = Vec::new();
    let missing_code = cli::run(
        &["tail", "-n", "2", "definitely_missing_file_939.log"],
        &mut out_missing,
        &mut err_missing,
    );
    assert_eq!(missing_code, 2);
}

#[test]
fn search_forward_and_viewer_state_machine() {
    let data = b"INFO start\nDEBUG middle\nERROR target hit\nINFO end\n";
    let mem = MemSource::new(data.to_vec());
    let mut view = LogView::open(mem, ViewOptions::default()).expect("open");
    let filter = Filter::new().with_level(Level::Error).with_grep("target");
    let found = view
        .search_forward(0, &filter)
        .expect("search_forward")
        .expect("matching line");
    assert_eq!(found.text, "ERROR target hit");
    assert_eq!(found.offset, 24);

    let none = view
        .search_forward(30, &filter)
        .expect("search_forward past match");
    assert!(none.is_none());

    assert_eq!(
        view.search_forward(999, &filter)
            .expect_err("out of range")
            .code(),
        "offset_out_of_range"
    );

    // Viewer state machine
    let mut viewer = Viewer::new(25);
    assert_eq!(viewer.page_lines(), 25);
    assert!(!viewer.follow());
    assert!(!viewer.is_quit());
    assert_eq!(viewer.handle(Key::ToggleFollow), Action::SetFollow(true));
    assert!(viewer.follow());
    assert_eq!(viewer.handle(Key::Up), Action::ScrollUp(1));
    assert!(!viewer.follow());
    assert_eq!(viewer.handle(Key::Down), Action::ScrollDown(1));
    assert_eq!(viewer.handle(Key::PageUp), Action::ScrollUp(25));
    assert_eq!(viewer.handle(Key::PageDown), Action::ScrollDown(25));
    assert_eq!(viewer.handle(Key::Home), Action::JumpStart);
    assert_eq!(viewer.handle(Key::End), Action::JumpEnd);
    assert_eq!(viewer.handle(Key::Quit), Action::Quit);
    assert!(viewer.is_quit());
}

#[test]
fn error_and_cli_edge_cases() {
    let err = ViewError::invalid_filter("x".repeat(300));
    assert!(err.message().chars().count() <= 200);
    assert!(err.to_string().starts_with("invalid_filter:"));

    let mut out = Vec::new();
    let mut err_buf = Vec::new();
    assert_eq!(cli::run::<&str>(&[], &mut out, &mut err_buf), 2);
    assert_eq!(cli::run(&["unknown"], &mut out, &mut err_buf), 2);
    assert_eq!(cli::run(&["tail", "--bad-flag"], &mut out, &mut err_buf), 2);
    assert_eq!(cli::run(&["page", "file.log"], &mut out, &mut err_buf), 2);
}

proptest! {
    #[test]
    fn highlight_spans_never_overlap_and_stay_in_bounds(s in "\\PC*") {
        let spans = highlight(&s);
        for (idx, span) in spans.iter().enumerate() {
            prop_assert!(span.start < span.end);
            prop_assert!(span.end <= s.len());
            if idx > 0 {
                prop_assert!(spans[idx - 1].end <= span.start);
            }
        }
    }

    #[test]
    fn tail_and_lines_from_offset_agree_on_small_buffers(lines_vec in prop::collection::vec("[a-zA-Z0-9 ]{1,40}", 1..20)) {
        let mut raw = String::new();
        for l in &lines_vec {
            raw.push_str(l);
            raw.push('\n');
        }
        let mem = MemSource::new(raw.into_bytes());
        let mut view = LogView::open(mem, ViewOptions::default()).unwrap();
        let all_from_zero = view.lines_from_offset(0, lines_vec.len()).unwrap();
        let all_from_tail = view.tail(lines_vec.len()).unwrap();
        prop_assert_eq!(all_from_zero, all_from_tail);
    }
}
