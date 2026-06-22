use anyhow::Result;
use clap::{CommandFactory, Parser};
use pawflash::cli::args::{Cli, Commands, ScatterAction, parse_mode, parse_storage};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
        }
        Some(Commands::ForceFastboot { verbose }) => {
            pawflash::cli::force_fastboot::run(verbose)?;
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
    }

    Ok(())
}
