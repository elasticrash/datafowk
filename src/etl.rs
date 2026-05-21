use std::collections::BTreeSet;
use std::fs;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use mysql::{prelude::Queryable, Conn, Opts, Params, TxOpts, Value};
use postgres::{
    types::{ToSql, Type},
    Client, NoTls, Row as PostgresRow,
};

use crate::config::{Config, ConnectionProperties, DatabaseKind};
use crate::etl_rule_parser::parser::{parse_rule, Rules, SourceJoin};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldReference {
    table: Option<String>,
    field: String,
}

#[derive(Debug, Clone, PartialEq)]
enum DataValue {
    Null,
    String(String),
    I64(i64),
    U64(u64),
    F64(f64),
    Bool(bool),
    Bytes(Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
}

enum DatabaseConnection {
    Mysql(Conn),
    Postgres(Client),
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
Options:\n  --config PATH             Path to the TOML config file (default: {DEFAULT_CONFIG_PATH})\n  --dry-run                 Validate the rules and simulate inserts without persisting\n  --truncate-destination    Truncate destination tables once before loading\n  -h, --help                Show this help message"
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

pub fn preview_schema(
    connection_properties: &ConnectionProperties,
    label: &str,
) -> Result<Vec<TableSchema>, String> {
    let mut connection = connect(connection_properties, label)?;

    match &mut connection {
        DatabaseConnection::Mysql(conn) => preview_mysql_schema(conn, connection_properties),
        DatabaseConnection::Postgres(client) => {
            preview_postgres_schema(client, connection_properties)
        }
    }
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

fn connect(
    connection_properties: &ConnectionProperties,
    label: &str,
) -> Result<DatabaseConnection, String> {
    let mut last_error = None;

    for attempt in 1..=CONNECTION_RETRIES {
        let result = match connection_properties.kind {
            DatabaseKind::Mysql => {
                connect_mysql(connection_properties).map(DatabaseConnection::Mysql)
            }
            DatabaseKind::Postgres => {
                connect_postgres(connection_properties).map(DatabaseConnection::Postgres)
            }
        };

        match result {
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
        last_error.unwrap_or_else(|| String::from("unknown connection error"))
    ))
}

fn connect_mysql(connection_properties: &ConnectionProperties) -> Result<Conn, String> {
    let url = format!(
        "mysql://{}:{}@{}:{}/{}",
        connection_properties.user,
        connection_properties.password,
        connection_properties.address,
        connection_properties.port,
        connection_properties.schema
    );

    let opts = Opts::from_url(&url)
        .map_err(|error| format!("invalid MySQL connection URL `{url}`: {error}"))?;

    Conn::new(opts).map_err(|error| error.to_string())
}

fn connect_postgres(connection_properties: &ConnectionProperties) -> Result<Client, String> {
    let connection_string = format!(
        "host={} port={} user={} password={} dbname={}",
        connection_properties.address,
        connection_properties.port,
        connection_properties.user,
        connection_properties.password,
        connection_properties.schema
    );

    Client::connect(&connection_string, NoTls).map_err(|error| error.to_string())
}

fn simulate_rules(
    source_connection: &mut DatabaseConnection,
    destination_connection: &mut DatabaseConnection,
    config: &Config,
    rules: &[Rules],
    truncate_destination: bool,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    match destination_connection {
        DatabaseConnection::Mysql(conn) => {
            let mut tx = conn.start_transaction(TxOpts::default()).map_err(|error| {
                format!("failed to start dry-run simulation transaction: {error}")
            })?;

            if truncate_destination {
                truncate_destination_tables_mysql(&mut tx, config, rules)?;
            }

            for rule in rules {
                let rows = read_rule_rows(source_connection, config, rule)?;
                summary.rows_read += rows.len();
                simulate_insert_rows_mysql(&mut tx, config, rule, rows, summary)?;
                summary.rules_processed += 1;
            }

            tx.rollback().map_err(|error| {
                format!("failed to rollback dry-run simulation transaction: {error}")
            })?;
        }
        DatabaseConnection::Postgres(client) => {
            let mut tx = client.transaction().map_err(|error| {
                format!("failed to start dry-run simulation transaction: {error}")
            })?;

            if truncate_destination {
                truncate_destination_tables_postgres(&mut tx, config, rules)?;
            }

            for rule in rules {
                let rows = read_rule_rows(source_connection, config, rule)?;
                summary.rows_read += rows.len();
                simulate_insert_rows_postgres(&mut tx, config, rule, rows, summary)?;
                summary.rules_processed += 1;
            }

            tx.rollback().map_err(|error| {
                format!("failed to rollback dry-run simulation transaction: {error}")
            })?;
        }
    }

    Ok(())
}

fn execute_rules(
    source_connection: &mut DatabaseConnection,
    destination_connection: &mut DatabaseConnection,
    config: &Config,
    rules: &[Rules],
    truncate_destination: bool,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    match destination_connection {
        DatabaseConnection::Mysql(conn) => {
            if truncate_destination {
                truncate_destination_tables_mysql(conn, config, rules)?;
            }

            for rule in rules {
                let rows = read_rule_rows(source_connection, config, rule)?;
                summary.rows_read += rows.len();
                insert_rows_mysql(conn, config, rule, rows, summary)?;
                summary.rules_processed += 1;
            }
        }
        DatabaseConnection::Postgres(client) => {
            if truncate_destination {
                truncate_destination_tables_postgres(client, config, rules)?;
            }

            for rule in rules {
                let rows = read_rule_rows(source_connection, config, rule)?;
                summary.rows_read += rows.len();
                insert_rows_postgres(client, config, rule, rows, summary)?;
                summary.rules_processed += 1;
            }
        }
    }

    Ok(())
}

fn read_rule_rows(
    source_connection: &mut DatabaseConnection,
    config: &Config,
    rule: &Rules,
) -> Result<Vec<Vec<DataValue>>, String> {
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
            "rule from `{:?}` to `{}` must map the same number of source and destination fields",
            rule.source_tables, rule.destination_table
        ));
    }

