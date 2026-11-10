use std::path::PathBuf;

pub struct Fifo(pub PathBuf);

impl Fifo {
    pub fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!("bp-{:016x}.fifo", rand_suffix()));
        nix::unistd::mkfifo(
            &path,
            nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
        )?;
        Ok(Self(path))
    }
}

impl Drop for Fifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn rand_suffix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64 ^ u64::from(std::process::id()))
        .unwrap_or_default()
}
