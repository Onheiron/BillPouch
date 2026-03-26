//! BillPouch daemon entry point.
//!
//! The daemon is started by the CLI (`bp hatch <type>`) when no running daemon
//! is detected.  It owns the libp2p Swarm and the control Unix socket.

use crate::{
    config,
    control::server::{load_cek_hints, run_control_server, DaemonState},
    error::{BpError, BpResult},
    identity::Identity,
    network::{
        self, run_quality_monitor, NetworkState, OutgoingAssignments, RemoteFragmentIndex,
        ReputationStore, StorageManagerMap,
    },
    service::ServiceRegistry,
    storage::FileRegistry,
};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc;

/// Start the daemon process.  This function blocks until the daemon is killed.
///
/// `passphrase` is forwarded to [`Identity::load`].  Pass `None` for
/// plaintext identities; pass `Some(p)` when the identity was created with
/// `--passphrase`.  As a convenience the daemon also checks the
/// `BP_PASSPHRASE` environment variable when `passphrase` is `None`.
pub async fn run_daemon(passphrase: Option<String>) -> BpResult<()> {
    config::ensure_dirs()?;

    // Resolve passphrase: explicit arg → BP_PASSPHRASE env var → None.
    let resolved_pass = passphrase
        .or_else(|| std::env::var("BP_PASSPHRASE").ok())
        .filter(|s| !s.is_empty());

    // ── Load identity ─────────────────────────────────────────────────────
    let identity = Identity::load(resolved_pass.as_deref())?;
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

    let qos = Arc::new(RwLock::new(crate::network::QosRegistry::new()));
    let outgoing_assignments: OutgoingAssignments = Arc::new(RwLock::new(HashMap::new()));
    let remote_fragment_index = Arc::new(RwLock::new(RemoteFragmentIndex::new()));

    let daemon_state = Arc::new(DaemonState {
        identity: identity.clone(),
        services: RwLock::new(ServiceRegistry::new()),
        network_state: Arc::clone(&network_state),
        networks: RwLock::new(Vec::new()),
        net_tx: net_tx.clone(),
        storage_managers: Arc::clone(&storage_managers),
        qos,
        outgoing_assignments: Arc::clone(&outgoing_assignments),
        remote_fragment_index: Arc::clone(&remote_fragment_index),
        chunk_cek_hints: RwLock::new(load_cek_hints()),
        reputation: RwLock::new(ReputationStore::new()),
        file_registry: RwLock::new(
            config::file_registry_path()
                .map(|p| FileRegistry::load(&p))
                .unwrap_or_default(),
        ),
    });

    // ── Build libp2p swarm ────────────────────────────────────────────────
    let swarm = network::build_swarm(identity.keypair.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    // Allow the operator to bind a fixed port via BP_LISTEN_PORT (useful for
    // Docker playgrounds and bootstrap peers).  Defaults to 0 (OS assigns a
    // random port, which is fine for normal use and mDNS-based discovery).
    let listen_port: u16 = std::env::var("BP_LISTEN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let listen_addr = format!("/ip4/0.0.0.0/tcp/{listen_port}")
        .parse()
        .map_err(|e: libp2p::multiaddr::Error| BpError::Network(e.to_string()))?;

    // ── Spawn network loop ────────────────────────────────────────────────
    let net_state = Arc::clone(&network_state);
    let net_storage = Arc::clone(&storage_managers);
    let net_outgoing = Arc::clone(&outgoing_assignments);
    let net_fragment_idx = Arc::clone(&remote_fragment_index);
    tokio::spawn(async move {
        if let Err(e) = network::run_network_loop(
            swarm,
            net_rx,
            net_state,
            listen_addr,
            net_storage,
            net_outgoing,
            net_fragment_idx,
        )
        .await
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

    // ── Network quality monitor — Ping all Pouch peers, update QoS ──────
    let monitor_state = Arc::clone(&network_state);
    let monitor_qos = Arc::clone(&daemon_state.qos);
    let monitor_tx = net_tx.clone();
    let monitor_outgoing = Arc::clone(&outgoing_assignments);
    tokio::spawn(async move {
        run_quality_monitor(monitor_state, monitor_qos, monitor_tx, monitor_outgoing).await;
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
