use anyhow::Result;
use clap::{CommandFactory, Parser};
use pawflash::cli::args::{Cli, Commands, ScatterAction, parse_mode, parse_storage};

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
        Some(Commands::Scatter { action }) => {
            match action {
                ScatterAction::Parse { scatter, full_json } => {
                    pawflash::cli::scatter::run_parse(&scatter, full_json)?;
                }
                ScatterAction::Plan {
                    scatter,
                    json,
                    verbose,
                    mode,
                    storage,
                    part,
                    group,
                    firmware_dir,
                    package_root,
                    check_images,
                    image_search,
                    include_preloader,
                    allow_incomplete_slots,
                } => {
                    pawflash::cli::scatter::run_plan(
                        &scatter,
                        json,
                        verbose,
                        parse_mode(&mode)?,
                        parse_storage(&storage)?,
                        part,
                        group,
                        firmware_dir,
                        package_root,
                        check_images,
                        image_search,
                        include_preloader,
                        allow_incomplete_slots,
                    )?;
                }
            }
        }
        Some(Commands::Flash { scatter, dry_run, verbose, mode, storage, part, group, firmware_dir, check_images, include_preloader }) => {
            pawflash::cli::flash::run(
                &scatter,
                dry_run,
                verbose,
                parse_mode(&mode)?,
                parse_storage(&storage)?,
                part,
                group,
                firmware_dir,
                check_images,
                include_preloader,
            ).await?;
        }
        Some(Commands::FormatData { verbose, fs_options }) => {
            pawflash::cli::format_data::run(verbose, fs_options).await?;
        }
        Some(Commands::Device { action }) => {
            pawflash::cli::device::run(action).await?;
        }
    }

    Ok(())
}
