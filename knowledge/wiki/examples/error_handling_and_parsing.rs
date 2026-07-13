// EXEMPLAR: idiomatic error handling — custom Error enum, Display, From, and `?` propagation.
// tags: error-handling, result, enum, display, from, question-mark, parsing, config, key-value,
//       dependency-free
//
// Parses `key = value` config lines into a typed struct, converting integer/bool fields with
// proper error propagation. Demonstrates: one error enum that wraps lower-level errors via
// `From`, the `?` operator, and returning `Result` all the way up — no `unwrap` in logic.

use std::collections::HashMap;
use std::num::ParseIntError;

#[derive(Debug, Clone, PartialEq)]
enum ConfigError {
    MalformedLine(String),
    UnknownKey(String),
    BadInt(String),
    BadBool(String),
    MissingKey(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::MalformedLine(line) => write!(f, "malformed line: '{line}' (expected key = value)"),
            ConfigError::UnknownKey(key) => write!(f, "unknown key '{key}'"),
            ConfigError::BadInt(value) => write!(f, "'{value}' is not a valid integer"),
            ConfigError::BadBool(value) => write!(f, "'{value}' is not a valid bool (use true/false)"),
            ConfigError::MissingKey(key) => write!(f, "required key '{key}' is missing"),
        }
    }
}

impl std::error::Error for ConfigError {}

// `From` lets a lower-level error flow through `?` and become our domain error.
impl From<ParseIntError> for ConfigError {
    fn from(error: ParseIntError) -> Self {
        ConfigError::BadInt(error.to_string())
    }
}

#[derive(Debug, PartialEq)]
struct Config {
    port: u16,
    retries: i32,
    verbose: bool,
}

fn parse_bool(value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(ConfigError::BadBool(other.to_string())),
    }
}

fn parse_config(text: &str) -> Result<Config, ConfigError> {
    let mut fields: HashMap<String, String> = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| ConfigError::MalformedLine(line.to_string()))?;
        fields.insert(key.trim().to_string(), value.trim().to_string());
    }

    let take = |key: &'static str| -> Result<&String, ConfigError> {
        fields.get(key).ok_or(ConfigError::MissingKey(key))
    };

    let port: u16 = take("port")?.parse().map_err(|_| ConfigError::BadInt("port".to_string()))?;
    let retries: i32 = take("retries")?.parse()?; // ParseIntError -> ConfigError via From
    let verbose = parse_bool(take("verbose")?)?;

    for key in fields.keys() {
        if !matches!(key.as_str(), "port" | "retries" | "verbose") {
            return Err(ConfigError::UnknownKey(key.clone()));
        }
    }

    Ok(Config { port, retries, verbose })
}

fn main() {
    let sample = "# demo config\nport = 8080\nretries = 3\nverbose = true\n";
    match parse_config(sample) {
        Ok(config) => println!("loaded {config:?}"),
        Err(error) => eprintln!("config error: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_valid_config() {
        let config = parse_config("port = 80\nretries = 5\nverbose = false\n").unwrap();
        assert_eq!(config, Config { port: 80, retries: 5, verbose: false });
    }

    #[test]
    fn surfaces_typed_errors() {
        assert_eq!(parse_config("port = 80\nverbose = true\n"), Err(ConfigError::MissingKey("retries")));
        assert!(matches!(parse_config("oops\n"), Err(ConfigError::MalformedLine(_))));
        assert!(matches!(
            parse_config("port = 80\nretries = x\nverbose = true\n"),
            Err(ConfigError::BadInt(_))
        ));
    }
}
