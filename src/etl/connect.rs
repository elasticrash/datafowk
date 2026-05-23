use std::thread;
use std::time::Duration;

use mysql::{Conn, Opts};
use postgres::{Client, NoTls};

use crate::config::{ConnectionProperties, DatabaseKind};

const CONNECTION_RETRIES: usize = 10;
const CONNECTION_RETRY_DELAY_MS: u64 = 1_000;

pub(super) enum DatabaseConnection {
    Mysql(Conn),
    Postgres(Client),
}

pub(super) fn connect(
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
