use crate::error::{BpError, BpResult};
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn base_dir() -> BpResult<PathBuf> {
    ProjectDirs::from("io", "billpouch", "billpouch")
        .map(|pd| pd.data_local_dir().to_path_buf())
        .ok_or_else(|| BpError::Config("Cannot determine home directory".into()))
}

pub fn identity_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("identity.key"))
}

pub fn encrypted_identity_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("identity.key.enc"))
}

pub fn profile_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("profile.json"))
}

pub fn pid_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("daemon.pid"))
}

pub fn socket_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("control.sock"))
}

pub fn kad_peers_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("kad_peers.json"))
}

pub fn bootstrap_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("bootstrap.json"))
}

pub fn network_keys_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("network_keys.json"))
}

pub fn cek_hints_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("cek_hints.json"))
}

pub fn file_registry_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("file_registry.json"))
}

pub fn ensure_dirs() -> BpResult<()> {
    std::fs::create_dir_all(base_dir()?).map_err(BpError::Io)
}