    match source_connection {
        DatabaseConnection::Mysql(conn) => {
            read_rule_rows_mysql(conn, &config.connection_properties_origin, rule)
        }
        DatabaseConnection::Postgres(client) => {
            read_rule_rows_postgres(client, &config.connection_properties_origin, rule)
        }
    }
}

fn read_rule_rows_mysql(
    connection: &mut Conn,
    source_properties: &ConnectionProperties,
    rule: &Rules,
) -> Result<Vec<Vec<DataValue>>, String> {
    let select_statement = build_select_statement(DatabaseKind::Mysql, rule)?;
    let rows: Vec<mysql::Row> = connection
        .query(select_statement.as_str())
        .map_err(|error| {
            format!(
                "failed to read MySQL source rows for `{:?}`: {error}",
                rule.source_tables
            )
        })?;

    rows.into_iter()
        .map(|row| {
            let values = row
                .unwrap()
                .into_iter()
                .map(mysql_value_to_data_value)
                .collect::<Result<Vec<_>, _>>()?;
            transform_values(rule, source_properties, values)
        })
        .collect()
}

fn read_rule_rows_postgres(
    client: &mut Client,
    source_properties: &ConnectionProperties,
    rule: &Rules,
) -> Result<Vec<Vec<DataValue>>, String> {
    let select_statement = build_select_statement(DatabaseKind::Postgres, rule)?;
    let rows = client
        .query(select_statement.as_str(), &[])
        .map_err(|error| {
            format!(
                "failed to read PostgreSQL source rows for `{:?}`: {error}",
                rule.source_tables
            )
        })?;

    rows.into_iter()
        .map(|row| {
            let values = postgres_row_to_data_values(&row)?;
            transform_values(rule, source_properties, values)
        })
        .collect()
}

fn insert_rows_mysql<Q: Queryable>(
    destination: &mut Q,
    config: &Config,
    rule: &Rules,
    rows: Vec<Vec<DataValue>>,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    ensure_matches_database(
        &rule.destination_db,
        &config.connection_properties_destination.schema,
        "destination",
    )?;
    let insert_statement = build_insert_statement(DatabaseKind::Mysql, rule)?;

    for row in rows {
        destination
            .exec_drop(
                insert_statement.as_str(),
                Params::from(data_values_to_mysql_values(row)?),
            )
            .map_err(|error| {
                format!(
                    "failed to insert into MySQL destination table `{}`: {error}",
                    rule.destination_table
                )
            })?;
        summary.rows_inserted += 1;
    }

    Ok(())
}

