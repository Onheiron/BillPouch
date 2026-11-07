use thiserror::Error;

#[derive(Debug, Error)]
pub enum BpError {
    #[error("Identity error: {0}")]
    Identity(String),

    #[error("Service error: {0}")]
    Service(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Control socket error: {0}")]
    Control(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Coding error: {0}")]
    Coding(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Not authenticated — run `bp login` first")]
    NotAuthenticated,

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

pub type BpResult<T> = std::result::Result<T, BpError>;
