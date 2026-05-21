use std::collections::BTreeSet;
use std::fs;
use std::thread;
use std::time::Duration;

use mysql::{prelude::Queryable, Conn, Opts, Params, Row, Value};

use crate::config::{Config, ConnectionProperties};
use crate::etl_rule_parser::parser::{parse_rule, Rules};

const DEFAULT_CONFIG_PATH: &str = "mysql_config.toml";
const CONNECTION_RETRIES: usize = 10;
const CONNECTION_RETRY_DELAY_MS: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOptions {
    pub config_path: String,
    pub dry_run: bool,
    pub truncate_destination: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiOptions {
    pub config_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Run(CliOptions),
    Ui(UiOptions),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionSummary {
    pub rules_processed: usize,
    pub rows_read: usize,
    pub rows_inserted: usize,
    pub dry_run: bool,
}

pub fn parse_cli<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    enum Mode {
        Run,
        Ui,
    }

    let mut config_path = DEFAULT_CONFIG_PATH.to_string();
    let mut dry_run = false;
    let mut truncate_destination = false;
    let mut mode = Mode::Run;

    let mut args = args.into_iter();

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "run" => mode = Mode::Run,
            "ui" => mode = Mode::Ui,
            "--dry-run" => dry_run = true,
            "--truncate-destination" => truncate_destination = true,
            "--config" => {
                let Some(value) = args.next() else {
                    return Err(String::from("`--config` expects a file path"));
                };

                config_path = value;
            }
            unknown => {
                return Err(format!("unknown argument `{unknown}`"));
            }
        }
    }

    match mode {
        Mode::Run => Ok(Command::Run(CliOptions {
            config_path,
            dry_run,
            truncate_destination,
        })),
        Mode::Ui => Ok(Command::Ui(UiOptions { config_path })),
    }
}

pub fn print_help() {
    println!(
        "datafowk\n\n\
Usage:\n  cargo run -- [run] [--config PATH] [--dry-run] [--truncate-destination]\n  cargo run -- ui [--config PATH]\n\n\
Commands:\n  run                       Execute the ETL pipeline (default)\n  ui                        Open the interactive terminal UI\n\n\
Options:\n  --config PATH             Path to the TOML config file (default: {DEFAULT_CONFIG_PATH})\n  --dry-run                 Validate the rules and read source rows without inserting\n  --truncate-destination    Truncate destination tables once before loading\n  -h, --help                Show this help message"
    );
}

pub fn run(options: CliOptions) -> Result<ExecutionSummary, String> {
    let config = load_config(&options.config_path)?;
    run_config(&config, options.dry_run, options.truncate_destination)
}

pub fn load_config(path: &str) -> Result<Config, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read config `{path}`: {error}"))?;

    toml::from_str(&contents).map_err(|error| format!("failed to parse config `{path}`: {error}"))
}

pub fn load_config_or_default(path: &str) -> Result<Config, String> {
    match fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents)
            .map_err(|error| format!("failed to parse config `{path}`: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(format!("failed to read config `{path}`: {error}")),
    }
}

pub fn save_config(path: &str, config: &Config) -> Result<(), String> {
    let contents = toml::to_string_pretty(config)
        .map_err(|error| format!("failed to serialize config `{path}`: {error}"))?;

    fs::write(path, contents).map_err(|error| format!("failed to write config `{path}`: {error}"))
}

pub fn run_config(
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

    if truncate_destination {
        truncate_destination_tables(&parsed_rules, config, &mut destination_connection)?;
    }

    let mut summary = ExecutionSummary {
        dry_run,
        ..ExecutionSummary::default()
    };

    for rule in &parsed_rules {
        execute_rule(
            rule,
            config,
            &mut source_connection,
            &mut destination_connection,
            dry_run,
            &mut summary,
        )?;
        summary.rules_processed += 1;
    }

    Ok(summary)
}

pub fn connect(connection_properties: &ConnectionProperties, label: &str) -> Result<Conn, String> {
    let url = format!(
        "mysql://{}:{}@{}:{}/{}",
        connection_properties.user,
        connection_properties.password,
        connection_properties.address,
        connection_properties.port,
        connection_properties.schema
    );

    let opts = Opts::from_url(&url)
        .map_err(|error| format!("invalid {label} connection URL `{url}`: {error}"))?;

    let mut last_error = None;

    for attempt in 1..=CONNECTION_RETRIES {
        match Conn::new(opts.clone()) {
            Ok(connection) => return Ok(connection),
            Err(error) => {
                last_error = Some(error);
                if attempt < CONNECTION_RETRIES {
                    thread::sleep(Duration::from_millis(CONNECTION_RETRY_DELAY_MS));
                }
            }
        }
    }

    Err(format!(
        "failed to connect to the {label} database after {CONNECTION_RETRIES} attempts: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| String::from("unknown connection error"))
    ))
}

fn truncate_destination_tables(
    rules: &[Rules],
    config: &Config,
    destination_connection: &mut Conn,
) -> Result<(), String> {
    let destination_schema = &config.connection_properties_destination.schema;
    let mut tables = BTreeSet::new();

    for rule in rules {
        ensure_matches_database(&rule.destination_db, destination_schema, "destination")?;
        tables.insert(rule.destination_table.as_str());
    }

    for table in tables {
        let truncate_statement = format!("TRUNCATE TABLE {}", quote_identifier(table)?);
        destination_connection
            .query_drop(truncate_statement)
            .map_err(|error| format!("failed to truncate destination table `{table}`: {error}"))?;
    }

    Ok(())
}