fn simulate_insert_rows_mysql<Q: Queryable>(
    destination: &mut Q,
    config: &Config,
    rule: &Rules,
    rows: Vec<Vec<DataValue>>,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    ensure_matches_database(
        &rule.destination_db,
        &config.connection_properties_destination.schema,
        "destination",
    )?;
    let insert_statement = build_insert_statement(DatabaseKind::Mysql, rule)?;

    for row in rows {
        destination
            .exec_drop(
                insert_statement.as_str(),
                Params::from(data_values_to_mysql_values(row)?),
            )
            .map_err(|error| {
                format!(
                    "dry-run simulation failed for MySQL destination table `{}`: {error}",
                    rule.destination_table
                )
            })?;
        summary.rows_inserted += 1;
    }

    Ok(())
}

fn insert_rows_postgres(
    destination: &mut Client,
    config: &Config,
    rule: &Rules,
    rows: Vec<Vec<DataValue>>,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    ensure_matches_database(
        &rule.destination_db,
        &config.connection_properties_destination.schema,
        "destination",
    )?;
    let insert_statement = build_insert_statement(DatabaseKind::Postgres, rule)?;

    for row in rows {
        let params = data_values_to_postgres_params(row);
        let refs = params
            .iter()
            .map(|param| param.as_ref())
            .collect::<Vec<_>>();
        destination
            .execute(insert_statement.as_str(), &refs)
            .map_err(|error| {
                format!(
                    "failed to insert into PostgreSQL destination table `{}`: {error}",
                    rule.destination_table
                )
            })?;
        summary.rows_inserted += 1;
    }

    Ok(())
}

fn simulate_insert_rows_postgres(
    destination: &mut postgres::Transaction<'_>,
    config: &Config,
    rule: &Rules,
    rows: Vec<Vec<DataValue>>,
    summary: &mut ExecutionSummary,
) -> Result<(), String> {
    ensure_matches_database(
        &rule.destination_db,
        &config.connection_properties_destination.schema,
        "destination",
    )?;
    let insert_statement = build_insert_statement(DatabaseKind::Postgres, rule)?;

    for row in rows {
        let params = data_values_to_postgres_params(row);
        let refs = params
            .iter()
            .map(|param| param.as_ref())
            .collect::<Vec<_>>();
        destination
            .execute(insert_statement.as_str(), &refs)
            .map_err(|error| {
                format!(
                    "dry-run simulation failed for PostgreSQL destination table `{}`: {error}",
                    rule.destination_table
                )
            })?;
        summary.rows_inserted += 1;
    }

    Ok(())
}

fn truncate_destination_tables_mysql<Q: Queryable>(
    destination: &mut Q,
    config: &Config,
    rules: &[Rules],
) -> Result<(), String> {
    let destination_schema = &config.connection_properties_destination.schema;
    let mut tables = BTreeSet::new();

    for rule in rules {
        ensure_matches_database(&rule.destination_db, destination_schema, "destination")?;
        tables.insert(rule.destination_table.as_str());
    }

    for table in tables {
        destination
            .query_drop(format!(
                "TRUNCATE TABLE {}",
                quote_identifier(DatabaseKind::Mysql, table)?
            ))
            .map_err(|error| {
                format!("failed to truncate MySQL destination table `{table}`: {error}")
            })?;
    }

    Ok(())
}

fn truncate_destination_tables_postgres<E>(
    destination: &mut E,
    config: &Config,
    rules: &[Rules],
) -> Result<(), String>
where
    E: PostgresExec,
{
    let destination_schema = &config.connection_properties_destination.schema;
    let mut tables = BTreeSet::new();

    for rule in rules {
        ensure_matches_database(&rule.destination_db, destination_schema, "destination")?;
        tables.insert(rule.destination_table.as_str());
    }

    for table in tables {
        destination
            .execute_query(&format!(
                "TRUNCATE TABLE {}",
                qualify_identifier(DatabaseKind::Postgres, destination_schema, table)?
            ))
            .map_err(|error| {
                format!("failed to truncate PostgreSQL destination table `{table}`: {error}")
            })?;
    }

    Ok(())
}

