//! Unified error type for bp-core.
//!
//! All fallible functions in the library return [`BpResult<T>`], which is a
//! type alias for `Result<T, BpError>`.  Callers can match on individual
//! variants for fine-grained handling, or propagate with `?` and let `anyhow`
//! or `thiserror` format a human-readable message.

use thiserror::Error;

/// Every error that can originate inside `bp-core`.
///
/// Uses `#[from]` blanket conversions for `std::io::Error` and
/// `serde_json::Error` so that `?` works transparently on I/O and
/// serialisation operations.
#[derive(Debug, Error)]
pub enum BpError {
    /// Keypair generation, encoding, or decoding failed.
    #[error("Identity error: {0}")]
    Identity(String),

    /// Invalid service type string or registry operation failed.
    #[error("Service error: {0}")]
    Service(String),

    /// libp2p swarm or gossip operation failed.
    #[error("Network error: {0}")]
    Network(String),

    /// Unix socket bind, accept, or I/O failed.
    #[error("Control socket error: {0}")]
    Control(String),

    /// XDG path resolution failed (e.g. no home directory).
    #[error("Config error: {0}")]
    Config(String),

    /// Storage directory, fragment I/O, or quota error.
    #[error("Storage error: {0}")]
    Storage(String),

    /// RLNC encoding, decoding, or recoding error (e.g. singular matrix).
    #[error("Coding error: {0}")]
    Coding(String),

    /// Transparent wrapper around [`std::io::Error`].
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Transparent wrapper around [`serde_json::Error`].
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// The user has not yet run `bp login` — no keypair on disk.
    #[error("Not authenticated — run `bp login` first")]
    NotAuthenticated,

    /// `bp hatch` has not been run — no daemon PID file or process found.
    #[error("Daemon not running — run `bp hatch <service_type>` first")]
    DaemonNotRunning,

    /// A service UUID was referenced but is not in the registry.
    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    /// A network ID was referenced that this daemon has not joined.
    #[error("Unknown network: {0}")]
    UnknownNetwork(String),
}

/// Convenience alias — all bp-core functions return this.
pub type BpResult<T> = std::result::Result<T, BpError>;
