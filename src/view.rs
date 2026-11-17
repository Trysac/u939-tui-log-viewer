use crate::error::ViewError;
use crate::filter::{Filter, Level, detect_level};
use crate::source::Source;

/// Default chunk size (64 KiB) used when scanning forward or backward.
pub const DEFAULT_CHUNK_BYTES: usize = 65_536;
/// Default maximum bytes kept per single line (64 KiB).
pub const DEFAULT_MAX_LINE_BYTES: usize = 65_536;
/// Default maximum bytes read from the [`Source`] in a single operation (1 MiB).
pub const DEFAULT_MAX_READ_PER_CALL: usize = 1_048_576;

/// Configuration bounds for [`LogView`] memory and I/O operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// Size in bytes of the working buffer used to scan chunks (default: `65_536`).
    pub chunk_bytes: usize,
    /// Maximum number of bytes returned in a single [`Line`] before truncating (default: `65_536`).
    pub max_line_bytes: usize,
    /// Maximum total bytes read from the source during one query call (default: `1_048_576`).
    pub max_read_per_call: usize,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            chunk_bytes: DEFAULT_CHUNK_BYTES,
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            max_read_per_call: DEFAULT_MAX_READ_PER_CALL,
        }
    }
}

/// A single decoded log line along with its starting byte offset in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Starting byte offset of this line in the underlying [`Source`].
    pub offset: u64,
    /// UTF-8 decoded line text (without trailing `\n` or `\r\n`; invalid UTF-8 replaced by `U+FFFD`).
    pub text: String,
    /// `true` if the line was truncated due to `max_line_bytes` or `max_read_per_call`.
    pub truncated: bool,
    /// Detected log severity level, if any.
    pub level: Option<Level>,
}

impl Line {
    /// Constructs a [`Line`] and automatically detects its [`Level`].
    #[must_use]
    pub fn new(offset: u64, text: impl Into<String>, truncated: bool) -> Self {
        let text = text.into();
        let level = detect_level(&text);
        Self {
            offset,
            text,
            truncated,
            level,
        }
    }
}

/// Result of polling the underlying [`Source`] via [`LogView::refresh`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The source length has not changed.
    Unchanged,
    /// The source grew by the given number of bytes.
    Grown(u64),
    /// The source shrunk (e.g. `copytruncate` log rotation) and the follow cursor reset to `0`.
    Truncated,
}

/// Bounded-memory log viewer over a random-access [`Source`].
#[derive(Debug)]
pub struct LogView<S: Source> {
    source: S,
    opts: ViewOptions,
    last_len: u64,
    follow_pos: u64,
    pending_lines: Vec<Line>,
}

impl<S: Source> LogView<S> {
    /// Opens a [`LogView`] over `source` with the given [`ViewOptions`].
    ///
    /// # Errors
    ///
    /// Returns [`ViewError::OffsetOutOfRange`] if any option bound is zero, or [`ViewError::Io`]
    /// if querying `source.len()` fails.
    pub fn open(source: S, opts: ViewOptions) -> Result<Self, ViewError> {
        if opts.chunk_bytes == 0 || opts.max_line_bytes == 0 || opts.max_read_per_call == 0 {
            return Err(ViewError::offset_out_of_range(
                "chunk_bytes, max_line_bytes, and max_read_per_call must be > 0",
            ));
        }
        let len = source.len()?;
        Ok(Self {
            source,
            opts,
            last_len: len,
            follow_pos: len,
            pending_lines: Vec::new(),
        })
    }

    /// Returns a reference to the underlying [`Source`].
    #[must_use]
    pub const fn source(&self) -> &S {
        &self.source
    }

    /// Returns the configured [`ViewOptions`].
    #[must_use]
    pub const fn options(&self) -> ViewOptions {
        self.opts
    }

    /// Returns the last observed byte length of the source.
    #[must_use]
    pub const fn last_len(&self) -> u64 {
        self.last_len
    }

