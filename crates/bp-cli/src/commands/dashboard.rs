use bp_core::{identity::Identity, session};

pub fn dashboard(port: u16, passphrase: Option<&str>) -> anyhow::Result<()> {
    let identity = Identity::load(passphrase)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let token = session::issue(&identity, now)?;
    let url = format!("https://127.0.0.1:{port}/#session={token}");
    println!(
        "🔐 Dashboard session for {}, valid 12 h:",
        identity.fingerprint
    );
    println!("   {url}");
    println!("   The certificate is self-signed: check its fingerprint against the one bp-api logs at start.");
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(&url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    Ok(())
}
