use std::process;

use clap::{CommandFactory, Parser};
use pawflash_cli::cli::args::{Cli, Commands};
use pawflash_cli::cli::init_logging;
use pawflash_core::flash::executor::set_expected_serial;
use tracing::error;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    init_logging(cli.verbose);
    if let Some(ref serial) = cli.serial {
        set_expected_serial(serial);
    }

    if let Err(err) = run(cli).await {
        error!("{err:#}");
        process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
        }
        Some(Commands::ForceFastboot) => {
            pawflash_cli::cli::force_fastboot::run().await?;
        }
        Some(Commands::Flash { action, partition, image, slot, both }) => {
            pawflash_cli::cli::flash::run(action, partition, image, slot, both).await?;
        }
        Some(Commands::DisableVbmeta) => {
            pawflash_cli::cli::disable_vbmeta::run().await?;
        }
        Some(Commands::FormatData { fs_options, fs_type, clean_test }) => {
            pawflash_cli::cli::format_data::run(fs_options, fs_type, clean_test).await?;
        }
        Some(Commands::Device { action }) => {
            pawflash_cli::cli::device::run(action).await?;
        }
    }

    Ok(())
}
