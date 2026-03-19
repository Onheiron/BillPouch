//! IPC control layer between the CLI and the daemon.
//!
//! Transport: Unix domain socket at `~/.local/share/billpouch/control.sock`.
//! Framing: one UTF-8 JSON object per line (newline-delimited JSON).
//!
//! The CLI sends a single [`protocol::ControlRequest`] and reads a single
//! [`protocol::ControlResponse`], then closes the connection.  The daemon
//! handles each connection in its own `tokio::spawn`ed task.
//!
//! ## Sub-modules
//!
//! - [`protocol`] — request/response types and typed data payloads.
//! - [`server`]   — async Unix listener, dispatcher, and `DaemonState`.

pub mod protocol;
pub mod server;

pub use protocol::{ControlRequest, ControlResponse};
pub use server::{run_control_server, DaemonState};
