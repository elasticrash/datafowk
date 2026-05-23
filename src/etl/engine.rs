use std::collections::BTreeSet;

use mysql::{prelude::Queryable, Conn, Params, TxOpts};
use postgres::{types::ToSql, Client};

use crate::{
    config::{Config, ConnectionProperties, DatabaseKind},
    models::{DataValue, ExecutionSummary, Rules},
    transforms::unique_destination_field_indexes,
};

use super::{
    connect::DatabaseConnection,
    sql::{
        build_insert_statement, build_select_statement, ensure_matches_database,
        qualify_identifier, quote_identifier,
    },
    values::{
        append_duplicate_log, data_values_to_mysql_values, data_values_to_postgres_params,
        mysql_value_to_data_value, postgres_row_to_data_values, transform_values,
    },
};

pub(super) fn simulate_rules(
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

pub(super) fn execute_rules(
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

fn build_duplicate_check_statement(
    kind: DatabaseKind,
    rule: &Rules,
    row: &[DataValue],
    unique_indexes: &[usize],
) -> Result<(String, Vec<DataValue>), String> {
    let mut conditions = Vec::new();
    let mut params = Vec::new();

    for index in unique_indexes {
        let field = &rule.destination_fields[*index];
        if matches!(row[*index], DataValue::Null) {
            conditions.push(format!("{} IS NULL", quote_identifier(kind, field)?));
        } else {
            let placeholder = match kind {
                DatabaseKind::Mysql => String::from("?"),
                DatabaseKind::Postgres => format!("${}", params.len() + 1),
            };
            conditions.push(format!(
                "{} = {}",
                quote_identifier(kind, field)?,
                placeholder
            ));
            params.push(row[*index].clone());
        }
    }

    let table_name = quote_identifier(kind, &rule.destination_table)?;
    Ok((
        format!(
            "SELECT 1 FROM {table_name} WHERE {} LIMIT 1",
            conditions.join(" AND ")
        ),
        params,
    ))
}

fn should_skip_duplicate_mysql<Q: Queryable>(
    destination: &mut Q,
    rule: &Rules,
    row: &[DataValue],
    unique_indexes: &[usize],
) -> Result<bool, String> {
    let (statement, params) =
        build_duplicate_check_statement(DatabaseKind::Mysql, rule, row, unique_indexes)?;
    let exists = destination
        .exec_first::<u8, _, _>(
            statement.as_str(),
            Params::from(data_values_to_mysql_values(params)?),
        )
        .map_err(|error| {
            format!(
                "failed to check duplicate rows for MySQL destination table `{}`: {error}",
                rule.destination_table
            )
        })?;

    Ok(exists.is_some())
}

fn should_skip_duplicate_postgres<E>(
    destination: &mut E,
    rule: &Rules,
    row: &[DataValue],
    unique_indexes: &[usize],
) -> Result<bool, String>
where
    E: PostgresQuery,
{
    let (statement, params) =
        build_duplicate_check_statement(DatabaseKind::Postgres, rule, row, unique_indexes)?;
    let params = data_values_to_postgres_params(params);
    let refs = params
        .iter()
        .map(|param| param.as_ref())
        .collect::<Vec<_>>();

    destination
        .row_exists(statement.as_str(), &refs)
        .map_err(|error| {
            format!(
                "failed to check duplicate rows for PostgreSQL destination table `{}`: {error}",
                rule.destination_table
            )
        })
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
    let unique_indexes = unique_destination_field_indexes(rule)?;

    for row in rows {
        if let Some(unique_indexes) = &unique_indexes {
            if should_skip_duplicate_mysql(destination, rule, &row, unique_indexes)? {
                append_duplicate_log(rule, &row, unique_indexes)?;
                summary.rows_skipped += 1;
                continue;
            }
        }
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
    let unique_indexes = unique_destination_field_indexes(rule)?;

    for row in rows {
        if let Some(unique_indexes) = &unique_indexes {
            if should_skip_duplicate_mysql(destination, rule, &row, unique_indexes)? {
                summary.rows_skipped += 1;
                continue;
            }
        }
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
    let unique_indexes = unique_destination_field_indexes(rule)?;

    for row in rows {
        if let Some(unique_indexes) = &unique_indexes {
            if should_skip_duplicate_postgres(destination, rule, &row, unique_indexes)? {
                append_duplicate_log(rule, &row, unique_indexes)?;
                summary.rows_skipped += 1;
                continue;
            }
        }
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
    let unique_indexes = unique_destination_field_indexes(rule)?;

    for row in rows {
        if let Some(unique_indexes) = &unique_indexes {
            if should_skip_duplicate_postgres(destination, rule, &row, unique_indexes)? {
                summary.rows_skipped += 1;
                continue;
            }
        }
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

trait PostgresQuery {
    fn row_exists(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<bool, postgres::Error>;
}

impl PostgresExec for Client {
    fn execute_query(&mut self, query: &str) -> Result<(), postgres::Error> {
        self.batch_execute(query)
    }
}

impl PostgresQuery for Client {
    fn row_exists(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<bool, postgres::Error> {
        self.query_opt(query, params).map(|row| row.is_some())
    }
}

impl PostgresExec for postgres::Transaction<'_> {
    fn execute_query(&mut self, query: &str) -> Result<(), postgres::Error> {
        self.batch_execute(query)
    }
}

impl PostgresQuery for postgres::Transaction<'_> {
    fn row_exists(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<bool, postgres::Error> {
        self.query_opt(query, params).map(|row| row.is_some())
    }
}
