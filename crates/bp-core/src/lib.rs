//! # bp-core — BillPouch core library
//!
//! Provides all types, networking, daemon logic and control protocol for
//! BillPouch — a P2P social distributed filesystem.
//!
//! ```text
//!   ┌──────────────────────────────────────────────────────┐
//!   │                    BillPouch Network                  │
//!   │                                                      │
//!   │  ┌────────┐     ┌────────┐     ┌────────┐           │
//!   │  │ Pouch  │◄────│  Post  │────►│  Bill  │           │
//!   │  │(storage│     │(relay) │     │(I/O)   │           │
//!   │  └────────┘     └────────┘     └────────┘           │
//!   │          gossipsub + Kademlia DHT                     │
//!   └──────────────────────────────────────────────────────┘
//! ```
//!
//! ## Modules
//! - [`identity`]  — Ed25519 keypair management (login/logout)
//! - [`service`]   — Service types (bill / pouch / post) and registry
//! - [`network`]   — libp2p swarm, gossipsub, Kademlia DHT, mDNS
//! - [`control`]   — Unix socket control protocol (CLI ↔ daemon)
//! - [`daemon`]    — Main daemon task orchestrating the above
//! - [`config`]    — Configuration file paths
//! - [`error`]     — Unified error type

pub mod coding;
pub mod config;
pub mod control;
pub mod daemon;
pub mod error;
pub mod identity;
pub mod invite;
pub mod network;
pub mod service;
pub mod storage;

pub use error::{BpError, BpResult};
