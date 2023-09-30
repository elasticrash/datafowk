mod etl_rule_parser;
use mysql::{Conn, Opts};
use serde_derive::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Deserialize, Serialize)]
pub struct ConnectionProperties {
    pub user: String,
    pub password: String,
    pub address: String,
    pub port: u16,
    pub schema: String,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Config {
    pub connection_properties_origin: ConnectionProperties,
    pub connection_properties_destination: ConnectionProperties,
}

impl Default for ConnectionProperties {
    fn default() -> Self {
        ConnectionProperties {
            user: String::from("root"),
            password: String::from("password"),
            address: String::from("127.0.0.1"),
            port: 3306,
            schema: String::from("test"),
        }
    }
}

fn main() {
    let config_file = fs::read_to_string("mysql_config.toml");

    let db_configs: Config = match config_file {
        Ok(data) => toml::from_str(&data).unwrap(),
        Err(_why) => Config::default(),
    };

    let urls = vec![
        format!(
            "mysql://{}:{}@{}:{}/{}",
            &db_configs.connection_properties_origin.user,
            &db_configs.connection_properties_origin.password,
            &db_configs.connection_properties_origin.address,
            &db_configs.connection_properties_origin.port,
            &db_configs.connection_properties_origin.schema
        ),
        format!(
            "mysql://{}:{}@{}:{}/{}",
            &db_configs.connection_properties_destination.user,
            &db_configs.connection_properties_destination.password,
            &db_configs.connection_properties_destination.address,
            &db_configs.connection_properties_destination.port,
            &db_configs.connection_properties_destination.schema
        ),
    ];

    let options_origin = match Opts::from_url(&urls[0]) {
        Ok(data) => data,

        Err(why) => {
            panic!("{}", why);
        }
    };

    let options_destination = match Opts::from_url(&urls[1]) {
        Ok(data) => data,

        Err(why) => {
            panic!("{}", why);
        }
    };

    let mut connection_origin = match Conn::new(options_origin) {
        Ok(con) => con,
        Err(why) => {
            panic!("{}", why);
        }
    };

    let mut connection_destination = match Conn::new(options_destination) {
        Ok(con) => con,
        Err(why) => {
            panic!("{}", why);
        }
    };
}