trait PostgresExec {
    fn execute_query(&mut self, query: &str) -> Result<(), postgres::Error>;
}

impl PostgresExec for Client {
    fn execute_query(&mut self, query: &str) -> Result<(), postgres::Error> {
        self.batch_execute(query)
    }
}

impl PostgresExec for postgres::Transaction<'_> {
    fn execute_query(&mut self, query: &str) -> Result<(), postgres::Error> {
        self.batch_execute(query)
    }
}

fn build_select_statement(kind: DatabaseKind, rule: &Rules) -> Result<String, String> {
    let fields = rule
        .source_fields
        .iter()
        .map(|field| build_source_field_expression(kind, field, &rule.source_tables))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    let from_clause = build_source_from_clause(kind, rule)?;

    Ok(format!("SELECT {fields} FROM {from_clause}"))
}

fn build_source_from_clause(kind: DatabaseKind, rule: &Rules) -> Result<String, String> {
    let Some(first_table) = rule.source_tables.first() else {
        return Err(String::from("at least one source table is required"));
    };

    let mut joined_tables = vec![first_table.clone()];
    let mut remaining_conditions = rule.join_conditions.clone();
    let mut from_clause = quote_identifier(kind, first_table)?;

    for table in rule.source_tables.iter().skip(1) {
        let mut join_conditions = Vec::new();
        let mut next_remaining = Vec::new();

        for condition in remaining_conditions {
            if join_condition_reaches_joined_table(&condition, table, &joined_tables) {
                join_conditions.push(condition);
            } else {
                next_remaining.push(condition);
            }
        }

        if join_conditions.is_empty() {
            return Err(format!(
                "source table `{table}` is not connected to the existing join path"
            ));
        }

        from_clause.push_str(&format!(" JOIN {} ON ", quote_identifier(kind, table)?));
        from_clause.push_str(
            &join_conditions
                .iter()
                .map(|condition| join_condition_to_sql(kind, condition))
                .collect::<Result<Vec<_>, _>>()?
                .join(" AND "),
        );

        joined_tables.push(table.clone());
        remaining_conditions = next_remaining;
    }

    if !remaining_conditions.is_empty() {
        from_clause.push_str(" WHERE ");
        from_clause.push_str(
            &remaining_conditions
                .iter()
                .map(|condition| join_condition_to_sql(kind, condition))
                .collect::<Result<Vec<_>, _>>()?
                .join(" AND "),
        );
    }

    Ok(from_clause)
}

fn join_condition_reaches_joined_table(
    condition: &SourceJoin,
    current_table: &str,
    joined_tables: &[String],
) -> bool {
    (condition.left_table == current_table
        && joined_tables
            .iter()
            .any(|table| table == &condition.right_table))
        || (condition.right_table == current_table
            && joined_tables
                .iter()
                .any(|table| table == &condition.left_table))
}

fn join_condition_to_sql(kind: DatabaseKind, condition: &SourceJoin) -> Result<String, String> {
    Ok(format!(
        "{} = {}",
        qualify_identifier(kind, &condition.left_table, &condition.left_field)?,
        qualify_identifier(kind, &condition.right_table, &condition.right_field)?
    ))
}

fn build_source_field_expression(
    kind: DatabaseKind,
    field: &str,
    source_tables: &[String],
) -> Result<String, String> {
    let reference = parse_source_field_reference(field, source_tables)?;
    match reference.table {
        Some(table) => qualify_identifier(kind, &table, &reference.field),
        None => quote_identifier(kind, &reference.field),
    }
}

