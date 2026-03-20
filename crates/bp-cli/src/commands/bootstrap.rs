//! `bp bootstrap` — manage the WAN bootstrap node list.
//!
//! These commands operate directly on
//! `~/.local/share/billpouch/bootstrap.json` without requiring a running
//! daemon.  Changes take effect the next time the daemon starts.

use anyhow::{bail, Context, Result};
use bp_core::{config, network::BootstrapList};

/// Print all currently configured bootstrap addresses.
pub fn list() -> Result<()> {
    let path = config::bootstrap_path()?;
    let list = BootstrapList::load(&path);
    if list.0.is_empty() {
        println!("No bootstrap nodes configured.");
        println!("  Add one with: bp bootstrap add <multiaddr>");
    } else {
        println!("Bootstrap nodes ({}):", list.0.len());
        for addr in &list.0 {
            println!("  {}", addr);
        }
    }
    Ok(())
}

/// Add `addr` to the bootstrap list (idempotent).
///
/// `addr` must be a valid multiaddr string.  A `/p2p/<PeerId>` suffix is
/// required for the daemon to register the peer in Kademlia correctly.
pub fn add(addr: String) -> Result<()> {
    // Basic validation: must start with '/' and contain a '/p2p/' component.
    if !addr.starts_with('/') {
        bail!("Invalid multiaddr '{addr}': must start with '/'");
    }
    if !addr.contains("/p2p/") {
        eprintln!(
            "⚠  Warning: '{addr}' has no /p2p/<PeerId> component — \
             the daemon will not be able to register this peer in Kademlia."
        );
    }

    let path = config::bootstrap_path().context("Cannot resolve bootstrap path")?;
    let mut list = BootstrapList::load(&path);

    if list.0.contains(&addr) {
        println!("Already in bootstrap list: {addr}");
        return Ok(());
    }

    list.add(addr.clone());
    list.save(&path);
    println!("✔ Added bootstrap node: {addr}");
    println!("  Restart the daemon for changes to take effect.");
    Ok(())
}

/// Remove `addr` from the bootstrap list.
pub fn remove(addr: &str) -> Result<()> {
    let path = config::bootstrap_path().context("Cannot resolve bootstrap path")?;
    let mut list = BootstrapList::load(&path);

    if !list.0.contains(&addr.to_string()) {
        println!("Not in bootstrap list: {addr}");
        return Ok(());
    }

    list.remove(addr);
    list.save(&path);
    println!("✔ Removed bootstrap node: {addr}");
    Ok(())
}
