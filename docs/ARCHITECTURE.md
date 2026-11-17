# Architecture

## Overview

`trysac-tui-log-viewer` (`tui_log_viewer`) separates the random-access log viewing engine, syntax highlighter, ANSI frame renderer, navigation state machine, and CLI runner from raw terminal I/O so every component is deterministic and unit-tested.

## Components

1. **`Source` (`src/source.rs`)**: Trait abstracting thread-safe random-access byte storage (`len` and `read_at`), implemented by `FileSource` (`Mutex<File>`) and `MemSource` (in-memory buffer for testing and synthetic fixtures).
2. **`LogView` (`src/view.rs`)**: Enforces `ViewOptions` (`chunk_bytes = 65_536`, `max_line_bytes = 65_536`, `max_read_per_call = 1_048_576`).
   - `tail(n)` scans backwards in 64 KiB chunks from the end of the source until `n + 1` newlines are found or `max_read_per_call` is reached.
   - `lines_from_offset(offset, n)` scans forward from `offset - 1` to align to the next line start before reading up to `n` lines.
   - `refresh()` and `new_lines()` implement follow mode with partial-line buffering and `copytruncate` detection.
3. **`Filter` and `Level` (`src/filter.rs`)**: Detects severity levels from standalone tokens, `level=<x>` logfmt fields, or `"level":"<x>"` JSON fields, and evaluates `--grep`, `--exclude`, and `--level` predicates.
4. **`highlight` (`src/highlight.rs`)**: Single-pass non-overlapping scanner for RFC 3339 timestamps, severity levels, numbers, and double-quoted strings with backslash escapes.
5. **`render_frame` (`src/frame.rs`)**: Produces ECMA-48 ANSI CSI frames (`\x1b[H\x1b[2J`, SGR colors `31`/`33`/`36`/`32`/`0`, ellipsis truncation `…`, and percentage/follow status bar).
6. **`Viewer` (`src/viewer.rs`)**: Pure state machine translating `Key` events into `Action` values.
7. **`cli` (`src/cli.rs`)**: Non-interactive `tail` and `page` commands writing to injected `Write` streams.
