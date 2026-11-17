# Bounded-Chunk Random-Access Log Viewing Core

Status: accepted (generated, pending human review)

## Context

Engineers inspecting multi-gigabyte server logs need tailing, arbitrary offset paging, filtering, syntax highlighting, and live follow without loading the whole file into memory or blocking on unbounded scans.

## Decision

Implement a standard-library-only core built around a random-access `Source` trait (`len` + `read_at`) and a `LogView` engine that enforces strict per-chunk (64 KiB), per-line (64 KiB), and per-call (1 MiB) byte limits, paired with a pure syntax highlighter, ECMA-48 ANSI frame renderer, and state machine.

## Consequences

Opening and tailing a 5 GiB file reads at most 1 MiB and keeps only a 64 KiB working buffer plus the returned lines in memory; all core behaviors and CLI commands can be deterministically verified in unit and property tests without creating multi-gigabyte files on disk.
