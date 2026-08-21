mod http_server;

use simpleload_balancer_rs::{AppError, Backend, BackendPool, Config, LoadBalancer};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    tracing_subscriber::fmt::init();

    let config = Config::from_env().map_err(AppError::Config)?;

    let shutdown_token = CancellationToken::new();

    let mut pool = BackendPool::new();

    for backend in config.backends {
        pool.add(Backend::new(backend));
    }

    info!(
        listen_address = %config.listen_addr,
        backends_count = pool.len(),
        "Starting proxy server"
    );

    let load_balancer = LoadBalancer::new(pool);

    http_server::run(
        &config.listen_addr,
        load_balancer,
        config.upstream_timeout_ms,
        config.health_check_interval_ms,
        shutdown_token,
    )
    .await
}

#[cfg(test)]
mod tests {
    use simpleload_balancer_rs::ConfigError;

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