fn parse_source_field_reference(
    field: &str,
    source_tables: &[String],
) -> Result<FieldReference, String> {
    if let Some((table, column)) = field.split_once('.') {
        let table = table.trim().to_string();
        let column = column.trim().to_string();

        if !source_tables
            .iter()
            .any(|source_table| source_table == &table)
        {
            return Err(format!(
                "source field `{field}` references unknown source table `{table}`"
            ));
        }

        if column.is_empty() {
            return Err(format!("source field `{field}` is missing a column name"));
        }

        Ok(FieldReference {
            table: Some(table),
            field: column,
        })
    } else if source_tables.len() == 1 {
        Ok(FieldReference {
            table: None,
            field: field.trim().to_string(),
        })
    } else {
        Err(format!(
            "source field `{field}` must use `table.column` when multiple source tables are configured"
        ))
    }
}

fn build_insert_statement(kind: DatabaseKind, rule: &Rules) -> Result<String, String> {
    let columns = rule
        .destination_fields
        .iter()
        .map(|field| quote_identifier(kind, field))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    let placeholders = match kind {
        DatabaseKind::Mysql => vec!["?".to_string(); rule.destination_fields.len()],
        DatabaseKind::Postgres => (1..=rule.destination_fields.len())
            .map(|index| format!("${index}"))
            .collect(),
    }
    .join(", ");

    let table_name = match kind {
        DatabaseKind::Mysql => quote_identifier(kind, &rule.destination_table)?,
        DatabaseKind::Postgres => quote_identifier(kind, &rule.destination_table)?,
    };

    Ok(format!(
        "INSERT INTO {table_name} ({columns}) VALUES ({placeholders})"
    ))
}

fn quote_identifier(kind: DatabaseKind, identifier: &str) -> Result<String, String> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(format!(
            "unsupported identifier `{identifier}`; only letters, numbers, and underscores are allowed"
        ));
    }

    Ok(match kind {
        DatabaseKind::Mysql => format!("`{identifier}`"),
        DatabaseKind::Postgres => format!("\"{identifier}\""),
    })
}

fn qualify_identifier(kind: DatabaseKind, table: &str, field: &str) -> Result<String, String> {
    Ok(format!(
        "{}.{}",
        quote_identifier(kind, table)?,
        quote_identifier(kind, field)?
    ))
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

fn transform_values(
    rule: &Rules,
    source_properties: &ConnectionProperties,
    mut values: Vec<DataValue>,
) -> Result<Vec<DataValue>, String> {
    if values.len() != rule.source_fields.len() {
        return Err(format!(
            "source query for tables `{:?}` returned {} columns but the rule expects {}",
            rule.source_tables,
            values.len(),
            rule.source_fields.len()
        ));
    }

    for function_name in &rule.function_chain {
        for value in &mut values {
            apply_function(value, function_name, source_properties.kind)?;
        }
    }

    Ok(values)
}

fn apply_function(
    value: &mut DataValue,
    function_name: &str,
    source_kind: DatabaseKind,
) -> Result<(), String> {
    match function_name {
        "copy" | "identity" => Ok(()),
        "trim" => transform_string_value(value, source_kind, |text| text.trim().to_string()),
        "lowercase" => transform_string_value(value, source_kind, |text| text.to_lowercase()),
        "uppercase" => transform_string_value(value, source_kind, |text| text.to_uppercase()),
        unknown => Err(format!("unsupported transformation function `{unknown}`")),
    }
}

fn transform_string_value<F>(
    value: &mut DataValue,
    source_kind: DatabaseKind,
    transformer: F,
) -> Result<(), String>
where
    F: FnOnce(&str) -> String,
{
    match value {
        DataValue::String(text) => {
            *text = transformer(text);
        }
        DataValue::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes)
                .map_err(|error| format!("string transformation requires UTF-8 data: {error}"))?;
            *value = DataValue::String(transformer(text));
        }
        DataValue::Null
        | DataValue::I64(_)
        | DataValue::U64(_)
        | DataValue::F64(_)
        | DataValue::Bool(_)
        | DataValue::Date(_)
        | DataValue::Time(_)
        | DataValue::DateTime(_) => {
            let _ = source_kind;
        }
    }

    Ok(())
}

