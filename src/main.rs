use anyhow::Result;
use clap::{CommandFactory, Parser};
use pawflash::cli::args::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
        }
        Some(Commands::ForceFastboot { verbose }) => {
            pawflash::cli::force_fastboot::run(verbose).await?;
        }
        Some(Commands::Flash { action, partition, image, slot, both, verbose }) => {
            pawflash::cli::flash::run(action, partition, image, slot, both, verbose).await?;
        }
        Some(Commands::DisableVbmeta { verbose }) => {
            pawflash::cli::disable_vbmeta::run(verbose).await?;
        }
        Some(Commands::FormatData { verbose, fs_options }) => {
            pawflash::cli::format_data::run(verbose, fs_options).await?;
        }
        Some(Commands::Device { verbose, action }) => {
            pawflash::cli::device::run(verbose, action).await?;
        }
    }

    Ok(())
}
