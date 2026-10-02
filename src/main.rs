use clap::Parser;

use geekcli::cli::{run, Cli};
use geekcli::output::{print_error, Format, Printer};

fn main() {
    let cli = Cli::parse();
    let format = Printer::new(cli.global.format(), false).format;
    if let Err(err) = run(cli) {
        let format = if matches!(format, Format::Jsonl) {
            Format::Json
        } else {
            format
        };
        print_error(&err, format);
        std::process::exit(err.exit_code());
    }
}
