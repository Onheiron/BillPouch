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
//! - [`identity`] — Ed25519 keypair, passphrase encryption, export/import
//! - [`service`]  — service types (bill / pouch / post) and registry
//! - [`network`]  — libp2p swarm, gossip, QoS, Proof-of-Storage, reputation
//! - [`coding`]   — RLNC erasure coding over GF(2⁸), adaptive k/n
//! - [`storage`]  — Pouch fragment storage, tiers, chunk encryption
//! - [`invite`]   — signed + encrypted network invite tokens
//! - [`control`]  — Unix socket control protocol (CLI ↔ daemon)
//! - [`daemon`]   — daemon entry point wiring the above together
//! - [`config`]   — XDG file paths
//! - [`error`]    — unified error type

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
