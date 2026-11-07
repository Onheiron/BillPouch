use anyhow::{bail, Context, Result};
use bp_core::{config, network::BootstrapList};

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

pub fn add(addr: String) -> Result<()> {
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
