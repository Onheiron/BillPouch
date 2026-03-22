//! CLI command handlers — one module per subcommand.
//!
//! Each module exposes one or more `pub async fn` that:
//! 1. Validate preconditions (e.g. identity exists, daemon is running).
//! 2. Build a [`ControlRequest`] and forward it via [`ControlClient`].
//! 3. Pretty-print the response to stdout.
//!
//! [`ControlRequest`]: bp_core::control::protocol::ControlRequest
//! [`ControlClient`]: crate::client::ControlClient

pub mod auth;
pub mod bootstrap;
pub mod farewell;
pub mod flock;
pub mod get;
pub mod hatch;
pub mod invite;
pub mod join;

pub mod put;
pub mod relay;
