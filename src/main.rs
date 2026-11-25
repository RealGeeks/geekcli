use clap::Parser;

use geekcli::cli::{run, Cli};
use geekcli::error::Error;
use geekcli::output::{print_error, Format, Printer};

const ISSUES: &str = "https://github.com/RealGeeks/geekcli/issues";

fn main() {
    let cli = Cli::parse();
    let format = Printer::new(cli.global.format(), false).format;
    install_panic_hook(format);
    if let Err(err) = run(cli) {
        if matches!(err, Error::BrokenPipe) {
            std::process::exit(0);
        }
        let format = if matches!(format, Format::Jsonl) {
            Format::Json
        } else {
            format
        };
        print_error(&err, format);
        std::process::exit(err.exit_code());
    }
}

/// A bug should still honour the error contract: a JSON error on stderr in
/// JSON mode and exit code 1, not Rust's panic text and 101.
fn install_panic_hook(format: Format) {
    std::panic::set_hook(Box::new(move |info| {
        let detail = info
            .payload()
            .downcast_ref::<&str>()
            .map(ToString::to_string)
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let location = info
            .location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();
        // `print!`/`println!` panic when stdout's reader has gone (`| head`);
        // stop quietly like other command-line tools instead of reporting a bug.
        if detail.starts_with("failed printing to stdout") {
            std::process::exit(0);
        }
        let message = format!("geekcli hit an internal error{location}: {detail}");
        eprintln!("{}", format_panic(format, &message));
        std::process::exit(1);
    }));
}

fn format_panic(format: Format, message: &str) -> String {
    if format == Format::Table {
        format!("error: {message}\n  please report it: {ISSUES}")
    } else {
        serde_json::json!({ "error": {
            "code": "internal_error",
            "message": message,
            "hint": format!("please report it: {ISSUES}"),
            "exit_code": 1,
        }})
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_panic_honours_table_and_json_error_contracts() {
        let table = format_panic(Format::Table, "boom");
        assert!(table.contains("error: boom") && table.contains(ISSUES));

        let json_out = format_panic(Format::Json, "boom");
        let parsed: serde_json::Value = serde_json::from_str(&json_out).unwrap_or_default();
        assert_eq!(parsed["error"]["code"], "internal_error");
        assert_eq!(parsed["error"]["message"], "boom");
        assert_eq!(parsed["error"]["exit_code"], 1);
    }
}