    /// Reads the last `n` lines from the end of the source in file order.
    ///
    /// Scans backwards in `chunk_bytes` blocks until `n + 1` newlines are found, the beginning of
    /// the file is reached, or `max_read_per_call` bytes have been read.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError::Io`] if reading from the source fails.
    pub fn tail(&mut self, n: usize) -> Result<Vec<Line>, ViewError> {
        if n == 0 {
            return Ok(Vec::new());
        }
        let file_len = self.source.len()?;
        self.last_len = file_len;
        if file_len == 0 {
            return Ok(Vec::new());
        }

        // Determine effective end, ignoring a single trailing '\n' so we count actual lines.
        let mut cursor = file_len;
        let mut budget = self.opts.max_read_per_call;
        let mut newline_offsets: Vec<u64> = Vec::new();
        let mut hit_budget_limit = false;
        let mut buf = vec![0_u8; self.opts.chunk_bytes];

        while cursor > 0 && budget > 0 {
            let max_step = self.opts.chunk_bytes.min(budget) as u64;
            let read_len_u64 = cursor.min(max_step);
            let read_len = usize::try_from(read_len_u64).unwrap_or(0);
            let chunk_start = cursor - read_len_u64;

            let actual = self.source.read_at(chunk_start, &mut buf[..read_len])?;
            if actual == 0 {
                break;
            }
            budget = budget.saturating_sub(actual);

            for idx in (0..actual).rev() {
                let abs_pos = chunk_start + (idx as u64);
                if buf[idx] == b'\n' {
                    // Ignore the very last byte if it is the terminating newline of the file.
                    if abs_pos + 1 == file_len {
                        continue;
                    }
                    newline_offsets.push(abs_pos);
                    if newline_offsets.len() == n {
                        break;
                    }
                }
            }

            cursor = chunk_start;
            if newline_offsets.len() == n {
                break;
            }
            if budget == 0 && cursor > 0 {
                hit_budget_limit = true;
                break;
            }
        }

        // Reconstruct line start offsets in ascending file order.
        let mut starts: Vec<(u64, bool)> = Vec::with_capacity(n);
        if newline_offsets.len() < n {
            // Earliest line starts at `cursor` (either 0 or where budget ran out).
            starts.push((cursor, hit_budget_limit));
        }
        for &nl_pos in newline_offsets.iter().rev() {
            starts.push((nl_pos + 1, false));
        }

        self.read_lines_at_starts(&starts, file_len)
    }

    /// Reads up to `n` consecutive lines starting at or after `offset`.
    ///
    /// If `offset > 0`, scans forward from `offset - 1` to locate the next line start (so if
    /// `offset - 1` is `\n`, `offset` itself is the line start; otherwise advances to the byte
    /// after the next `\n`).
    ///
    /// # Errors
    ///
    /// Returns [`ViewError::OffsetOutOfRange`] if `offset` exceeds the source length, or
    /// [`ViewError::Io`] if reading fails.
    pub fn lines_from_offset(&mut self, offset: u64, n: usize) -> Result<Vec<Line>, ViewError> {
        let file_len = self.source.len()?;
        self.last_len = file_len;
        if offset > file_len {
            return Err(ViewError::offset_out_of_range(format!(
                "offset {offset} exceeds source length {file_len}"
            )));
        }
        if n == 0 || offset == file_len {
            return Ok(Vec::new());
        }

        let mut budget = self.opts.max_read_per_call;
        let mut buf = vec![0_u8; self.opts.chunk_bytes];

        let first_start = if offset == 0 {
            0
        } else {
            let mut scan_pos = offset - 1;
            let mut found_start = None;
            while scan_pos < file_len && budget > 0 {
                let to_read =
                    usize::try_from((file_len - scan_pos).min(self.opts.chunk_bytes as u64))
                        .unwrap_or(self.opts.chunk_bytes)
                        .min(budget);
                if to_read == 0 {
                    break;
                }
                let actual = self.source.read_at(scan_pos, &mut buf[..to_read])?;
                if actual == 0 {
                    break;
                }
                if let Some(rel) = buf[..actual].iter().position(|&b| b == b'\n') {
                    let nl_abs = scan_pos + (rel as u64);
                    // Count only the bytes consumed up to the newline so the following line read can reuse the chunk.
                    budget = budget.saturating_sub(rel + 1);
                    found_start = Some(nl_abs + 1);
                    break;
                }
                budget = budget.saturating_sub(actual);
                scan_pos += actual as u64;
            }
            match found_start {
                Some(pos) if pos < file_len => pos,
                _ => return Ok(Vec::new()),
            }
        };

        self.read_n_lines_forward(first_start, n, file_len, budget, &mut buf)
    }

