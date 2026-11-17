// The README is the crate documentation, so its usage example runs as a doc test.
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

/// Command-line interface runner for non-interactive `tail` and `page` subcommands.
pub mod cli;
mod error;
mod filter;
mod frame;
mod highlight;
mod source;
mod view;
mod viewer;

pub use error::ViewError;
pub use filter::{Filter, Level, detect_level};
pub use frame::{Status, render_frame, render_highlighted_line};
pub use highlight::{Span, SpanKind, highlight};
pub use source::{FileSource, MemSource, Source};
pub use view::{Change, Line, LogView, ViewOptions};
pub use viewer::{Action, Key, Viewer};
