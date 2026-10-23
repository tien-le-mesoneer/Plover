//! The `plv` binary; everything lives in the library so the desktop app can embed it.

fn main() -> std::process::ExitCode {
    plover_cli::main_with_args(std::env::args_os())
}