fn mysql_value_to_data_value(value: Value) -> Result<DataValue, String> {
    match value {
        Value::NULL => Ok(DataValue::Null),
        Value::Bytes(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => Ok(DataValue::String(text)),
            Err(_) => Ok(DataValue::Bytes(bytes)),
        },
        Value::Int(value) => Ok(DataValue::I64(value)),
        Value::UInt(value) => Ok(DataValue::U64(value)),
        Value::Float(value) => Ok(DataValue::F64(value as f64)),
        Value::Double(value) => Ok(DataValue::F64(value)),
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if hour == 0 && minute == 0 && second == 0 && micros == 0 {
                NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
                    .map(DataValue::Date)
                    .ok_or_else(|| String::from("invalid MySQL date value"))
            } else {
                let date = NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
                    .ok_or_else(|| String::from("invalid MySQL date value"))?;
                let time = NaiveTime::from_hms_micro_opt(
                    hour as u32,
                    minute as u32,
                    second as u32,
                    micros,
                )
                .ok_or_else(|| String::from("invalid MySQL datetime value"))?;
                Ok(DataValue::DateTime(NaiveDateTime::new(date, time)))
            }
        }
        Value::Time(negative, days, hours, minutes, seconds, micros) => {
            if negative || days > 0 {
                Err(String::from(
                    "MySQL TIME values with negative or multi-day durations are not supported yet",
                ))
            } else {
                NaiveTime::from_hms_micro_opt(hours as u32, minutes as u32, seconds as u32, micros)
                    .map(DataValue::Time)
                    .ok_or_else(|| String::from("invalid MySQL time value"))
            }
        }
    }
}

fn postgres_row_to_data_values(row: &PostgresRow) -> Result<Vec<DataValue>, String> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, column)| postgres_cell_to_data_value(row, index, column.type_()))
        .collect()
}

fn postgres_cell_to_data_value(
    row: &PostgresRow,
    index: usize,
    ty: &Type,
) -> Result<DataValue, String> {
    match *ty {
        Type::BOOL => Ok(row
            .try_get::<_, Option<bool>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Bool)
            .unwrap_or(DataValue::Null)),
        Type::INT2 => Ok(row
            .try_get::<_, Option<i16>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::I64(value as i64))
            .unwrap_or(DataValue::Null)),
        Type::INT4 | Type::OID => Ok(row
            .try_get::<_, Option<i32>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::I64(value as i64))
            .unwrap_or(DataValue::Null)),
        Type::INT8 => Ok(row
            .try_get::<_, Option<i64>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::I64)
            .unwrap_or(DataValue::Null)),
        Type::FLOAT4 => Ok(row
            .try_get::<_, Option<f32>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::F64(value as f64))
            .unwrap_or(DataValue::Null)),
        Type::FLOAT8 => Ok(row
            .try_get::<_, Option<f64>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::F64)
            .unwrap_or(DataValue::Null)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => Ok(row
            .try_get::<_, Option<String>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::String)
            .unwrap_or(DataValue::Null)),
        Type::BYTEA => Ok(row
            .try_get::<_, Option<Vec<u8>>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Bytes)
            .unwrap_or(DataValue::Null)),
        Type::DATE => Ok(row
            .try_get::<_, Option<NaiveDate>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Date)
            .unwrap_or(DataValue::Null)),
        Type::TIME => Ok(row
            .try_get::<_, Option<NaiveTime>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::Time)
            .unwrap_or(DataValue::Null)),
        Type::TIMESTAMP => Ok(row
            .try_get::<_, Option<NaiveDateTime>>(index)
            .map_err(|error| error.to_string())?
            .map(DataValue::DateTime)
            .unwrap_or(DataValue::Null)),
        Type::TIMESTAMPTZ => Ok(row
            .try_get::<_, Option<DateTime<Utc>>>(index)
            .map_err(|error| error.to_string())?
            .map(|value| DataValue::DateTime(value.naive_utc()))
            .unwrap_or(DataValue::Null)),
        _ => Err(format!(
            "unsupported PostgreSQL source type `{}`",
            ty.name()
        )),
    }
}

fn data_values_to_mysql_values(values: Vec<DataValue>) -> Result<Vec<Value>, String> {
    values.into_iter().map(data_value_to_mysql_value).collect()
}

