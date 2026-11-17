use std::env;
use std::io;

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let code = tui_log_viewer::cli::run(&args[1..], &mut stdout, &mut stderr);
    std::process::exit(code);
}