    /// Searches forward from `from` byte offset for the first line matching `filter`.
    ///
    /// Respects `max_read_per_call` so a single search call stays bounded.
    ///
    /// # Errors
    ///
    /// Returns [`ViewError::OffsetOutOfRange`] if `from` exceeds the source length, or
    /// [`ViewError::Io`] on read errors.
    pub fn search_forward(
        &mut self,
        from: u64,
        filter: &Filter,
    ) -> Result<Option<Line>, ViewError> {
        let file_len = self.source.len()?;
        self.last_len = file_len;
        if from > file_len {
            return Err(ViewError::offset_out_of_range(format!(
                "offset {from} exceeds source length {file_len}"
            )));
        }
        if from == file_len {
            return Ok(None);
        }

        let mut budget = self.opts.max_read_per_call;
        let mut buf = vec![0_u8; self.opts.chunk_bytes];

        // Align to line start if from > 0.
        let mut cursor = if from == 0 {
            0
        } else {
            let mut scan_pos = from - 1;
            let mut found = None;
            while scan_pos < file_len && budget > 0 {
                let to_read =
                    usize::try_from((file_len - scan_pos).min(self.opts.chunk_bytes as u64))
                        .unwrap_or(self.opts.chunk_bytes)
                        .min(budget);
                let actual = self.source.read_at(scan_pos, &mut buf[..to_read])?;
                if actual == 0 {
                    break;
                }
                if let Some(rel) = buf[..actual].iter().position(|&b| b == b'\n') {
                    budget = budget.saturating_sub(rel + 1);
                    found = Some(scan_pos + (rel as u64) + 1);
                    break;
                }
                budget = budget.saturating_sub(actual);
                scan_pos += actual as u64;
            }
            match found {
                Some(pos) if pos < file_len => pos,
                _ => return Ok(None),
            }
        };

        let mut line_offset = cursor;
        let mut line_buf: Vec<u8> = Vec::new();
        let mut line_truncated = false;

        while cursor < file_len && budget > 0 {
            let to_read = usize::try_from((file_len - cursor).min(self.opts.chunk_bytes as u64))
                .unwrap_or(self.opts.chunk_bytes)
                .min(budget);
            let actual = self.source.read_at(cursor, &mut buf[..to_read])?;
            if actual == 0 {
                break;
            }
            budget = budget.saturating_sub(actual);
            let chunk = &buf[..actual];
            let mut slice_start = 0;

            for (i, &b) in chunk.iter().enumerate() {
                if b == b'\n' {
                    append_capped(
                        &mut line_buf,
                        &chunk[slice_start..i],
                        self.opts.max_line_bytes,
                        &mut line_truncated,
                    );
                    let line = build_line(line_offset, &line_buf, line_truncated);
                    line_buf.clear();
                    line_truncated = false;
                    line_offset = cursor + (i as u64) + 1;
                    slice_start = i + 1;
                    if filter.matches(&line) {
                        return Ok(Some(line));
                    }
                }
            }
            if slice_start < actual {
                append_capped(
                    &mut line_buf,
                    &chunk[slice_start..actual],
                    self.opts.max_line_bytes,
                    &mut line_truncated,
                );
            }
            cursor += actual as u64;
        }

        if !line_buf.is_empty() {
            if cursor < file_len {
                line_truncated = true;
            }
            let line = build_line(line_offset, &line_buf, line_truncated);
            if filter.matches(&line) {
                return Ok(Some(line));
            }
        }

        Ok(None)
    }

