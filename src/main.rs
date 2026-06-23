use std::process;

use clap::{CommandFactory, Parser};
use pawflash::cli::args::{Cli, Commands};
use pawflash::cli::init_logging;
use tracing::error;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    init_logging(cli.verbose);

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
            pawflash::cli::force_fastboot::run().await?;
        }
        Some(Commands::Flash { action, partition, image, slot, both }) => {
            pawflash::cli::flash::run(action, partition, image, slot, both).await?;
        }
        Some(Commands::DisableVbmeta) => {
            pawflash::cli::disable_vbmeta::run().await?;
        }
        Some(Commands::FormatData { fs_options, clean_test }) => {
            pawflash::cli::format_data::run(fs_options, clean_test).await?;
        }
        Some(Commands::Device { action }) => {
            pawflash::cli::device::run(action).await?;
        }
    }

    Ok(())
}
