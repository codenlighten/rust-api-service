//! Configuration from environment variables, validated at startup.

use std::net::SocketAddr;

const MIN_SECRET_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub token_secret: Vec<u8>,
    pub token_ttl_secs: u64,
    pub db_max_connections: u32,
    pub json_logs: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ConfigError(String);

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Reads settings through `get`, so tests do not touch the process environment.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let secret = get("TOKEN_SECRET").ok_or_else(|| {
            ConfigError(
                "TOKEN_SECRET is not set; set it to a random value of at least 32 bytes \
                 (for example: openssl rand -hex 32)"
                    .into(),
            )
        })?;
        if secret.len() < MIN_SECRET_BYTES {
            return Err(ConfigError(format!(
                "TOKEN_SECRET is {} bytes; it must be at least {MIN_SECRET_BYTES}",
                secret.len()
            )));
        }

        let bind = get("BIND_ADDR").unwrap_or_else(|| "127.0.0.1:3000".into());
        let bind_addr = bind.parse().map_err(|_| {
            ConfigError(format!(
                "BIND_ADDR '{bind}' is not a host:port such as 127.0.0.1:3000"
            ))
        })?;

        Ok(Self {
            database_url: get("DATABASE_URL")
                .unwrap_or_else(|| "sqlite://notes.sqlite?mode=rwc".into()),
            bind_addr,
            token_secret: secret.into_bytes(),
            token_ttl_secs: parse_num(&get, "TOKEN_TTL_SECS", 3600, 1, 30 * 24 * 3600)?,
            db_max_connections: parse_num(&get, "DB_MAX_CONNECTIONS", 8, 1, 64)? as u32,
            json_logs: get("LOG_FORMAT").is_some_and(|v| v == "json"),
        })
    }
}

fn parse_num(
    get: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, ConfigError> {
    match get(key) {
        None => Ok(default),
        Some(raw) => match raw.parse::<u64>() {
            Ok(n) if (min..=max).contains(&n) => Ok(n),
            _ => Err(ConfigError(format!(
                "{key} '{raw}' must be an integer from {min} to {max}"
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.to_string())
        }
    }

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn defaults_apply_when_only_the_secret_is_set() {
        let cfg = Config::from_lookup(lookup(&[("TOKEN_SECRET", SECRET)])).unwrap();
        assert_eq!(cfg.bind_addr.to_string(), "127.0.0.1:3000");
        assert_eq!(cfg.token_ttl_secs, 3600);
        assert!(!cfg.json_logs);
    }

    #[test]
    fn missing_secret_says_how_to_fix_it() {
        let err = Config::from_lookup(lookup(&[])).unwrap_err();
        assert!(err.to_string().contains("openssl rand -hex 32"));
    }

    #[test]
    fn short_secret_is_rejected() {
        let err = Config::from_lookup(lookup(&[("TOKEN_SECRET", "short")])).unwrap_err();
        assert!(err.to_string().contains("at least 32"));
    }

    #[test]
    fn out_of_range_ttl_is_rejected() {
        let err = Config::from_lookup(lookup(&[("TOKEN_SECRET", SECRET), ("TOKEN_TTL_SECS", "0")]))
            .unwrap_err();
        assert!(err.to_string().contains("TOKEN_TTL_SECS"));
    }

    #[test]
    fn bad_bind_address_is_rejected() {
        let err = Config::from_lookup(lookup(&[("TOKEN_SECRET", SECRET), ("BIND_ADDR", "nope")]))
            .unwrap_err();
        assert!(err.to_string().contains("BIND_ADDR"));
    }
}
