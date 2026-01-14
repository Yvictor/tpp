use thiserror::Error;

#[derive(Error, Debug)]
pub enum TppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Token pool error: {0}")]
    TokenPool(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Server initialization error: {0}")]
    ServerInit(String),
}

pub type Result<T> = std::result::Result<T, TppError>;
