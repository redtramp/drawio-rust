//! drawio-sync CLI 入口（完整参数见 docs/06）。

use std::process::ExitCode;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str);
    match cmd {
        Some("status") => run_status(&args[2..]),
        Some("run") => run_sync(&args[2..]),
        Some("doctor") => run_doctor(),
        _ => {
            eprintln!(
                "usage:\n  drawio-sync status [upstream...]\n  drawio-sync run --all [--dry-run] [--json]\n  drawio-sync run <upstream> [--dry-run] [--json]\n  drawio-sync doctor"
            );
            ExitCode::FAILURE
        }
    }
}

fn run_status(args: &[String]) -> ExitCode {
    drawio_rust_lib::sync::fetch::status(std::env::current_dir().unwrap_or_default(), args)
        .map(|_| ExitCode::SUCCESS)
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        })
}

fn run_sync(args: &[String]) -> ExitCode {
    drawio_rust_lib::sync::apply::run(std::env::current_dir().unwrap_or_default(), args)
        .map(|_| ExitCode::SUCCESS)
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        })
}

fn run_doctor() -> ExitCode {
    drawio_rust_lib::sync::fetch::doctor()
        .map(|_| ExitCode::SUCCESS)
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        })
}
