use mysql::prelude::Queryable;

use crate::{
    config::ConnectionProperties,
    models::{DataValue, Rules},
};

use super::{
    connect::{connect, DatabaseConnection},
    sql::build_select_statement,
    values::{mysql_value_to_data_value, postgres_row_to_data_values},
};

/// Fetches up to `limit` rows from the origin database for the given rule.
pub(crate) fn fetch_data_preview(
    connection_properties: &ConnectionProperties,
    rule: &Rules,
    limit: usize,
) -> Result<Vec<Vec<DataValue>>, String> {
    let select = build_select_statement(connection_properties.kind, rule)?;
    let limited = format!("{select} LIMIT {limit}");

    let mut connection = connect(connection_properties, "origin (geometry preview)")?;

    match &mut connection {
        DatabaseConnection::Mysql(conn) => {
            let rows: Vec<mysql::Row> = conn
                .query(&limited)
                .map_err(|e| format!("data preview query failed: {e}"))?;
            rows.into_iter()
                .map(|row| {
                    row.unwrap()
                        .into_iter()
                        .map(mysql_value_to_data_value)
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect()
        }
        DatabaseConnection::Postgres(client) => {
            let rows = client
                .query(limited.as_str(), &[])
                .map_err(|e| format!("data preview query failed: {e}"))?;
            rows.iter().map(postgres_row_to_data_values).collect()
        }
    }
}