    /// Polls the source for size changes since the last open or refresh.
    ///
    /// - If the source grew, reads complete (`\n`-terminated) new lines into the pending buffer
    ///   and returns [`Change::Grown`] with the byte delta. Partial trailing lines without a
    ///   newline are held until their newline arrives.
    /// - If the source shrunk (`new_len < last_len`), resets the follow cursor to `0`, reads
    ///   complete lines from the new content, and returns [`Change::Truncated`].
    /// - Otherwise returns [`Change::Unchanged`].
    ///
    /// # Errors
    ///
    /// Returns [`ViewError::Io`] if querying or reading the source fails.
    pub fn refresh(&mut self) -> Result<Change, ViewError> {
        let current_len = self.source.len()?;
        if current_len == self.last_len {
            return Ok(Change::Unchanged);
        }

        let change = if current_len < self.last_len {
            self.follow_pos = 0;
            self.last_len = current_len;
            Change::Truncated
        } else {
            let delta = current_len - self.last_len;
            self.last_len = current_len;
            Change::Grown(delta)
        };

        self.read_complete_follow_lines(current_len)?;
        Ok(change)
    }

    /// Drains and returns all complete new lines collected by previous [`LogView::refresh`] calls.
    pub fn new_lines(&mut self) -> Vec<Line> {
        std::mem::take(&mut self.pending_lines)
    }

    fn read_complete_follow_lines(&mut self, target_len: u64) -> Result<(), ViewError> {
        let mut budget = self.opts.max_read_per_call;
        let mut buf = vec![0_u8; self.opts.chunk_bytes];
        let mut cursor = self.follow_pos;
        let mut line_start = cursor;
        let mut line_buf: Vec<u8> = Vec::new();
        let mut line_truncated = false;

        while cursor < target_len && budget > 0 {
            let to_read = usize::try_from((target_len - cursor).min(self.opts.chunk_bytes as u64))
                .unwrap_or(self.opts.chunk_bytes)
                .min(budget);
            let actual = self.source.read_at(cursor, &mut buf[..to_read])?;
            if actual == 0 {
                break;
            }
            budget = budget.saturating_sub(actual);
            let chunk = &buf[..actual];
            let mut slice_start = 0;

            for (i, &b) in chunk.iter().enumerate() {
                if b == b'\n' {
                    append_capped(
                        &mut line_buf,
                        &chunk[slice_start..i],
                        self.opts.max_line_bytes,
                        &mut line_truncated,
                    );
                    let line = build_line(line_start, &line_buf, line_truncated);
                    self.pending_lines.push(line);
                    line_buf.clear();
                    line_truncated = false;
                    line_start = cursor + (i as u64) + 1;
                    self.follow_pos = line_start;
                    slice_start = i + 1;
                }
            }
            if slice_start < actual {
                append_capped(
                    &mut line_buf,
                    &chunk[slice_start..actual],
                    self.opts.max_line_bytes,
                    &mut line_truncated,
                );
            }
            cursor += actual as u64;
        }
        Ok(())
    }

