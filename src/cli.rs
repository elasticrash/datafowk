use std::ffi::OsString;

use clap::{CommandFactory, Parser, Subcommand};

use crate::models::{CliOptions, Command, UiOptions};

pub const DEFAULT_CONFIG_PATH: &str = "mysql_config.toml";

#[derive(Debug, Parser)]
#[command(name = "datafowk")]
struct Cli {
    #[arg(long, global = true, default_value = DEFAULT_CONFIG_PATH)]
    config: String,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Execute the ETL pipeline
    Run {
        /// Validate the rules and simulate inserts without persisting
        #[arg(long)]
        dry_run: bool,

        /// Truncate destination tables once before loading
        #[arg(long)]
        truncate_destination: bool,
    },
    /// Open the interactive terminal UI
    Ui,
}

pub fn parse_cli<I, T>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match Cli::try_parse_from(
        std::iter::once(OsString::from("datafowk")).chain(args.into_iter().map(Into::into)),
    ) {
        Ok(cli) => Ok(cli.into_command()),
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => Ok(Command::Help),
        Err(error) => Err(error.to_string()),
    }
}

pub fn print_help() {
    print!("{}", Cli::command().render_long_help());
}

impl Cli {
    fn into_command(self) -> Command {
        match self.command {
            Some(CliCommand::Run {
                dry_run,
                truncate_destination,
            }) => Command::Run(CliOptions {
                config_path: self.config,
                dry_run,
                truncate_destination,
            }),
            Some(CliCommand::Ui) | None => Command::Ui(UiOptions {
                config_path: self.config,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_defaults_to_repo_config() {
        let command = parse_cli(Vec::<String>::new()).unwrap();

        assert_eq!(
            command,
            Command::Ui(UiOptions {
                config_path: String::from(DEFAULT_CONFIG_PATH),
            })
        );
    }

    #[test]
    fn parse_cli_supports_run_flags() {
        let command = parse_cli(vec![
            String::from("run"),
            String::from("--config"),
            String::from("custom.toml"),
            String::from("--dry-run"),
            String::from("--truncate-destination"),
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::Run(CliOptions {
                config_path: String::from("custom.toml"),
                dry_run: true,
                truncate_destination: true,
            })
        );
    }

    #[test]
    fn parse_cli_supports_ui_mode() {
        let command = parse_cli(vec![
            String::from("ui"),
            String::from("--config"),
            String::from("wizard.toml"),
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::Ui(UiOptions {
                config_path: String::from("wizard.toml"),
            })
        );
    }

    #[test]
    fn parse_cli_reports_help() {
        let command = parse_cli(vec![String::from("--help")]).unwrap();
        assert_eq!(command, Command::Help);
    }
}
