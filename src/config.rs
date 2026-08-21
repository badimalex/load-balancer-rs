use std::env;

#[derive(Debug)]
pub struct Config {
    pub listen_addr: String,
    pub backends: Vec<String>,
    pub upstream_timeout_ms: u64,
    pub health_check_interval_ms: u64,
}

#[derive(Debug)]
pub enum ConfigError {
    NotFound(String),
    InvalidFormat(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NotFound(var) => write!(f, "Пропущена обязательная переменная: {var}"),
            ConfigError::InvalidFormat(var) => {
                write!(f, "Неверный формат значения в переменной: {var}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn parse(
        listen_addr: String,
        backends_raw: String,
        upstream_timeout_raw: String,
        health_interval_raw: String,
    ) -> Result<Self, ConfigError> {
        let trimmed = listen_addr.trim();

        if trimmed.is_empty() {
            return Err(ConfigError::InvalidFormat("LB_LISTEN_ADDR".to_string()));
        }

        let backends: Vec<String> = backends_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if backends.is_empty() {
            return Err(ConfigError::InvalidFormat("LB_BACKENDS".to_string()));
        }

        if upstream_timeout_raw.trim().is_empty() {
            return Err(ConfigError::InvalidFormat(
                "LB_UPSTREAM_TIMEOUT_MS".to_string(),
            ));
        }
        let upstream_timeout_ms = upstream_timeout_raw
            .trim()
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidFormat("LB_UPSTREAM_TIMEOUT_MS".to_string()))
            .and_then(|v| {
                if v == 0 {
                    Err(ConfigError::InvalidFormat(
                        "LB_UPSTREAM_TIMEOUT_MS".to_string(),
                    ))
                } else {
                    Ok(v)
                }
            })?;

        if health_interval_raw.trim().is_empty() {
            return Err(ConfigError::InvalidFormat(
                "LB_HEALTH_CHECK_INTERVAL_MS".to_string(),
            ));
        }
        let health_check_interval_ms = health_interval_raw
            .trim()
            .parse::<u64>()
            .map_err(|_| ConfigError::InvalidFormat("LB_HEALTH_CHECK_INTERVAL_MS".to_string()))
            .and_then(|v| {
                if v == 0 {
                    Err(ConfigError::InvalidFormat(
                        "LB_HEALTH_CHECK_INTERVAL_MS".to_string(),
                    ))
                } else {
                    Ok(v)
                }
            })?;

        Ok(Config {
            listen_addr: trimmed.to_string(),
            upstream_timeout_ms,
            health_check_interval_ms,
            backends,
        })
    }

    pub fn from_env() -> Result<Self, ConfigError> {
        let listen_addr = env::var("LB_LISTEN_ADDR")
            .map_err(|_| ConfigError::NotFound("LB_LISTEN_ADDR".to_string()))?;

        let backends_raw = env::var("LB_BACKENDS")
            .map_err(|_| ConfigError::NotFound("LB_BACKENDS".to_string()))?;

        let upstream_timeout_ms = env::var("LB_UPSTREAM_TIMEOUT_MS")
            .map_err(|_| ConfigError::NotFound("LB_UPSTREAM_TIMEOUT_MS".to_string()))?;

        let health_check_interval_ms = env::var("LB_HEALTH_CHECK_INTERVAL_MS")
            .map_err(|_| ConfigError::NotFound("LB_HEALTH_CHECK_INTERVAL_MS".to_string()))?;

        Self::parse(
            listen_addr,
            backends_raw,
            upstream_timeout_ms,
            health_check_interval_ms,
        )
    }
}
