use std::io::Write;

use crate::filter::Filter;
use crate::frame::render_highlighted_line;
use crate::source::FileSource;
use crate::view::{Line, LogView, ViewOptions};

const DEFAULT_LINES: usize = 10;

/// Runs the CLI command specified in `args` (excluding the binary name).
///
/// Supported commands:
/// - `tail [-n N] [--grep s] [--exclude s] [--level l] [--color] <file>`
/// - `page <file> --offset N [-n N] [--color]`
///
/// Exit codes:
/// - `0`: Success
/// - `1`: Domain failure (e.g. filter matched zero lines when filtering)
/// - `2`: Usage, argument parse, or I/O error
pub fn run<S: AsRef<str>>(args: &[S], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let str_args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let Some(&cmd) = str_args.first() else {
        let _ = writeln!(stderr, "usage: tui-log-viewer <tail|page> [options] <file>");
        return 2;
    };

    match cmd {
        "tail" => run_tail(&str_args[1..], stdout, stderr),
        "page" => run_page(&str_args[1..], stdout, stderr),
        other => {
            let _ = writeln!(stderr, "unknown command: {other}");
            2
        }
    }
}

fn run_tail(args: &[&str], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let mut count = DEFAULT_LINES;
    let mut color = false;
    let mut file_path: Option<&str> = None;
    let mut filter_args: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-n" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    let _ = writeln!(stderr, "missing value for -n");
                    return 2;
                };
                let Ok(parsed) = val.parse::<usize>() else {
                    let _ = writeln!(stderr, "invalid line count: {val}");
                    return 2;
                };
                count = parsed;
            }
            "--color" => {
                color = true;
            }
            "--grep" | "--exclude" | "--level" => {
                let flag = args[i];
                i += 1;
                let Some(val) = args.get(i) else {
                    let _ = writeln!(stderr, "missing value for {flag}");
                    return 2;
                };
                filter_args.push(flag);
                filter_args.push(val);
            }
            arg if arg.starts_with('-') => {
                let _ = writeln!(stderr, "unknown option: {arg}");
                return 2;
            }
            positional => {
                if file_path.is_some() {
                    let _ = writeln!(stderr, "unexpected positional argument: {positional}");
                    return 2;
                }
                file_path = Some(positional);
            }
        }
        i += 1;
    }

    let Some(path) = file_path else {
        let _ = writeln!(stderr, "missing file path for tail");
        return 2;
    };

    let filter = match Filter::parse(&filter_args) {
        Ok(f) => f,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 2;
        }
    };

    let source = match FileSource::open(path) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "io: {e}");
            return 2;
        }
    };

    let mut view = match LogView::open(source, ViewOptions::default()) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 2;
        }
    };

    let lines = match view.tail(count) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 2;
        }
    };

    write_lines(&lines, &filter, color, stdout, stderr)
}

fn run_page(args: &[&str], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let mut count = DEFAULT_LINES;
    let mut offset: Option<u64> = None;
    let mut color = false;
    let mut file_path: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-n" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    let _ = writeln!(stderr, "missing value for -n");
                    return 2;
                };
                let Ok(parsed) = val.parse::<usize>() else {
                    let _ = writeln!(stderr, "invalid line count: {val}");
                    return 2;
                };
                count = parsed;
            }
            "--offset" => {
                i += 1;
                let Some(val) = args.get(i) else {
                    let _ = writeln!(stderr, "missing value for --offset");
                    return 2;
                };
                let Ok(parsed) = val.parse::<u64>() else {
                    let _ = writeln!(stderr, "invalid offset: {val}");
                    return 2;
                };
                offset = Some(parsed);
            }
            "--color" => {
                color = true;
            }
            arg if arg.starts_with('-') => {
                let _ = writeln!(stderr, "unknown option: {arg}");
                return 2;
            }
            positional => {
                if file_path.is_some() {
                    let _ = writeln!(stderr, "unexpected positional argument: {positional}");
                    return 2;
                }
                file_path = Some(positional);
            }
        }
        i += 1;
    }

    let Some(path) = file_path else {
        let _ = writeln!(stderr, "missing file path for page");
        return 2;
    };
    let Some(off) = offset else {
        let _ = writeln!(stderr, "missing required --offset for page");
        return 2;
    };

    let source = match FileSource::open(path) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(stderr, "io: {e}");
            return 2;
        }
    };

    let mut view = match LogView::open(source, ViewOptions::default()) {
        Ok(v) => v,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 2;
        }
    };

    let lines = match view.lines_from_offset(off, count) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 1;
        }
    };

    write_lines(&lines, &Filter::new(), color, stdout, stderr)
}

fn write_lines(
    lines: &[Line],
    filter: &Filter,
    color: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    for line in lines {
        if filter.matches(line) {
            let rendered = if color {
                render_highlighted_line(&line.text)
            } else {
                line.text.clone()
            };
            if let Err(e) = writeln!(stdout, "{rendered}") {
                let _ = writeln!(stderr, "io: {e}");
                return 2;
            }
        }
    }
    0
}
