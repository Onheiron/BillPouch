//! `bp login` / `bp logout`

use bp_core::identity::Identity;

/// Create or load the user's identity and display their fingerprint.
pub async fn login(alias: Option<String>) -> anyhow::Result<()> {
    if Identity::exists()? {
        let id = Identity::load()?;
        let display = id.profile.alias.as_deref().unwrap_or("<no alias>");
        println!("✅ Already logged in");
        println!("   Fingerprint : {}", id.fingerprint);
        println!("   PeerId      : {}", id.peer_id);
        println!("   Alias       : {}", display);
        return Ok(());
    }

    let id = Identity::generate(alias.clone())?;
    println!("🦤 New identity created for BillPouch");
    println!("   Fingerprint : {}", id.fingerprint);
    println!("   PeerId      : {}", id.peer_id);
    if let Some(a) = &alias {
        println!("   Alias       : {}", a);
    }
    println!();
    println!("Your identity key is stored locally. Keep it safe — it proves ownership.");
    Ok(())
}

/// Remove the stored identity from disk.
pub async fn logout() -> anyhow::Result<()> {
    if !Identity::exists()? {
        println!("You are not logged in.");
        return Ok(());
    }

    let id = Identity::load()?;
    Identity::remove()?;
    println!("👋 Logged out (fingerprint: {})", id.fingerprint);
    println!("   Your identity key has been removed from this machine.");
    Ok(())
}
