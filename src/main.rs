mod config;
mod etl;
mod etl_rule_parser;
mod models;
mod transforms;
mod ui;
use std::env;
use std::process::ExitCode;

use etl::{parse_cli, print_help, run};
use models::Command;
use ui::run_ui;

fn main() -> ExitCode {
    match parse_cli(env::args().skip(1)) {
        Ok(Command::Help) => {
            print_help();
            ExitCode::SUCCESS
        }
        Ok(Command::Ui(options)) => match run_ui(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(Command::Run(options)) => match run(options) {
            Ok(summary) => {
                if summary.dry_run {
                    println!(
                        "Dry run simulation completed: {} rule(s), {} row(s) read, {} row(s) fully validated.",
                        summary.rules_processed, summary.rows_read, summary.rows_inserted
                    );
                } else {
                    println!(
                        "ETL completed: {} rule(s), {} row(s) read, {} row(s) inserted.",
                        summary.rules_processed, summary.rows_read, summary.rows_inserted
                    );
                }

                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("error: {error}\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}
