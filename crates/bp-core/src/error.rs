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

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Not authenticated — run `bp login` first")]
    NotAuthenticated,

    #[error("Daemon not running — run `bp hatch <service_type>` first")]
    DaemonNotRunning,

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("Unknown network: {0}")]
    UnknownNetwork(String),
}

pub type BpResult<T> = std::result::Result<T, BpError>;
