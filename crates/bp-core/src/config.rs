use crate::error::{BpError, BpResult};
use directories::ProjectDirs;
use std::path::PathBuf;

pub fn base_dir() -> BpResult<PathBuf> {
    ProjectDirs::from("io", "billpouch", "billpouch")
        .map(|pd| pd.data_local_dir().to_path_buf())
        .ok_or_else(|| BpError::Config("Cannot determine home directory".into()))
}

pub fn user_key_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("user_ed25519"))
}

pub fn device_key_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("device.key"))
}

pub fn device_cert_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("device_cert.json"))
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

pub fn networks_dir() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("networks"))
}

pub fn fillers_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("fillers.json"))
}

pub fn files_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("files.json"))
}

pub fn ensure_dirs() -> BpResult<()> {
    std::fs::create_dir_all(base_dir()?).map_err(BpError::Io)
}
