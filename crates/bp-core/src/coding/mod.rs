//! Erasure coding layer — Random Linear Network Coding (RLNC) over GF(2⁸).
//!
//! ## Why RLNC
//!
//! RLNC has a property that standard Reed-Solomon and RaptorQ lack: a node can
//! **recode** — produce a new valid encoded fragment by recombining the fragments
//! it already holds, without ever reconstructing the original data.  This is the
//! key requirement for BillPouch's distributed fragment replication.
//!
//! ## Sub-modules
//!
//! - [`gf256`] — arithmetic in GF(2⁸) (add, mul, div, inv).
//! - [`rlnc`]  — encode, recode and decode operations on byte chunks.

pub mod gf256;
pub mod params;
pub mod rlnc;

pub use params::{compute_coding_params, effective_recovery_probability, NetworkCodingParams};
pub use rlnc::{decode, encode, recode, EncodedFragment};
