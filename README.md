# trysac-tui-log-viewer

Terminal log viewer core that tails, pages, searches, and filters multi-gigabyte log files through bounded random-access reads, with syntax highlighting, ANSI terminal frame rendering, and a follow mode that survives `copytruncate` rotation.

## Usage

```rust
use tui_log_viewer::{Filter, Level, LogView, MemSource, ViewOptions, highlight};

let src = MemSource::new(b"2026-11-17T10:00:00Z ERROR code=42 \"bad input\"\n".to_vec());
let mut view = LogView::open(src, ViewOptions::default()).unwrap();
let lines = view.tail(1).unwrap();
let filter = Filter::parse(&["--grep", "bad", "--level", "warn"]).unwrap();
assert!(filter.matches(&lines[0]));
assert_eq!(lines[0].level, Some(Level::Error));
assert_eq!(highlight(&lines[0].text).len(), 4);
```

## Architecture & Guarantees

- **Bounded Memory and Reads**: `LogView` reads in `65_536`-byte chunks (`chunk_bytes`), caps individual lines at `65_536` bytes (`max_line_bytes`), and limits total bytes read per query call to `1_048_576` bytes (`max_read_per_call`). Opening and tailing a 5 GiB file never loads more than one chunk plus the requested lines.
- **Level Detection & Filtering**: Recognizes standalone level tokens (`TRACE`, `DEBUG`, `INFO`, `WARN`/`WARNING`, `ERROR`, `FATAL`), logfmt `level=<lvl>`, and JSON `"level":"<lvl>"`. Filters combine `--grep`, `--exclude`, and `--level`.
- **Follow Mode**: Tracks file growth and emits complete newline-terminated lines while holding partial trailing lines until completed, resetting automatically to offset `0` on `copytruncate` shrink.
