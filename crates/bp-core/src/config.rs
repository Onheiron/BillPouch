use crate::error::{BpError, BpResult};
use directories::ProjectDirs;
use std::path::PathBuf;

/// Returns the base BillPouch config directory (~/.local/share/billpouch on Linux).
pub fn base_dir() -> BpResult<PathBuf> {
    ProjectDirs::from("io", "billpouch", "billpouch")
        .map(|pd| pd.data_local_dir().to_path_buf())
        .ok_or_else(|| BpError::Config("Cannot determine home directory".into()))
}

/// Path to the stored keypair (identity) — plaintext.
pub fn identity_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("identity.key"))
}

/// Path to the passphrase-encrypted keypair (Argon2id + AES-256-GCM).
///
/// Present only when the user creates the identity with `--passphrase`.
/// Takes precedence over [`identity_path`] when both files are checked.
pub fn encrypted_identity_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("identity.key.enc"))
}

/// Path to the user profile JSON (display name, etc.).
pub fn profile_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("profile.json"))
}

/// Path to the daemon PID file.
pub fn pid_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("daemon.pid"))
}

/// Path to the Unix control socket.
pub fn socket_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("control.sock"))
}

/// Path to persisted service registry.
pub fn services_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("services.json"))
}

/// Path to persisted network membership list.
pub fn networks_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("networks.json"))
}

/// Path to the persisted Kademlia peer multi-addresses.
///
/// Saved periodically and on daemon shutdown so the next startup can
/// immediately dial previously-known peers, skipping the mDNS warm-up.
pub fn kad_peers_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("kad_peers.json"))
}

/// Path to the user-editable bootstrap node list.
///
/// Each entry is a multiaddr string that includes a `/p2p/<PeerId>` suffix,
/// for example `/ip4/203.0.113.1/tcp/4001/p2p/12D3KooW...`.
/// The list is read once at daemon startup; add entries with a text editor
/// or via `bp bootstrap add <addr>` (when implemented).
pub fn bootstrap_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("bootstrap.json"))
}

/// Path to the per-network secret keys store.
///
/// JSON map: `{ "<network_id>": "<hex-encoded 32-byte key>" }`.
/// Each entry is a randomly generated secret — **not** derived from the
/// network name.  This file must never leave the local machine; keys are
/// distributed to new members exclusively via signed+encrypted invite tokens.
pub fn network_keys_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("network_keys.json"))
}

/// Path to the persisted per-chunk CEK hints.
///
/// JSON map: `{ "<chunk_id>": "<hex-encoded 32-byte plaintext hash>" }`.
/// The plaintext hash is used to re-derive the per-user CEK at `GetFile` time.
/// Without this file, files encrypted with a previous daemon session cannot
/// be decrypted after a daemon restart.
pub fn cek_hints_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("cek_hints.json"))
}

/// Ensure the base directory (and any subdirs) exist.
pub fn ensure_dirs() -> BpResult<()> {
    std::fs::create_dir_all(base_dir()?).map_err(BpError::Io)
}
