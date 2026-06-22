use anyhow::Result;
use clap::{CommandFactory, Parser};
use pawflash::cli::args::{Cli, Commands, ScatterAction};

fn print_help(sub_name: &str) -> Result<()> {
    let mut cmd = Cli::command();
    if let Some(sub) = cmd.find_subcommand_mut(sub_name) {
        sub.print_help()?;
        println!();
    }
    Ok(())
}

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
                ScatterAction::Parse { scatter: Some(scatter), full_json } => {
                    pawflash::cli::scatter::run_parse(&scatter, full_json)?;
                }
                ScatterAction::Parse { scatter: None, .. } => {
                    print_help("scatter")?;
                }
                ScatterAction::Plan { scatter: Some(scatter), json, verbose, mode, storage, part, group, firmware_dir, package_root, check_images, image_search, include_preloader, allow_incomplete_slots } => {
                    pawflash::cli::scatter::run_plan(
                        &scatter,
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
                    )?;
                }
                ScatterAction::Plan { scatter: None, .. } => {
                    print_help("scatter")?;
                }
            }
        }
        Some(Commands::Flash { scatter: Some(scatter), dry_run, verbose, mode, storage, part, group, firmware_dir, check_images, include_preloader }) => {
            pawflash::cli::flash::run(
                &scatter,
                dry_run,
                verbose,
                mode,
                storage,
                part,
                group,
                firmware_dir,
                check_images,
                include_preloader,
            ).await?;
        }
        Some(Commands::Flash { scatter: None, .. }) => {
            print_help("flash")?;
        }
        Some(Commands::FormatData { verbose, fs_options }) => {
            pawflash::cli::format_data::run(verbose, fs_options).await?;
        }
        Some(Commands::FlashRaw { partition: Some(partition), image: Some(image), slot, both, verbose }) => {
            pawflash::cli::flash_raw::run(&partition, &image, slot, both, verbose).await?;
        }
        Some(Commands::FlashRaw { .. }) => {
            print_help("flash-raw")?;
        }
        Some(Commands::DisableVbmeta { verbose }) => {
            pawflash::cli::disable_vbmeta::run(verbose).await?;
        }
        Some(Commands::Device { verbose, action }) => {
            pawflash::cli::device::run(verbose, action).await?;
        }
    }

    Ok(())
}
