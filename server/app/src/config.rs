//! Settings come from environment variables. Startup fails if any value is
//! missing or invalid, and every bad value is reported at once rather than one
//! per restart.

use std::{env, fmt::Display, str::FromStr, time::Duration};

#[derive(Debug)]
pub struct Config {
    pub http_port: u16,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    /// After SIGTERM, how long `/readyz` reports 503 before the server stops
    /// accepting connections, so the load balancer has time to notice.
    pub shutdown_delay: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self, Vec<String>> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    /// Takes the lookup as a function so tests can pass fake variables.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let config = Self {
            http_port: optional(&lookup, "HTTP_PORT", 8080, &mut errors),
            database_url: required(&lookup, "DATABASE_URL", &mut errors),
            db_max_connections: optional(&lookup, "DB_MAX_CONNECTIONS", 20, &mut errors),
            db_min_connections: optional(&lookup, "DB_MIN_CONNECTIONS", 2, &mut errors),
            shutdown_delay: Duration::from_millis(optional(
                &lookup,
                "SHUTDOWN_DELAY_MS",
                5000,
                &mut errors,
            )),
        };

        if config.db_max_connections == 0 {
            errors.push("DB_MAX_CONNECTIONS must be at least 1".into());
        }
        if config.db_min_connections > config.db_max_connections {
            errors.push("DB_MIN_CONNECTIONS can't be larger than DB_MAX_CONNECTIONS".into());
        }
        if config.shutdown_delay >= crate::DRAIN_TIMEOUT {
            errors.push(format!(
                "SHUTDOWN_DELAY_MS must be below the {}s drain limit",
                crate::DRAIN_TIMEOUT.as_secs()
            ));
        }

        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }
}

fn required(
    lookup: impl Fn(&str) -> Option<String>,
    name: &str,
    errors: &mut Vec<String>,
) -> String {
    match lookup(name) {
        Some(value) if !value.trim().is_empty() => value,
        _ => {
            errors.push(format!("{name} is required"));
            String::new()
        }
    }
}

/// Parses `name`, using `default` when it isn't set. A value that is set but
/// doesn't parse is recorded in `errors`.
fn optional<T>(
    lookup: impl Fn(&str) -> Option<String>,
    name: &str,
    default: T,
    errors: &mut Vec<String>,
) -> T
where
    T: FromStr,
    T::Err: Display,
{
    let Some(raw) = lookup(name) else {
        return default;
    };
    raw.trim().parse().unwrap_or_else(|e| {
        errors.push(format!("{name}={raw:?} is invalid: {e}"));
        default
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    const DB: (&str, &str) = ("DATABASE_URL", "postgres://localhost/gh");

    #[test]
    fn defaults_when_only_required_is_set() {
        let config = Config::from_lookup(vars(&[DB])).unwrap();
        assert_eq!(config.http_port, 8080);
        assert_eq!(config.db_max_connections, 20);
        assert_eq!(config.db_min_connections, 2);
        assert_eq!(config.shutdown_delay, Duration::from_secs(5));
    }

    #[test]
    fn missing_database_url_is_an_error() {
        let errors = Config::from_lookup(vars(&[])).unwrap_err();
        assert_eq!(errors, ["DATABASE_URL is required"]);
    }

    #[test]
    fn blank_database_url_is_an_error() {
        let errors = Config::from_lookup(vars(&[("DATABASE_URL", "  ")])).unwrap_err();
        assert_eq!(errors, ["DATABASE_URL is required"]);
    }

    #[test]
    fn reports_every_bad_value_at_once() {
        let errors = Config::from_lookup(vars(&[
            ("HTTP_PORT", "99999"),
            ("DB_MAX_CONNECTIONS", "lots"),
        ]))
        .unwrap_err();
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors[0].starts_with("HTTP_PORT="));
        assert_eq!(errors[1], "DATABASE_URL is required");
        assert!(errors[2].starts_with("DB_MAX_CONNECTIONS="));
    }

    #[test]
    fn rejects_zero_and_inverted_pool_sizes() {
        let errors = Config::from_lookup(vars(&[DB, ("DB_MAX_CONNECTIONS", "0")])).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("at least 1")),
            "{errors:?}"
        );

        let errors = Config::from_lookup(vars(&[
            DB,
            ("DB_MAX_CONNECTIONS", "4"),
            ("DB_MIN_CONNECTIONS", "5"),
        ]))
        .unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("larger than")),
            "{errors:?}"
        );
    }

    #[test]
    fn rejects_negative_numbers() {
        let errors = Config::from_lookup(vars(&[DB, ("DB_MIN_CONNECTIONS", "-1")])).unwrap_err();
        assert!(errors[0].starts_with("DB_MIN_CONNECTIONS="), "{errors:?}");
    }

    #[test]
    fn shutdown_delay_must_fit_inside_drain_limit() {
        let errors = Config::from_lookup(vars(&[DB, ("SHUTDOWN_DELAY_MS", "15000")])).unwrap_err();
        assert!(errors[0].contains("drain limit"), "{errors:?}");
    }

    #[test]
    fn trims_whitespace_around_numbers() {
        let config = Config::from_lookup(vars(&[DB, ("HTTP_PORT", " 9000 ")])).unwrap();
        assert_eq!(config.http_port, 9000);
    }
}
