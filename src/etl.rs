#[path = "etl/config_io.rs"]
mod config_io;
#[path = "etl/connect.rs"]
mod connect;
#[path = "etl/engine.rs"]
mod engine;
#[path = "etl/schema.rs"]
mod schema;
#[path = "etl/sql.rs"]
mod sql;
#[path = "etl/values.rs"]
mod values;

use crate::{
    config::Config,
    etl_rule_parser::parser::parse_rule,
    models::{CliOptions, ExecutionSummary},
};
use config_io::load_config;
pub(crate) use config_io::{load_config_or_default, save_config};
use connect::connect;
use engine::{execute_rules, simulate_rules};
pub(crate) use schema::preview_schema;

const DUPLICATE_LOG_PATH: &str = "datafowk-skipped-duplicates.log";

pub fn run(options: CliOptions) -> Result<ExecutionSummary, String> {
    let config = load_config(&options.config_path)?;
    run_config(&config, options.dry_run, options.truncate_destination)
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mysql_sql_statements_quote_identifiers() {
        let rule =
            parse_rule("(origin:users)[firstname,lastname]<copy>(destination:spot)[name,surname]")
                .unwrap();

        assert_eq!(
            build_select_statement(DatabaseKind::Mysql, &rule).unwrap(),
            "SELECT `firstname`, `lastname` FROM `users`"
        );
        assert_eq!(
            build_insert_statement(DatabaseKind::Mysql, &rule).unwrap(),
            "INSERT INTO `spot` (`name`, `surname`) VALUES (?, ?)"
        );
    }

    #[test]
    fn build_postgres_sql_statements_use_postgres_quoting() {
        let rule =
            parse_rule("(origin:users)[firstname,lastname]<copy>(destination:spot)[name,surname]")
                .unwrap();

        assert_eq!(
            build_select_statement(DatabaseKind::Postgres, &rule).unwrap(),
            "SELECT \"firstname\", \"lastname\" FROM \"users\""
        );
        assert_eq!(
            build_insert_statement(DatabaseKind::Postgres, &rule).unwrap(),
            "INSERT INTO \"spot\" (\"name\", \"surname\") VALUES ($1, $2)"
        );
    }

    #[test]
    fn build_sql_statements_support_joined_sources() {
        let rule = parse_rule(
            "(origin:users,address){users.address_id=address.id}[users.firstname,address.address,address.number]<trim>(destination:spot)[name,address,number]",
        )
        .unwrap();

        assert_eq!(
            build_select_statement(DatabaseKind::Mysql, &rule).unwrap(),
            "SELECT `users`.`firstname`, `address`.`address`, `address`.`number` FROM `users` JOIN `address` ON `users`.`address_id` = `address`.`id`"
        );
    }

    #[test]
    fn apply_function_transforms_string_values() {
        let mut value = DataValue::String(String::from("  Alice  "));

        apply_transform(
            &mut value,
            &crate::models::RuleTransform {
                name: String::from("trim"),
                arguments: Vec::new(),
            },
            DatabaseKind::Mysql,
        )
        .unwrap();
        apply_transform(
            &mut value,
            &crate::models::RuleTransform {
                name: String::from("uppercase"),
                arguments: Vec::new(),
            },
            DatabaseKind::Mysql,
        )
        .unwrap();

        assert_eq!(value, DataValue::String(String::from("ALICE")));
    }

    #[test]
    fn quote_identifier_rejects_invalid_names() {
        let error = quote_identifier(DatabaseKind::Mysql, "users; DROP TABLE spot").unwrap_err();
        assert!(error.contains("unsupported identifier"));
    }

    #[test]
    fn load_config_or_default_returns_default_for_missing_file() {
        let config = load_config_or_default("/tmp/datafowk-missing-config.toml").unwrap();

        assert_eq!(config.connection_properties_origin.user, "root");
        assert_eq!(
            config.connection_properties_origin.kind,
            DatabaseKind::Mysql
        );
        assert!(config.rules.is_empty());
    }
}
