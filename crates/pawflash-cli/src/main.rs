use std::process;

use clap::{CommandFactory, Parser};
use miette::IntoDiagnostic;
use pawflash_cli::cli::args::{Cli, Commands};
use pawflash_cli::cli::init_logging;
use pawflash_core::flash::executor::set_expected_serial;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    init_logging(cli.verbose);
    if let Some(ref serial) = cli.serial {
        set_expected_serial(serial);
    }

    if let Err(err) = run(cli).await {
        eprintln!("{err}");
        process::exit(1);
    }
}

async fn run(cli: Cli) -> miette::Result<()> {
    let simulate = cli.simulate;
    match cli.command {
        None => {
            let mut cmd = Cli::command();
            cmd.print_help().into_diagnostic()?;
            println!();
        }
        Some(Commands::ForceFastboot) => {
            pawflash_cli::cli::force_fastboot::run(simulate).await?;
        }
        Some(Commands::Flash { action, partition, image, slot, both }) => {
            pawflash_cli::cli::flash::run(action, partition, image, slot, both, simulate).await?;
        }
        Some(Commands::DisableVbmeta) => {
            pawflash_cli::cli::disable_vbmeta::run(simulate).await?;
        }
        Some(Commands::FormatData { fs_options, fs_type, clean_test }) => {
            pawflash_cli::cli::format_data::run(fs_options, fs_type, clean_test, simulate).await?;
        }
        Some(Commands::Device { action }) => {
            pawflash_cli::cli::device::run(action, simulate).await?;
        }
    }

    Ok(())
}
