use std::fs;

use crate::config::Config;

pub(crate) fn load_config(path: &str) -> Result<Config, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read config `{path}`: {error}"))?;

    toml::from_str(&contents).map_err(|error| format!("failed to parse config `{path}`: {error}"))
}

pub(crate) fn load_config_or_default(path: &str) -> Result<Config, String> {
    match fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents)
            .map_err(|error| format!("failed to parse config `{path}`: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(format!("failed to read config `{path}`: {error}")),
    }
}

pub(crate) fn save_config(path: &str, config: &Config) -> Result<(), String> {
    let contents = toml::to_string_pretty(config)
        .map_err(|error| format!("failed to serialize config `{path}`: {error}"))?;

    fs::write(path, contents).map_err(|error| format!("failed to write config `{path}`: {error}"))
}
