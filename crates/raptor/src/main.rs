use std::process::ExitCode;

use clap::Parser;
use raptor::{run, Cli};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            raptor::term::error_line(format!("{e}"));
            ExitCode::from(100)
        }
    }
}
