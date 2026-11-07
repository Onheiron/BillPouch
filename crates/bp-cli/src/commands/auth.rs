use bp_core::identity::Identity;
use std::path::Path;

pub async fn login(
    ssh_key: Option<&Path>,
    alias: Option<String>,
    passphrase: Option<&str>,
) -> anyhow::Result<()> {
    let (id, verb) = if Identity::exists()? {
        (Identity::load(passphrase)?, "Already logged in")
    } else {
        (
            Identity::create(ssh_key, alias, passphrase)?,
            "Logged in on this device",
        )
    };
    println!("✅ {verb}");
    println!(
        "   User        : {} ({})",
        id.fingerprint,
        id.profile.alias.as_deref().unwrap_or("no alias")
    );
    println!("   Device      : {}", id.peer_id);
    println!();
    println!("Your public key (share it to be invited to a network):");
    println!("   {}", id.user_key);
    Ok(())
}

pub async fn logout() -> anyhow::Result<()> {
    if !Identity::exists()? {
        println!("You are not logged in.");
        return Ok(());
    }
    Identity::remove()?;
    println!("👋 Logged out: user key, device key and certificate removed from this machine.");
    Ok(())
}
