use core::fmt;
use std::error::Error;

use crate::config::ConfigError;

#[derive(Debug)]
pub enum AppError {
    Config(ConfigError),

    Bind(std::io::Error),

    Accept(std::io::Error),

    Shutdown(std::io::Error),

    Task(tokio::task::JoinError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Config(err) => write!(f, "Configuration error: {err}"),
            AppError::Bind(err) => write!(f, "Failed to bind network listener: {err}"),
            AppError::Accept(err) => write!(f, "Failed to accept incoming connection: {err}"),
            AppError::Shutdown(msg) => write!(f, "Shutdown signal error (ctrl_c): {msg}"),
            AppError::Task(err) => write!(f, "Background task failed: {err}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Config(err) => Some(err),
            AppError::Bind(err) => Some(err),
            AppError::Accept(err) => Some(err),
            AppError::Shutdown(err) => Some(err),
            AppError::Task(err) => Some(err),
        }
    }
}
