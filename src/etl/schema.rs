use mysql::{prelude::Queryable, Conn};
use postgres::Client;

use crate::{
    config::ConnectionProperties,
    models::{TableColumnSchema, TableSchema},
};

use super::connect::DatabaseConnection;

pub(crate) fn preview_schema(
    connection_properties: &ConnectionProperties,
    label: &str,
) -> Result<Vec<TableSchema>, String> {
    let mut connection = super::connect(connection_properties, label)?;

    match &mut connection {
        DatabaseConnection::Mysql(conn) => preview_mysql_schema(conn, connection_properties),
        DatabaseConnection::Postgres(client) => {
            preview_postgres_schema(client, connection_properties)
        }
    }
}

fn preview_mysql_schema(
    connection: &mut Conn,
    connection_properties: &ConnectionProperties,
) -> Result<Vec<TableSchema>, String> {
    let rows: Vec<(String, String, String)> = connection
        .exec(
            "SELECT table_name, column_name, data_type \
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
            "SELECT table_name, column_name, data_type \
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
            let data_type: String = row.get(2);
            (table_name, column_name, data_type)
        })
        .collect();

    group_schema_rows(normalized)
}

fn group_schema_rows(rows: Vec<(String, String, String)>) -> Result<Vec<TableSchema>, String> {
    let mut grouped = Vec::<TableSchema>::new();

    for (table_name, column_name, data_type) in rows {
        if let Some(existing) = grouped.iter_mut().find(|table| table.name == table_name) {
            existing.columns.push(TableColumnSchema {
                name: column_name,
                data_type,
            });
        } else {
            grouped.push(TableSchema {
                name: table_name,
                columns: vec![TableColumnSchema {
                    name: column_name,
                    data_type,
                }],
            });
        }
    }

    Ok(grouped)
}
