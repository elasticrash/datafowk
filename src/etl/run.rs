use crate::{
    config::Config,
    etl_rule_parser::parser::parse_rule,
    models::{CliOptions, ExecutionSummary},
};

use super::{
    config_io::load_config,
    connect::connect,
    engine::{execute_rules, simulate_rules},
};

pub(crate) fn run(options: CliOptions) -> Result<ExecutionSummary, String> {
    let config = load_config(&options.config_path)?;
    run_config(&config, options.dry_run, options.truncate_destination)
}

pub(crate) fn run_config(
    config: &Config,
    dry_run: bool,
    truncate_destination: bool,
) -> Result<ExecutionSummary, String> {
    if config.rules.is_empty() {
        return Err(String::from(
            "the current config does not define any `[[rules]]` entries",
        ));
    }

    let parsed_rules = config
        .rules
        .iter()
        .map(|rule| parse_rule(&rule.expression))
        .collect::<Result<Vec<_>, _>>()?;

    let mut source_connection = connect(&config.connection_properties_origin, "origin")?;
    let mut destination_connection =
        connect(&config.connection_properties_destination, "destination")?;

    let mut summary = ExecutionSummary {
        dry_run,
        ..ExecutionSummary::default()
    };

    if dry_run {
        simulate_rules(
            &mut source_connection,
            &mut destination_connection,
            config,
            &parsed_rules,
            truncate_destination,
            &mut summary,
        )?;
    } else {
        execute_rules(
            &mut source_connection,
            &mut destination_connection,
            config,
            &parsed_rules,
            truncate_destination,
            &mut summary,
        )?;
    }

    Ok(summary)
}
