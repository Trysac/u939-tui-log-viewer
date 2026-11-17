# Security Policy & Threat Model

## Threat Model

Log viewers regularly process untrusted, malformed, or multi-gigabyte log files containing arbitrary byte sequences, binary garbage, or adversarial long lines without newline delimiters.

### Mitigations

- **Resource Exhaustion (Memory & I/O)**:
  - `ViewOptions::chunk_bytes` (default 64 KiB) bounds the working read buffer.
  - `ViewOptions::max_line_bytes` (default 64 KiB) caps any single line's allocation and marks overlong lines with `truncated = true`.
  - `ViewOptions::max_read_per_call` (default 1 MiB) bounds total bytes read from a `Source` per API call.
- **Malformed Text / Terminal Escape Injection**:
  - Invalid UTF-8 sequences are lossily decoded into `U+FFFD` without panicking.
  - Error messages in `ViewError` are capped at 200 characters.
- **No Unsafe Code**: `#![forbid(unsafe_code)]` is enforced across the crate.
