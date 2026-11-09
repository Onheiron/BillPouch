use crate::{
    config,
    control::server::{run_control_server, DaemonState},
    error::{BpError, BpResult},
    identity::Identity,
    network::{self, run_quality_monitor, NetworkState, RemoteFragmentIndex, StorageManagerMap},
    service::{ServiceRegistry, ServiceType},
    storage::StorageManager,
};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

pub async fn run_daemon(passphrase: Option<String>) -> BpResult<()> {
    config::ensure_dirs()?;
    if is_running() {
        return Err(BpError::Control("a daemon is already running".into()));
    }

    let resolved_pass = passphrase
        .or_else(|| std::env::var("BP_PASSPHRASE").ok())
        .filter(|s| !s.is_empty());

    let identity = Identity::load(resolved_pass.as_deref())?;
    tracing::info!(
        peer_id = %identity.peer_id,
        fingerprint = %identity.fingerprint,
        "BillPouch daemon starting"
    );

    let network_state = Arc::new(RwLock::new(NetworkState::new()));
    let (net_tx, net_rx) = mpsc::channel::<crate::network::NetworkCommand>(64);

    let services = ServiceRegistry::load();
    let storage_managers: StorageManagerMap = Arc::new(RwLock::new(
        services
            .all()
            .into_iter()
            .filter(|s| s.service_type == ServiceType::Pouch)
            .filter_map(|s| match StorageManager::load(&s.network_id, &s.id) {
                Ok(sm) => Some((s.id.clone(), Arc::new(RwLock::new(sm)))),
                Err(e) => {
                    tracing::warn!(service_id = %s.id, "cannot reopen a Pouch: {e}");
                    None
                }
            })
            .collect(),
    ));
    tracing::info!(services = services.all().len(), "services restored");

    let qos = Arc::new(RwLock::new(crate::network::QosRegistry::load()));
    let remote_fragment_index = Arc::new(RwLock::new(RemoteFragmentIndex::new()));
    let chains = Arc::new(RwLock::new(crate::chain::Chain::load_all(
        &identity,
        crate::chain::now_ms(),
    )?));
    let observations = crate::chain::ObservationLog::default();
    let listen_addrs: crate::network::ListenAddrs = Arc::default();

    let daemon_state = Arc::new(DaemonState {
        identity: identity.clone(),
        services: RwLock::new(services),
        network_state: Arc::clone(&network_state),
        networks: RwLock::new(Vec::new()),
        chains: Arc::clone(&chains),
        net_tx: net_tx.clone(),
        storage_managers: Arc::clone(&storage_managers),
        qos,
        remote_fragment_index: Arc::clone(&remote_fragment_index),
        files: RwLock::new(crate::files::FileStore::load()),
        listen_addrs: Arc::clone(&listen_addrs),
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
    let net_fragment_idx = Arc::clone(&remote_fragment_index);
    let net_chains = Arc::clone(&chains);
    let net_identity = identity.clone();
    let net_observations = Arc::clone(&observations);
    let net_listen_addrs = Arc::clone(&listen_addrs);
    let (file_tx, mut file_rx) = mpsc::channel(256);
    tokio::spawn(async move {
        if let Err(e) = network::run_network_loop(
            swarm,
            net_rx,
            net_state,
            listen_addr,
            net_storage,
            net_fragment_idx,
            net_chains,
            net_identity,
            net_observations,
            file_tx,
            net_listen_addrs,
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
    let monitor_observations = Arc::clone(&observations);
    let monitor_identity = identity.clone();
    let monitor_storage = Arc::clone(&storage_managers);
    let monitor_chains = Arc::clone(&chains);
    tokio::spawn(async move {
        run_quality_monitor(
            monitor_identity,
            monitor_state,
            monitor_qos,
            monitor_observations,
            monitor_tx,
            monitor_storage,
            monitor_chains,
        )
        .await;
    });

    let socket_path = config::socket_path()?;
    run_control_server(socket_path, daemon_state).await
}

pub fn is_running() -> bool {
    config::socket_path().is_ok_and(|p| std::os::unix::net::UnixStream::connect(p).is_ok())
}
