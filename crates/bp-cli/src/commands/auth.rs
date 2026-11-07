use bp_core::identity::Identity;

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

pub async fn logout() -> anyhow::Result<()> {
    if !Identity::exists()? {
        println!("You are not logged in.");
        return Ok(());
    }

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

pub async fn export_identity(out: &str) -> anyhow::Result<()> {
    let dest = std::path::Path::new(out);
    Identity::export_to_file(dest)?;
    println!("✅ Identity exported to: {out}");
    println!("   Transfer this file to the target machine and run:");
    println!("   bp import-identity {out}");
    Ok(())
}

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
