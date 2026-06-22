use anyhow::Result;
use clap::{CommandFactory, Parser};
use pawflash::cli::args::{Cli, Commands};
use pawflash::cli::init_stderr_logging;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = if cli.verbose { "debug" } else { "info" };
    init_stderr_logging(level);

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
        Some(Commands::FormatData { fs_options }) => {
            pawflash::cli::format_data::run(fs_options).await?;
        }
        Some(Commands::Device { action }) => {
            pawflash::cli::device::run(action).await?;
        }
    }

    Ok(())
}
