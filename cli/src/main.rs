//! The `marq-comments` binary: parses arguments, runs one command, and maps
//! the outcome to the exit codes of design 6.3.

use clap::error::ErrorKind;
use clap::Parser;
use marq_comments::cli::Cli;
use marq_comments::commands;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let help = matches!(
                e.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayVersion
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            );
            let _ = e.print();
            // clap's own usage-error code is 2, which design 6.3 gives to an id
            // that matched nothing. Bad arguments are exit 1.
            std::process::exit(if help { 0 } else { 1 });
        }
    };
    if let Some(dir) = &cli.dir {
        if let Err(e) = std::env::set_current_dir(dir) {
            eprintln!("cannot use {} as the directory: {e}", dir.display());
            std::process::exit(1);
        }
    }
    if let Err(failure) = commands::run(cli) {
        eprintln!("{failure}");
        std::process::exit(failure.exit_code());
    }
}