fn execute_rule(
    rule: &Rules,
    config: &Config,
    source_connection: &mut Conn,
    destination_connection: &mut Conn,
    dry_run: bool,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    ensure_matches_database(
        &rule.source_db,
        &config.connection_properties_origin.schema,
        "origin",
    )?;
    ensure_matches_database(
        &rule.destination_db,
        &config.connection_properties_destination.schema,
        "destination",
    )?;

    if rule.source_fields.len() != rule.destination_fields.len() {
        return Err(format!(
            "rule `{}` -> `{}` must map the same number of source and destination fields",
            rule.source_table, rule.destination_table
        ));
    }

    let select_statement = build_select_statement(rule)?;
    let rows: Vec<Row> = source_connection
        .query(select_statement.as_str())
        .map_err(|error| {
            format!(
                "failed to read source rows for `{}`: {error}",
                rule.source_table
            )
        })?;

    summary.rows_read += rows.len();

    if rows.is_empty() {
        return Ok(());
    }

    let insert_statement = build_insert_statement(rule)?;

    for row in rows {
        let values = transform_row(rule, row)?;
        summary.rows_inserted += 1;

        if !dry_run {
            destination_connection
                .exec_drop(insert_statement.as_str(), Params::from(values))
                .map_err(|error| {
                    format!(
                        "failed to insert into destination table `{}`: {error}",
                        rule.destination_table
                    )
                })?;
        }
    }

    Ok(())
}

fn build_select_statement(rule: &Rules) -> Result<String, String> {
    let fields = rule
        .source_fields
        .iter()
        .map(|field| quote_identifier(field))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    Ok(format!(
        "SELECT {fields} FROM {}",
        quote_identifier(&rule.source_table)?
    ))
}

fn build_insert_statement(rule: &Rules) -> Result<String, String> {
    let columns = rule
        .destination_fields
        .iter()
        .map(|field| quote_identifier(field))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    let placeholders = vec!["?"; rule.destination_fields.len()].join(", ");

    Ok(format!(
        "INSERT INTO {} ({columns}) VALUES ({placeholders})",
        quote_identifier(&rule.destination_table)?
    ))
}

fn quote_identifier(identifier: &str) -> Result<String, String> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(format!(
            "unsupported identifier `{identifier}`; only letters, numbers, and underscores are allowed"
        ));
    }

    Ok(format!("`{identifier}`"))
}

fn ensure_matches_database(
    rule_database: &str,
    configured_schema: &str,
    alias: &str,
) -> Result<(), String> {
    if rule_database == alias || rule_database == configured_schema {
        Ok(())
    } else {
        Err(format!(
            "rule database `{rule_database}` does not match the configured {alias} schema `{configured_schema}`"
        ))
    }
}

fn transform_row(rule: &Rules, row: Row) -> Result<Vec<Value>, String> {
    let mut values = row.unwrap();

    if values.len() != rule.source_fields.len() {
        return Err(format!(
            "source query for table `{}` returned {} columns but the rule expects {}",
            rule.source_table,
            values.len(),
            rule.source_fields.len()
        ));
    }

    for function_name in &rule.function_chain {
        for value in &mut values {
            apply_function(value, function_name)?;
        }
    }

    Ok(values)
}

fn apply_function(value: &mut Value, function_name: &str) -> Result<(), String> {
    match function_name {
        "copy" | "identity" => Ok(()),
        "trim" => transform_string_value(value, |text| text.trim().to_string()),
        "lowercase" => transform_string_value(value, |text| text.to_lowercase()),
        "uppercase" => transform_string_value(value, |text| text.to_uppercase()),
        unknown => Err(format!("unsupported transformation function `{unknown}`")),
    }
}

fn transform_string_value<F>(value: &mut Value, transformer: F) -> Result<(), String>
where
    F: FnOnce(&str) -> String,
{
    if let Value::Bytes(bytes) = value {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| format!("string transformation requires UTF-8 data: {error}"))?;

        *bytes = transformer(text).into_bytes();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_defaults_to_repo_config() {
        let command = parse_cli(Vec::<String>::new()).unwrap();

        assert_eq!(
            command,
            Command::Run(CliOptions {
                config_path: String::from(DEFAULT_CONFIG_PATH),
                dry_run: false,
                truncate_destination: false,
            })
        );
    }

    #[test]
    fn parse_cli_supports_flags() {
        let command = parse_cli(vec![
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
    fn build_sql_statements_quotes_identifiers() {
        let rule =
            parse_rule("(origin:users)[firstname,lastname]<copy>(destination:spot)[name,surname]")
                .unwrap();

        assert_eq!(
            build_select_statement(&rule).unwrap(),
            "SELECT `firstname`, `lastname` FROM `users`"
        );
        assert_eq!(
            build_insert_statement(&rule).unwrap(),
            "INSERT INTO `spot` (`name`, `surname`) VALUES (?, ?)"
        );
    }

    #[test]
    fn apply_function_transforms_string_values() {
        let mut value = Value::Bytes(b"  Alice  ".to_vec());

        apply_function(&mut value, "trim").unwrap();
        apply_function(&mut value, "uppercase").unwrap();

        assert_eq!(value, Value::Bytes(b"ALICE".to_vec()));
    }

    #[test]
    fn quote_identifier_rejects_invalid_names() {
        let error = quote_identifier("users; DROP TABLE spot").unwrap_err();
        assert!(error.contains("unsupported identifier"));
    }

    #[test]
    fn load_config_or_default_returns_default_for_missing_file() {
        let config = load_config_or_default("/tmp/datafowk-missing-config.toml").unwrap();

        assert_eq!(config.connection_properties_origin.user, "root");
        assert!(config.rules.is_empty());
    }
}
