use simpleload_balancer_rs::{Backend, BackendPool, LoadBalancer};
use std::env;

mod http_server;

#[derive(Debug)]
struct Config {
    listen_addr: String,
    backends: Vec<String>,
    upstream_timeout_ms: u64,
    health_check_interval_ms: u64,
}

#[derive(Debug)]
enum ConfigError {
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
    fn parse(
        listen_addr: String,
        backends_raw: String,
        upstream_timeout_raw: String,
        health_interval_raw: String,
    ) -> Result<Self, ConfigError> {
        // 1.
        let trimmed = listen_addr.trim();

        if trimmed.is_empty() {
            return Err(ConfigError::InvalidFormat("LB_LISTEN_ADDR".to_string()));
        }

        //2.
        let backends: Vec<String> = backends_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if backends.is_empty() {
            return Err(ConfigError::InvalidFormat("LB_BACKENDS".to_string()));
        }

        //3.
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

        //4.
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

    fn from_env() -> Result<Self, ConfigError> {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::from_env()?;

    let mut pool = BackendPool::new();

    for backend in config.backends {
        pool.add(Backend::new(backend));
    }

    let load_balancer = LoadBalancer::new(pool);

    http_server::run(
        &config.listen_addr,
        load_balancer,
        config.upstream_timeout_ms,
        config.health_check_interval_ms,
    )
    .await
}
#[cfg(test)]
mod tests {
    use super::*;

    fn valid_params() -> (String, String, String, String) {
        (
            "127.0.0.1:8080".to_string(),
            "http://backend1, http://backend2".to_string(),
            "1000".to_string(),
            "5000".to_string(),
        )
    }

    #[test]
    fn test_parse_success() {
        let (addr, backends, timeout, interval) = valid_params();
        let result = Config::parse(addr, backends, timeout, interval);

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.listen_addr, "127.0.0.1:8080");
        assert_eq!(config.backends.len(), 2);
        assert_eq!(config.upstream_timeout_ms, 1000);
        assert_eq!(config.health_check_interval_ms, 5000);
    }

    #[test]
    fn test_listen_addr_empty_or_whitespace() {
        let (_, backends, timeout, interval) = valid_params();
        let result = Config::parse("   ".to_string(), backends, timeout, interval);

        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn test_backends_empty_or_whitespace() {
        let (addr, _, timeout, interval) = valid_params();
        let result = Config::parse(addr, " , , ".to_string(), timeout, interval);

        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn test_upstream_timeout_invalid() {
        let (addr, backends, _, interval) = valid_params();
        let result = Config::parse(addr, backends, "not_a_number".to_string(), interval);

        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn test_health_interval_invalid() {
        let (addr, backends, timeout, _) = valid_params();
        let result = Config::parse(addr, backends, timeout, "abc".to_string());

        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn zero_upstream_timeout_is_rejected() {
        let (addr, backends, _, interval) = valid_params();
        let result = Config::parse(addr, backends, "0".to_string(), interval);

        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }

    #[test]
    fn zero_health_check_interval_is_rejected() {
        let (addr, backends, timeout, _) = valid_params();
        let result = Config::parse(addr, backends, timeout, "0".to_string());

        assert!(matches!(result, Err(ConfigError::InvalidFormat(_))));
    }
}