fn data_value_to_mysql_value(value: DataValue) -> Result<Value, String> {
    match value {
        DataValue::Null => Ok(Value::NULL),
        DataValue::String(text) => Ok(Value::Bytes(text.into_bytes())),
        DataValue::I64(value) => Ok(Value::Int(value)),
        DataValue::U64(value) => Ok(Value::UInt(value)),
        DataValue::F64(value) => Ok(Value::Double(value)),
        DataValue::Bool(value) => Ok(Value::Int(if value { 1 } else { 0 })),
        DataValue::Bytes(bytes) => Ok(Value::Bytes(bytes)),
        DataValue::Date(value) => Ok(Value::Date(
            value.year() as u16,
            value.month() as u8,
            value.day() as u8,
            0,
            0,
            0,
            0,
        )),
        DataValue::Time(value) => Ok(Value::Time(
            false,
            0,
            value.hour() as u8,
            value.minute() as u8,
            value.second() as u8,
            value.nanosecond() / 1_000,
        )),
        DataValue::DateTime(value) => Ok(Value::Date(
            value.date().year() as u16,
            value.date().month() as u8,
            value.date().day() as u8,
            value.time().hour() as u8,
            value.time().minute() as u8,
            value.time().second() as u8,
            value.time().nanosecond() / 1_000,
        )),
    }
}

type PgParam = Box<dyn ToSql + Sync>;

fn data_values_to_postgres_params(values: Vec<DataValue>) -> Vec<PgParam> {
    values
        .into_iter()
        .map(|value| match value {
            DataValue::Null => Box::new(Option::<String>::None) as PgParam,
            DataValue::String(text) => Box::new(text) as PgParam,
            DataValue::I64(value) => Box::new(value) as PgParam,
            DataValue::U64(value) => {
                if value <= i64::MAX as u64 {
                    Box::new(value as i64) as PgParam
                } else {
                    Box::new(value.to_string()) as PgParam
                }
            }
            DataValue::F64(value) => Box::new(value) as PgParam,
            DataValue::Bool(value) => Box::new(value) as PgParam,
            DataValue::Bytes(bytes) => Box::new(bytes) as PgParam,
            DataValue::Date(value) => Box::new(value) as PgParam,
            DataValue::Time(value) => Box::new(value) as PgParam,
            DataValue::DateTime(value) => Box::new(value) as PgParam,
        })
        .collect()
}

fn preview_mysql_schema(
    connection: &mut Conn,
    connection_properties: &ConnectionProperties,
) -> Result<Vec<TableSchema>, String> {
    let rows: Vec<(String, String)> = connection
        .exec(
            "SELECT table_name, column_name \
             FROM information_schema.columns \
             WHERE table_schema = ? \
             ORDER BY table_name, ordinal_position",
            (&connection_properties.schema,),
        )
        .map_err(|error| format!("failed to inspect MySQL schema: {error}"))?;

    group_schema_rows(rows)
}

fn preview_postgres_schema(
    client: &mut Client,
    connection_properties: &ConnectionProperties,
) -> Result<Vec<TableSchema>, String> {
    let rows = client
        .query(
            "SELECT table_name, column_name \
             FROM information_schema.columns \
             WHERE table_schema = $1 \
             ORDER BY table_name, ordinal_position",
            &[&connection_properties.schema],
        )
        .map_err(|error| format!("failed to inspect PostgreSQL schema: {error}"))?;

    let normalized = rows
        .into_iter()
        .map(|row| {
            let table_name: String = row.get(0);
            let column_name: String = row.get(1);
            (table_name, column_name)
        })
        .collect();

    group_schema_rows(normalized)
}

fn group_schema_rows(rows: Vec<(String, String)>) -> Result<Vec<TableSchema>, String> {
    let mut grouped = Vec::<TableSchema>::new();

    for (table_name, column_name) in rows {
        if let Some(existing) = grouped.iter_mut().find(|table| table.name == table_name) {
            existing.columns.push(column_name);
        } else {
            grouped.push(TableSchema {
                name: table_name,
                columns: vec![column_name],
            });
        }
    }

    Ok(grouped)
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

        apply_function(&mut value, "trim", DatabaseKind::Mysql).unwrap();
        apply_function(&mut value, "uppercase", DatabaseKind::Mysql).unwrap();

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
