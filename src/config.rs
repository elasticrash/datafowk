use serde_derive::{Deserialize, Serialize};

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
