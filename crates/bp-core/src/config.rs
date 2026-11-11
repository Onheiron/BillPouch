use crate::error::{BpError, BpResult};
use directories::ProjectDirs;
use std::path::PathBuf;

fn project_dirs() -> BpResult<ProjectDirs> {
    ProjectDirs::from("io", "billpouch", "billpouch")
        .ok_or_else(|| BpError::Config("Cannot determine home directory".into()))
}

pub fn base_dir() -> BpResult<PathBuf> {
    Ok(project_dirs()?.data_local_dir().to_path_buf())
}

pub fn runtime_dir() -> BpResult<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs
        .runtime_dir()
        .unwrap_or_else(|| dirs.data_local_dir())
        .to_path_buf())
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

pub fn log_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("daemon.log"))
}

pub fn socket_path() -> BpResult<PathBuf> {
    Ok(runtime_dir()?.join("control.sock"))
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

pub fn qos_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("qos.json"))
}

pub fn services_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("services.json"))
}

pub fn files_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("files.json"))
}

pub fn chat_path() -> BpResult<PathBuf> {
    Ok(base_dir()?.join("chat.cbor"))
}

pub fn ensure_dirs() -> BpResult<()> {
    std::fs::create_dir_all(base_dir()?).map_err(BpError::Io)?;
    std::fs::create_dir_all(runtime_dir()?).map_err(BpError::Io)
}
