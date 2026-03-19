//! BillPouch daemon entry point.
//!
//! The daemon is started by the CLI (`bp hatch <type>`) when no running daemon
//! is detected.  It owns the libp2p Swarm and the control Unix socket.

use crate::{
    config,
    control::server::{run_control_server, DaemonState},
    error::{BpError, BpResult},
    identity::Identity,
    network::{self, NetworkState, StorageManagerMap},
    service::ServiceRegistry,
};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc;

/// Start the daemon process.  This function blocks until the daemon is killed.
pub async fn run_daemon() -> BpResult<()> {
    config::ensure_dirs()?;

    // ── Load identity ─────────────────────────────────────────────────────
    let identity = Identity::load()?;
    tracing::info!(
        peer_id = %identity.peer_id,
        fingerprint = %identity.fingerprint,
        "BillPouch daemon starting"
    );

    // Write PID file so the CLI can detect a running daemon.
    write_pid()?;

    // ── Build shared state ────────────────────────────────────────────────
    let network_state = Arc::new(RwLock::new(NetworkState::new()));
    let (net_tx, net_rx) = mpsc::channel::<crate::network::NetworkCommand>(64);

    // Shared storage managers: populated when Pouch services are hatched.
    let storage_managers: StorageManagerMap = Arc::new(RwLock::new(HashMap::new()));

    let daemon_state = Arc::new(DaemonState {
        identity: identity.clone(),
        services: RwLock::new(ServiceRegistry::new()),
        network_state: Arc::clone(&network_state),
        networks: RwLock::new(Vec::new()),
        net_tx: net_tx.clone(),
        storage_managers: Arc::clone(&storage_managers),
    });

    // ── Build libp2p swarm ────────────────────────────────────────────────
    let swarm = network::build_swarm(identity.keypair.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    let listen_addr = "/ip4/0.0.0.0/tcp/0"
        .parse()
        .map_err(|e: libp2p::multiaddr::Error| BpError::Network(e.to_string()))?;

    // ── Spawn network loop ────────────────────────────────────────────────
    let net_state = Arc::clone(&network_state);
    let net_storage = Arc::clone(&storage_managers);
    tokio::spawn(async move {
        if let Err(e) =
            network::run_network_loop(swarm, net_rx, net_state, listen_addr, net_storage).await
        {
            tracing::error!("Network loop exited with error: {}", e);
        }
    });

    // ── Gossip eviction task — remove stale peers every 60 s ─────────────
    let evict_state = Arc::clone(&network_state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Ok(mut ns) = evict_state.write() {
                ns.evict_stale(120); // evict nodes silent for >2 min
            }
        }
    });

    // ── Run control socket (blocks until daemon is killed) ────────────────
    let socket_path = config::socket_path()?;
    run_control_server(socket_path, daemon_state).await?;

    remove_pid();
    Ok(())
}

fn write_pid() -> BpResult<()> {
    let pid = std::process::id();
    let path = config::pid_path()?;
    std::fs::write(&path, pid.to_string()).map_err(BpError::Io)
}

fn remove_pid() {
    if let Ok(path) = config::pid_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Returns true if a daemon process is currently running (PID file exists and
/// process is alive).
pub fn is_running() -> bool {
    let path = match config::pid_path() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let pid: u32 = match content.trim().parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    // Check if process exists — kill(pid, 0) equivalent via /proc on Linux.
    let proc_path = format!("/proc/{}", pid);
    std::path::Path::new(&proc_path).exists()
}
