use crate::{
    config,
    control::server::{run_control_server, DaemonState},
    error::{BpError, BpResult},
    identity::Identity,
    network::{
        self, run_quality_monitor, NetworkState, OutgoingAssignments, RemoteFragmentIndex,
        StorageManagerMap,
    },
    service::ServiceRegistry,
};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc;

pub async fn run_daemon(passphrase: Option<String>) -> BpResult<()> {
    config::ensure_dirs()?;

    let resolved_pass = passphrase
        .or_else(|| std::env::var("BP_PASSPHRASE").ok())
        .filter(|s| !s.is_empty());

    let identity = Identity::load(resolved_pass.as_deref())?;
    tracing::info!(
        peer_id = %identity.peer_id,
        fingerprint = %identity.fingerprint,
        "BillPouch daemon starting"
    );

    write_pid()?;

    let network_state = Arc::new(RwLock::new(NetworkState::new()));
    let (net_tx, net_rx) = mpsc::channel::<crate::network::NetworkCommand>(64);

    let storage_managers: StorageManagerMap = Arc::new(RwLock::new(HashMap::new()));

    let qos = Arc::new(RwLock::new(crate::network::QosRegistry::new()));
    let outgoing_assignments: OutgoingAssignments = Arc::new(RwLock::new(HashMap::new()));
    let remote_fragment_index = Arc::new(RwLock::new(RemoteFragmentIndex::new()));
    let chains = Arc::new(RwLock::new(crate::chain::Chain::load_all(
        &identity,
        crate::chain::now_ms(),
    )?));
    let observations = crate::chain::ObservationLog::default();

    let daemon_state = Arc::new(DaemonState {
        identity: identity.clone(),
        services: RwLock::new(ServiceRegistry::new()),
        network_state: Arc::clone(&network_state),
        networks: RwLock::new(Vec::new()),
        chains: Arc::clone(&chains),
        net_tx: net_tx.clone(),
        storage_managers: Arc::clone(&storage_managers),
        qos,
        outgoing_assignments: Arc::clone(&outgoing_assignments),
        remote_fragment_index: Arc::clone(&remote_fragment_index),
        files: RwLock::new(crate::files::FileStore::load()),
    });

    let swarm = network::build_swarm(identity.device.clone())
        .map_err(|e| BpError::Network(e.to_string()))?;

    let listen_port: u16 = std::env::var("BP_LISTEN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let listen_addr = format!("/ip4/0.0.0.0/tcp/{listen_port}")
        .parse()
        .map_err(|e: libp2p::multiaddr::Error| BpError::Network(e.to_string()))?;

    let net_state = Arc::clone(&network_state);
    let net_storage = Arc::clone(&storage_managers);
    let net_outgoing = Arc::clone(&outgoing_assignments);
    let net_fragment_idx = Arc::clone(&remote_fragment_index);
    let net_chains = Arc::clone(&chains);
    let net_identity = identity.clone();
    let net_observations = Arc::clone(&observations);
    let (file_tx, mut file_rx) = mpsc::channel(256);
    tokio::spawn(async move {
        if let Err(e) = network::run_network_loop(
            swarm,
            net_rx,
            net_state,
            listen_addr,
            net_storage,
            net_outgoing,
            net_fragment_idx,
            net_chains,
            net_identity,
            net_observations,
            file_tx,
        )
        .await
        {
            tracing::error!("Network loop exited with error: {}", e);
        }
    });

    crate::control::server::join_all(&daemon_state).await;

    let file_state = Arc::clone(&daemon_state);
    tokio::spawn(async move {
        while let Some((network, gossip)) = file_rx.recv().await {
            crate::control::files::on_gossip(&file_state, &network, gossip).await;
        }
    });

    let evict_state = Arc::clone(&network_state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Ok(mut ns) = evict_state.write() {
                ns.evict_stale(120);
            }
        }
    });

    let reannounce_state = Arc::clone(&daemon_state);
    tokio::spawn(async move {
        for delay in [10, 10, 10].into_iter().chain(std::iter::repeat(30)) {
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            crate::control::server::announce_all(&reannounce_state).await;
            crate::control::files::sync_cycle(&reannounce_state).await;
        }
    });

    let monitor_state = Arc::clone(&network_state);
    let monitor_qos = Arc::clone(&daemon_state.qos);
    let monitor_tx = net_tx.clone();
    let monitor_outgoing = Arc::clone(&outgoing_assignments);
    let monitor_observations = Arc::clone(&observations);
    tokio::spawn(async move {
        run_quality_monitor(
            monitor_state,
            monitor_qos,
            monitor_observations,
            monitor_tx,
            monitor_outgoing,
        )
        .await;
    });

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
    let proc_path = format!("/proc/{}", pid);
    std::path::Path::new(&proc_path).exists()
}
