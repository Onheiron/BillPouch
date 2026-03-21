//! `bp login` / `bp logout`

use bp_core::identity::Identity;

/// Create or load the user's identity and display their fingerprint.
///
/// If `passphrase` is `Some`, new identities are stored encrypted
/// (Argon2id + AES-256-GCM).  Existing encrypted identities also require
/// `passphrase` to display current identity info.
pub async fn login(alias: Option<String>, passphrase: Option<&str>) -> anyhow::Result<()> {
    if Identity::exists()? {
        let id = Identity::load(passphrase)?;
        let display = id.profile.alias.as_deref().unwrap_or("<no alias>");
        println!("✅ Already logged in");
        println!("   Fingerprint : {}", id.fingerprint);
        println!("   PeerId      : {}", id.peer_id);
        println!("   Alias       : {}", display);
        if passphrase.is_some() {
            println!("   Key         : passphrase-protected");
        } else {
            println!("   Key         : plaintext (no passphrase)");
        }
        return Ok(());
    }

    let id = Identity::generate(alias.clone(), passphrase)?;
    println!("🦤 New identity created for BillPouch");
    println!("   Fingerprint : {}", id.fingerprint);
    println!("   PeerId      : {}", id.peer_id);
    if let Some(a) = &alias {
        println!("   Alias       : {}", a);
    }
    if passphrase.is_some() {
        println!("   Key         : passphrase-protected (Argon2id + AES-256-GCM)");
    }
    println!();
    println!("Your identity key is stored locally. Keep it safe — it proves ownership.");
    Ok(())
}

/// Remove the stored identity from disk.
///
/// Logout does not require the passphrase — the fingerprint is read from the
/// plaintext `profile.json` and both key files are deleted.
pub async fn logout() -> anyhow::Result<()> {
    if !Identity::exists()? {
        println!("You are not logged in.");
        return Ok(());
    }

    // Read fingerprint from profile without decrypting the key.
    let fp = bp_core::config::profile_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| {
            serde_json::from_str::<bp_core::identity::UserProfile>(&s)
                .ok()
                .map(|up| up.fingerprint)
        })
        .unwrap_or_else(|| "<unknown>".into());

    Identity::remove()?;
    println!("👋 Logged out (fingerprint: {})", fp);
    println!("   Your identity key has been removed from this machine.");
    Ok(())
}