    fn read_lines_at_starts(
        &self,
        starts: &[(u64, bool)],
        file_len: u64,
    ) -> Result<Vec<Line>, ViewError> {
        if starts.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(starts.len());
        let mut buf = vec![0_u8; self.opts.chunk_bytes];

        for (idx, &(start_offset, pre_truncated)) in starts.iter().enumerate() {
            let next_bound = starts
                .get(idx + 1)
                .map_or(file_len, |&(next_start, _)| next_start.saturating_sub(1));
            let span_len = next_bound.saturating_sub(start_offset);
            let max_bytes = self.opts.max_line_bytes as u64;
            let to_fetch = span_len.min(max_bytes);
            let mut line_truncated = pre_truncated || (span_len > max_bytes);

            let mut line_bytes = Vec::with_capacity(usize::try_from(to_fetch).unwrap_or(0));
            let mut pos = start_offset;
            let limit = start_offset + to_fetch;
            while pos < limit {
                let step = usize::try_from((limit - pos).min(self.opts.chunk_bytes as u64))
                    .unwrap_or(self.opts.chunk_bytes);
                let actual = self.source.read_at(pos, &mut buf[..step])?;
                if actual == 0 {
                    break;
                }
                if let Some(nl_rel) = buf[..actual].iter().position(|&b| b == b'\n') {
                    line_bytes.extend_from_slice(&buf[..nl_rel]);
                    break;
                }
                line_bytes.extend_from_slice(&buf[..actual]);
                pos += actual as u64;
            }
            // If we did not strip a trailing '\r' solely because the line was truncated right at '\r',
            // only strip '\r' when not truncated.
            if !line_truncated && line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }
            if line_bytes.len() > self.opts.max_line_bytes {
                line_bytes.truncate(self.opts.max_line_bytes);
                line_truncated = true;
            }
            let text = String::from_utf8_lossy(&line_bytes).into_owned();
            results.push(Line::new(start_offset, text, line_truncated));
        }

        Ok(results)
    }

    fn read_n_lines_forward(
        &self,
        first_start: u64,
        n: usize,
        file_len: u64,
        mut budget: usize,
        buf: &mut [u8],
    ) -> Result<Vec<Line>, ViewError> {
        let mut lines = Vec::with_capacity(n);
        let mut cursor = first_start;
        let mut line_start = first_start;
        let mut line_buf: Vec<u8> = Vec::new();
        let mut line_truncated = false;

        while cursor < file_len && budget > 0 && lines.len() < n {
            let to_read = usize::try_from((file_len - cursor).min(buf.len() as u64))
                .unwrap_or(buf.len())
                .min(budget);
            if to_read == 0 {
                break;
            }
            let actual = self.source.read_at(cursor, &mut buf[..to_read])?;
            if actual == 0 {
                break;
            }
            budget = budget.saturating_sub(actual);
            let chunk = &buf[..actual];
            let mut slice_start = 0;

            for (i, &b) in chunk.iter().enumerate() {
                if b == b'\n' {
                    append_capped(
                        &mut line_buf,
                        &chunk[slice_start..i],
                        self.opts.max_line_bytes,
                        &mut line_truncated,
                    );
                    lines.push(build_line(line_start, &line_buf, line_truncated));
                    line_buf.clear();
                    line_truncated = false;
                    line_start = cursor + (i as u64) + 1;
                    slice_start = i + 1;
                    if lines.len() == n {
                        break;
                    }
                }
            }

            if lines.len() < n && slice_start < actual {
                append_capped(
                    &mut line_buf,
                    &chunk[slice_start..actual],
                    self.opts.max_line_bytes,
                    &mut line_truncated,
                );
            }
            cursor += actual as u64;
        }

        if lines.len() < n && !line_buf.is_empty() {
            if cursor < file_len {
                line_truncated = true;
            }
            lines.push(build_line(line_start, &line_buf, line_truncated));
        }

        Ok(lines)
    }
}

fn append_capped(dst: &mut Vec<u8>, src: &[u8], max_bytes: usize, truncated: &mut bool) {
    if dst.len() >= max_bytes {
        if !src.is_empty() {
            *truncated = true;
        }
        return;
    }
    let remaining = max_bytes - dst.len();
    if src.len() > remaining {
        dst.extend_from_slice(&src[..remaining]);
        *truncated = true;
    } else {
        dst.extend_from_slice(src);
    }
}

fn build_line(offset: u64, raw: &[u8], truncated: bool) -> Line {
    let slice = if !truncated && raw.last() == Some(&b'\r') {
        &raw[..raw.len() - 1]
    } else {
        raw
    };
    let text = String::from_utf8_lossy(slice).into_owned();
    Line::new(offset, text, truncated)
}
