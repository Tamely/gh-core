//! Settings come from environment variables. Startup fails if any value is
//! invalid, and every bad value is reported at once rather than one per restart.

use std::{env, fmt::Display, str::FromStr};

#[derive(Debug)]
pub struct Config {
    pub http_port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, Vec<String>> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    /// Takes the lookup as a function so tests can pass fake variables.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let config = Self {
            http_port: optional(&lookup, "HTTP_PORT", 8080, &mut errors),
        };
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
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
    raw.parse().unwrap_or_else(|e| {
        errors.push(format!("{name}={raw:?} is invalid: {e}"));
        default
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_unset() {
        let config = Config::from_lookup(|_| None).unwrap();
        assert_eq!(config.http_port, 8080);
    }

    #[test]
    fn rejects_bad_port() {
        let errors = Config::from_lookup(|_| Some("99999".into())).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("HTTP_PORT="));
    }
}
