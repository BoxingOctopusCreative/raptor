use std::process::ExitCode;

use clap::Parser;
use raptor::{run, Cli};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("E: {e}");
            ExitCode::from(100)
        }
    }
}
