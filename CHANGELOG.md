# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added bounded-memory `LogView` (`tail`, `lines_from_offset`, `search_forward`, `refresh`, `new_lines`), `Source` (`FileSource`, `MemSource`), `Filter` (`--grep`, `--exclude`, `--level`), `highlight`, `render_frame`, `Viewer` state machine, and CLI `tail`/`page` commands.
- Initial project structure from the factory utility template.
