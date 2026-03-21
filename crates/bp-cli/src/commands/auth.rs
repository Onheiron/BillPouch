//! `bp login` / `bp logout` / `bp export-identity` / `bp import-identity`

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

/// Export the current identity to a portable JSON file.
///
/// The exported file carries the keypair (plaintext or encrypted) and the
/// user profile.  Transfer it to another machine and use `bp import-identity`
/// to install it there.
///
/// ⚠️  A **plaintext** key export contains raw private-key material.
///     Keep the file confidential.  A passphrase-protected identity exports
///     the encrypted ciphertext — the original passphrase is still required
///     to use the key on the new machine.
pub async fn export_identity(out: &str) -> anyhow::Result<()> {
    let dest = std::path::Path::new(out);
    Identity::export_to_file(dest)?;
    println!("✅ Identity exported to: {out}");
    println!("   Transfer this file to the target machine and run:");
    println!("   bp import-identity {out}");
    Ok(())
}

/// Import an identity exported with `bp export-identity`.
///
/// Installs the keypair and profile from `path` into the local XDG data
/// directory.  If `force` is `true`, any existing identity is overwritten
/// without prompting.
pub async fn import_identity(path: &str, force: bool) -> anyhow::Result<()> {
    let src = std::path::Path::new(path);
    let profile = Identity::import_from_file(src, force)?;
    let display = profile.alias.as_deref().unwrap_or("<no alias>");
    println!("✅ Identity imported");
    println!("   Fingerprint : {}", profile.fingerprint);
    println!("   Alias       : {display}");
    println!("   Created     : {}", profile.created_at);
    println!();
    println!("You can now start the daemon:  bp hatch post");
    Ok(())
}
