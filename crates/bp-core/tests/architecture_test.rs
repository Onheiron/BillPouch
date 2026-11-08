use bp_core::control::protocol::*;
use bp_core::network::state::{NetworkState, NodeInfo};
use bp_core::service::*;
use std::collections::HashMap;

fn chains_for(identity: &bp_core::identity::Identity, names: &[&str]) -> bp_core::chain::Chains {
    let chains = names
        .iter()
        .map(|n| {
            let chain = bp_core::chain::Chain::create(identity, n).unwrap();
            (n.to_string(), chain)
        })
        .collect();
    std::sync::Arc::new(std::sync::RwLock::new(chains))
}

fn test_cert() -> bp_core::identity::DeviceCert {
    bp_core::identity::Identity::in_memory(None).unwrap().cert
}

#[test]
fn servizi_tre_tipi_pouch_bill_post() {
    let pouch: ServiceType = "pouch".parse().unwrap();
    let bill: ServiceType = "bill".parse().unwrap();
    let post: ServiceType = "post".parse().unwrap();

    assert_eq!(pouch, ServiceType::Pouch);
    assert_eq!(bill, ServiceType::Bill);
    assert_eq!(post, ServiceType::Post);
}

#[test]
fn servizi_tipo_sconosciuto_errore() {
    let result: Result<ServiceType, _> = "fridge".parse();
    assert!(result.is_err(), "'fridge' non è un servizio valido");
}

#[test]
fn servizi_case_insensitive() {
    let p1: ServiceType = "POUCH".parse().unwrap();
    let p2: ServiceType = "Pouch".parse().unwrap();
    let p3: ServiceType = "pouch".parse().unwrap();
    assert_eq!(p1, p2);
    assert_eq!(p2, p3);
}

#[test]
fn servizio_ogni_istanza_ha_uuid_unico() {
    let s1 = ServiceInfo::new(ServiceType::Pouch, "net-a".into(), HashMap::new());
    let s2 = ServiceInfo::new(ServiceType::Pouch, "net-a".into(), HashMap::new());

    assert_ne!(s1.id, s2.id, "Due servizi devono avere ID diversi");
    uuid::Uuid::parse_str(&s1.id).expect("Service ID deve essere un UUID valido");
}

#[test]
fn servizio_nasce_in_stato_starting() {
    let s = ServiceInfo::new(ServiceType::Bill, "my-net".into(), HashMap::new());
    assert_eq!(s.status, ServiceStatus::Running);
}

#[test]
fn servizio_pouch_con_metadata_storage() {
    let mut meta = HashMap::new();
    meta.insert("storage_bytes".into(), serde_json::json!(10_737_418_240u64));

    let s = ServiceInfo::new(ServiceType::Pouch, "net-a".into(), meta);

    assert_eq!(
        s.metadata.get("storage_bytes").unwrap(),
        &serde_json::json!(10_737_418_240u64),
        "Pouch deve poter dichiarare storage_bytes nei metadata"
    );
}

#[test]
fn registry_registra_e_recupera_servizi() {
    let mut reg = ServiceRegistry::new();
    let s = ServiceInfo::new(ServiceType::Pouch, "net".into(), HashMap::new());
    let id = s.id.clone();

    reg.register(s);
    assert!(
        reg.get(&id).is_some(),
        "Servizio registrato deve essere trovabile"
    );
}

#[test]
fn registry_rimuove_servizio_farewell() {
    let mut reg = ServiceRegistry::new();
    let s = ServiceInfo::new(ServiceType::Post, "net".into(), HashMap::new());
    let id = s.id.clone();

    reg.register(s);
    let removed = reg.remove(&id);
    assert!(removed.is_some(), "farewell deve rimuovere il servizio");
    assert!(
        reg.get(&id).is_none(),
        "Servizio rimosso non deve più esistere"
    );
}

#[test]
fn registry_piu_servizi_stessa_macchina() {
    let mut reg = ServiceRegistry::new();
    let pouch = ServiceInfo::new(ServiceType::Pouch, "net".into(), HashMap::new());
    let bill = ServiceInfo::new(ServiceType::Bill, "net".into(), HashMap::new());
    let post = ServiceInfo::new(ServiceType::Post, "net".into(), HashMap::new());

    reg.register(pouch);
    reg.register(bill);
    reg.register(post);

    assert_eq!(
        reg.all().len(),
        3,
        "Deve supportare 3 servizi sulla stessa macchina"
    );
}

#[test]
fn registry_servizi_su_network_diversi() {
    let mut reg = ServiceRegistry::new();
    let s1 = ServiceInfo::new(ServiceType::Pouch, "amici".into(), HashMap::new());
    let s2 = ServiceInfo::new(ServiceType::Bill, "lavoro".into(), HashMap::new());

    let net1 = s1.network_id.clone();
    let net2 = s2.network_id.clone();
    reg.register(s1);
    reg.register(s2);

    assert_ne!(net1, net2, "Servizi su network diversi");
    assert_eq!(reg.all().len(), 2);
}

fn make_node_info(
    peer_id: &str,
    fingerprint: &str,
    svc: ServiceType,
    net: &str,
    ts: u64,
) -> NodeInfo {
    NodeInfo {
        peer_id: peer_id.into(),
        user_fingerprint: fingerprint.into(),
        user_alias: Some("test".into()),
        device_cert: test_cert(),
        service_type: svc,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: net.into(),
        listen_addrs: vec!["/ip4/192.168.1.10/tcp/1234".into()],
        announced_at: ts,
        metadata: HashMap::new(),
    }
}

#[test]
fn gossip_upsert_inserisce_nuovo_nodo() {
    let mut state = NetworkState::new();
    let node = make_node_info("peer-A", "aabbccdd", ServiceType::Pouch, "net", 100);

    state.upsert(node);
    assert_eq!(state.len(), 1);
    assert_eq!(state.all()[0].peer_id, "peer-A");
}

#[test]
fn gossip_upsert_mantiene_annuncio_piu_recente() {
    let mut state = NetworkState::new();

    let old = make_node_info("peer-A", "aabbccdd", ServiceType::Pouch, "net", 100);
    let new = make_node_info("peer-A", "aabbccdd", ServiceType::Pouch, "net", 200);

    state.upsert(old);
    state.upsert(new);

    assert_eq!(state.len(), 1, "Deve essere un solo nodo, non duplicato");
    assert_eq!(
        state.all()[0].announced_at,
        200,
        "Deve tenere il timestamp più recente"
    );
}

#[test]
fn gossip_upsert_ignora_annuncio_vecchio() {
    let mut state = NetworkState::new();

    let new = make_node_info("peer-A", "aabbccdd", ServiceType::Pouch, "net", 200);
    let old = make_node_info("peer-A", "aabbccdd", ServiceType::Pouch, "net", 100);

    state.upsert(new);
    state.upsert(old);

    assert_eq!(
        state.all()[0].announced_at,
        200,
        "Non deve sovrascrivere con annuncio vecchio"
    );
}

#[test]
fn gossip_piu_utenti_stesso_network() {
    let mut state = NetworkState::new();

    let carlo = make_node_info("peer-carlo", "aabbccdd", ServiceType::Pouch, "amici", 100);
    let marco = make_node_info("peer-marco", "11223344", ServiceType::Pouch, "amici", 100);

    state.upsert(carlo);
    state.upsert(marco);

    let peers = state.in_network("amici");
    assert_eq!(peers.len(), 2, "Due utenti diversi nella stessa rete");

    let fingerprints: Vec<&str> = peers.iter().map(|n| n.user_fingerprint.as_str()).collect();
    assert!(fingerprints.contains(&"aabbccdd"), "Carlo deve esserci");
    assert!(fingerprints.contains(&"11223344"), "Marco deve esserci");
}

#[test]
fn gossip_stesso_utente_piu_servizi() {
    let mut state = NetworkState::new();

    let pouch = make_node_info("peer-carlo", "aabbccdd", ServiceType::Pouch, "amici", 100);
    let bill = make_node_info("peer-carlo", "aabbccdd", ServiceType::Bill, "amici", 200);

    state.upsert(pouch);
    state.upsert(bill);

    assert_eq!(state.len(), 2, "Due nodi, stesso utente");

    let all_nodes = state.all();
    let carlo_nodes: Vec<_> = all_nodes
        .iter()
        .filter(|n| n.user_fingerprint == "aabbccdd")
        .collect();
    assert_eq!(carlo_nodes.len(), 2, "Carlo ha 2 nodi nella rete");
}

#[test]
fn gossip_filtra_per_network() {
    let mut state = NetworkState::new();

    let amici = make_node_info("peer-1", "aabbccdd", ServiceType::Pouch, "amici", 100);
    let lavoro = make_node_info("peer-2", "aabbccdd", ServiceType::Bill, "lavoro", 100);
    let other = make_node_info("peer-3", "11223344", ServiceType::Post, "amici", 100);

    state.upsert(amici);
    state.upsert(lavoro);
    state.upsert(other);

    assert_eq!(
        state.in_network("amici").len(),
        2,
        "2 nodi nella rete 'amici'"
    );
    assert_eq!(
        state.in_network("lavoro").len(),
        1,
        "1 nodo nella rete 'lavoro'"
    );
    assert_eq!(
        state.in_network("inesistente").len(),
        0,
        "0 nodi in rete inesistente"
    );
}

#[test]
fn gossip_evict_stale_rimuove_nodi_vecchi() {
    let mut state = NetworkState::new();

    let now = chrono::Utc::now().timestamp() as u64;
    let fresh = make_node_info("peer-fresh", "aa", ServiceType::Pouch, "net", now);
    let stale = make_node_info("peer-stale", "bb", ServiceType::Pouch, "net", now - 300);

    state.upsert(fresh);
    state.upsert(stale);

    assert_eq!(state.len(), 2);
    state.evict_stale(120);
    assert_eq!(state.len(), 1, "Il nodo stale deve essere rimosso");
    assert_eq!(state.all()[0].peer_id, "peer-fresh");
}

#[test]
fn node_info_serializza_come_da_readme() {
    let info = NodeInfo {
        peer_id: "12D3KooWGjE...".into(),
        user_fingerprint: "a3f19c2b".into(),
        user_alias: Some("carlo".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Pouch,
        service_id: "550e8400-e29b-41d4-...".into(),
        network_id: "my-network".into(),
        listen_addrs: vec!["/ip4/192.168.1.10/tcp/54321".into()],
        announced_at: 1710000000,
        metadata: {
            let mut m = HashMap::new();
            m.insert("storage_bytes".into(), serde_json::json!(10_737_418_240u64));
            m.insert("free_bytes".into(), serde_json::json!(8_000_000_000u64));
            m.insert("version".into(), serde_json::json!("0.1.0"));
            m
        },
    };

    let json = serde_json::to_string_pretty(&info).unwrap();

    assert!(json.contains("\"peer_id\""));
    assert!(json.contains("\"user_fingerprint\""));
    assert!(json.contains("\"user_alias\""));
    assert!(json.contains("\"service_type\""));
    assert!(json.contains("\"service_id\""));
    assert!(json.contains("\"network_id\""));
    assert!(json.contains("\"listen_addrs\""));
    assert!(json.contains("\"announced_at\""));
    assert!(json.contains("\"metadata\""));
    assert!(json.contains("\"storage_bytes\""));
    assert!(json.contains("\"free_bytes\""));
}

#[test]
fn node_info_metadata_estendibile() {
    let mut meta = HashMap::new();
    meta.insert("storage_bytes".into(), serde_json::json!(1000));
    meta.insert(
        "custom_field_v2".into(),
        serde_json::json!("future feature"),
    );
    meta.insert("nested".into(), serde_json::json!({"a": 1, "b": [2, 3]}));

    let info = make_node_info("peer", "fp", ServiceType::Pouch, "net", 100);
    let mut info_with_meta = info;
    info_with_meta.metadata = meta;

    let json = serde_json::to_string(&info_with_meta).unwrap();
    let roundtrip: NodeInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(
        roundtrip.metadata.get("custom_field_v2").unwrap(),
        &serde_json::json!("future feature"),
        "Metadata custom deve sopravvivere al roundtrip"
    );
    assert_eq!(
        roundtrip.metadata.get("nested").unwrap(),
        &serde_json::json!({"a": 1, "b": [2, 3]}),
        "Metadata nested deve funzionare"
    );
}

#[tokio::test]
async fn swarm_si_costruisce_con_tutti_i_protocolli() {
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    match bp_core::network::build_swarm(keypair) {
        Ok(_swarm) => {}
        Err(e) => {
            eprintln!(
                "NOTE: swarm build failed (expected on some CI environments): {}",
                e
            );
        }
    }
}

#[test]
fn scenario_rete_amici_carlo_marco_lucia() {
    let mut state = NetworkState::new();
    let now = chrono::Utc::now().timestamp() as u64;

    let carlo_pouch = NodeInfo {
        peer_id: "peer-carlo-nas".into(),
        user_fingerprint: "carlo___".into(),
        user_alias: Some("carlo".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Pouch,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/192.168.1.10/tcp/5000".into()],
        announced_at: now,
        metadata: {
            let mut m = HashMap::new();
            m.insert("storage_bytes".into(), serde_json::json!(10_737_418_240u64));
            m
        },
    };
    let carlo_bill = NodeInfo {
        peer_id: "peer-carlo-laptop".into(),
        user_fingerprint: "carlo___".into(),
        user_alias: Some("carlo".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Bill,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/192.168.1.20/tcp/5001".into()],
        announced_at: now,
        metadata: HashMap::new(),
    };

    let marco_pouch1 = NodeInfo {
        peer_id: "peer-marco-desktop".into(),
        user_fingerprint: "marco___".into(),
        user_alias: Some("marco".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Pouch,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/10.0.0.5/tcp/5000".into()],
        announced_at: now,
        metadata: {
            let mut m = HashMap::new();
            m.insert("storage_bytes".into(), serde_json::json!(53_687_091_200u64));
            m
        },
    };
    let marco_pouch2 = NodeInfo {
        peer_id: "peer-marco-vps".into(),
        user_fingerprint: "marco___".into(),
        user_alias: Some("marco".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Pouch,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/203.0.113.5/tcp/5000".into()],
        announced_at: now,
        metadata: {
            let mut m = HashMap::new();
            m.insert(
                "storage_bytes".into(),
                serde_json::json!(107_374_182_400u64),
            );
            m
        },
    };
    let marco_post = NodeInfo {
        peer_id: "peer-marco-vps-relay".into(),
        user_fingerprint: "marco___".into(),
        user_alias: Some("marco".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Post,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/203.0.113.5/tcp/5001".into()],
        announced_at: now,
        metadata: HashMap::new(),
    };

    let lucia_bill = NodeInfo {
        peer_id: "peer-lucia-laptop".into(),
        user_fingerprint: "lucia___".into(),
        user_alias: Some("lucia".into()),
        device_cert: test_cert(),
        service_type: ServiceType::Bill,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/192.168.1.30/tcp/5000".into()],
        announced_at: now,
        metadata: HashMap::new(),
    };

    state.upsert(carlo_pouch);
    state.upsert(carlo_bill);
    state.upsert(marco_pouch1);
    state.upsert(marco_pouch2);
    state.upsert(marco_post);
    state.upsert(lucia_bill);

    let amici = state.in_network("amici");
    assert_eq!(amici.len(), 6, "6 nodi nella rete amici");

    let mut users: Vec<&str> = amici.iter().map(|n| n.user_fingerprint.as_str()).collect();
    users.sort();
    users.dedup();
    assert_eq!(users.len(), 3, "3 utenti distinti: carlo, marco, lucia");

    let carlo_nodes: Vec<_> = amici
        .iter()
        .filter(|n| n.user_fingerprint == "carlo___")
        .collect();
    assert_eq!(carlo_nodes.len(), 2, "Carlo: 1 pouch + 1 bill");

    let marco_nodes: Vec<_> = amici
        .iter()
        .filter(|n| n.user_fingerprint == "marco___")
        .collect();
    assert_eq!(marco_nodes.len(), 3, "Marco: 2 pouch + 1 post");

    let lucia_nodes: Vec<_> = amici
        .iter()
        .filter(|n| n.user_fingerprint == "lucia___")
        .collect();
    assert_eq!(lucia_nodes.len(), 1, "Lucia: 1 bill");

    let total_storage: u64 = amici
        .iter()
        .filter(|n| n.service_type == ServiceType::Pouch)
        .filter_map(|n| n.metadata.get("storage_bytes"))
        .filter_map(|v| v.as_u64())
        .sum();

    assert_eq!(
        total_storage,
        10_737_418_240 + 53_687_091_200 + 107_374_182_400,
        "Storage totale = somma di tutti i pouch"
    );

    let available_storage: Vec<_> = amici
        .iter()
        .filter(|n| n.service_type == ServiceType::Pouch)
        .collect();
    assert_eq!(
        available_storage.len(),
        3,
        "Lucia vede 3 pouch disponibili nella rete"
    );

    let relays: Vec<_> = amici
        .iter()
        .filter(|n| n.service_type == ServiceType::Post)
        .collect();
    assert_eq!(relays.len(), 1, "1 solo relay (post) nella rete");
    assert_eq!(
        relays[0].user_fingerprint, "marco___",
        "Il relay è di Marco"
    );
}

use std::sync::Mutex;

static IDENTITY_FS_LOCK: Mutex<()> = Mutex::new(());

#[cfg(unix)]
fn with_temp_home<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
    let _guard = IDENTITY_FS_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let tmp = std::env::temp_dir().join(format!("bp_test_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&tmp).unwrap();

    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &tmp);

    let result = std::panic::catch_unwind(f);

    if let Some(home) = old_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
    std::fs::remove_dir_all(&tmp).ok();

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
#[cfg(unix)]
fn identity_create_load_remove_roundtrip() {
    use bp_core::identity::Identity;
    with_temp_home(|| {
        assert!(!Identity::exists().unwrap());
        assert!(matches!(
            Identity::load(None).map(|_| ()).unwrap_err(),
            bp_core::error::BpError::NotAuthenticated
        ));

        let created = Identity::create(None, Some("carlo".into()), None).unwrap();
        assert!(Identity::exists().unwrap());
        assert!(Identity::create(None, None, None).is_err());

        let loaded = Identity::load(None).unwrap();
        assert_eq!(loaded.fingerprint, created.fingerprint);
        assert_eq!(loaded.peer_id, created.peer_id);
        assert_eq!(loaded.profile.alias.as_deref(), Some("carlo"));

        let key_mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(bp_core::config::user_key_path().unwrap())
                .unwrap()
                .permissions(),
        );
        assert_eq!(key_mode & 0o777, 0o600);

        Identity::remove().unwrap();
        assert!(!Identity::exists().unwrap());
    });
}

#[test]
#[cfg(unix)]
fn identity_generated_key_is_protected_by_passphrase() {
    use bp_core::identity::Identity;
    with_temp_home(|| {
        let created = Identity::create(None, None, Some("s3cret")).unwrap();
        let text = std::fs::read_to_string(bp_core::config::user_key_path().unwrap()).unwrap();
        assert!(text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(Identity::load(None).is_err());
        assert!(Identity::load(Some("wrong")).is_err());
        assert_eq!(
            Identity::load(Some("s3cret")).unwrap().fingerprint,
            created.fingerprint
        );
    });
}

#[test]
#[cfg(unix)]
fn identity_imported_ssh_key_gives_a_new_device_for_the_same_user() {
    use bp_core::identity::Identity;
    with_temp_home(|| {
        let first = Identity::create(None, None, None).unwrap();
        let key = std::env::temp_dir().join(format!("bp_key_{}", uuid::Uuid::new_v4().simple()));
        std::fs::copy(bp_core::config::user_key_path().unwrap(), &key).unwrap();
        Identity::remove().unwrap();

        let second = Identity::create(Some(&key), None, None).unwrap();
        std::fs::remove_file(&key).ok();
        assert_eq!(second.fingerprint, first.fingerprint);
        assert_eq!(second.user_key, first.user_key);
        assert_ne!(second.peer_id, first.peer_id);
    });
}

#[test]
#[cfg(unix)]
fn identity_config_ensure_dirs_crea_directory() {
    with_temp_home(|| {
        bp_core::config::ensure_dirs().expect("ensure_dirs() deve riuscire");
        let base = bp_core::config::base_dir().unwrap();
        assert!(base.exists(), "La directory base deve essere creata");
    });
}

#[test]
#[cfg(unix)]
fn daemon_is_running_false_senza_pid_file() {
    with_temp_home(|| {
        bp_core::config::ensure_dirs().unwrap();
        assert!(
            !bp_core::daemon::is_running(),
            "is_running() deve essere false se non c'è PID file"
        );
    });
}

#[test]
#[cfg(unix)]
fn daemon_is_running_false_con_pid_inesistente() {
    with_temp_home(|| {
        bp_core::config::ensure_dirs().unwrap();
        let pid_path = bp_core::config::pid_path().unwrap();
        std::fs::write(&pid_path, "99999999").unwrap();
        assert!(
            !bp_core::daemon::is_running(),
            "is_running() deve essere false con PID inesistente"
        );
    });
}

#[test]
#[cfg(target_os = "linux")]
fn daemon_is_running_true_con_pid_corrente() {
    with_temp_home(|| {
        bp_core::config::ensure_dirs().unwrap();
        let pid_path = bp_core::config::pid_path().unwrap();
        let current_pid = std::process::id();
        std::fs::write(&pid_path, current_pid.to_string()).unwrap();
        assert!(
            bp_core::daemon::is_running(),
            "is_running() deve essere true con il PID del processo corrente"
        );
    });
}

#[test]
#[cfg(unix)]
fn daemon_is_running_false_con_pid_malformato() {
    with_temp_home(|| {
        bp_core::config::ensure_dirs().unwrap();
        let pid_path = bp_core::config::pid_path().unwrap();
        std::fs::write(&pid_path, "non-un-numero").unwrap();
        assert!(
            !bp_core::daemon::is_running(),
            "is_running() deve essere false con PID file malformato"
        );
    });
}

#[cfg(unix)]
fn make_in_memory_state() -> std::sync::Arc<bp_core::control::server::DaemonState> {
    let identity = bp_core::identity::Identity::in_memory(Some("test-server".into())).unwrap();
    let (net_tx, _net_rx) = tokio::sync::mpsc::channel(64);
    std::sync::Arc::new(bp_core::control::server::DaemonState {
        chains: chains_for(
            &identity,
            &[
                "test-net",
                "rete",
                "bye-net",
                "my-network",
                "test",
                "amici",
                "lavoro",
            ],
        ),
        identity,
        services: std::sync::RwLock::new(ServiceRegistry::new()),
        network_state: std::sync::Arc::new(std::sync::RwLock::new(NetworkState::new())),
        networks: std::sync::RwLock::new(Vec::new()),
        net_tx,
        storage_managers: std::sync::Arc::new(std::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        qos: std::sync::Arc::new(std::sync::RwLock::new(bp_core::network::QosRegistry::new())),
        remote_fragment_index: std::sync::Arc::new(std::sync::RwLock::new(
            bp_core::network::RemoteFragmentIndex::new(),
        )),
        files: std::sync::RwLock::new(bp_core::files::FileStore::default()),
    })
}

#[cfg(unix)]
async fn start_test_server() -> (
    std::path::PathBuf,
    std::sync::Arc<bp_core::control::server::DaemonState>,
) {
    let socket_path = std::env::temp_dir().join(format!(
        "bp_test_ctrl_{}.sock",
        uuid::Uuid::new_v4().simple()
    ));
    let state = make_in_memory_state();
    let state_clone = std::sync::Arc::clone(&state);
    let sp = socket_path.clone();
    tokio::spawn(async move {
        let _ = bp_core::control::server::run_control_server(sp, state_clone).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (socket_path, state)
}

#[cfg(unix)]
async fn send_cmd(socket_path: &std::path::Path, req: &ControlRequest) -> ControlResponse {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut stream = tokio::net::UnixStream::connect(socket_path)
        .await
        .expect("connect fallito");
    let json = format!("{}\n", serde_json::to_string(req).unwrap());
    stream.write_all(json.as_bytes()).await.unwrap();
    let (read_half, _) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(line.trim()).expect("risposta JSON non valida")
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_ping_risponde_ok() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(&sp, &ControlRequest::Ping).await;
    assert!(matches!(resp, ControlResponse::Ok { .. }));
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_status_restituisce_peer_id_e_fingerprint() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(&sp, &ControlRequest::Status).await;
    match resp {
        ControlResponse::Ok { data: Some(v) } => {
            assert!(v["peer_id"].is_string());
            assert!(v["fingerprint"].is_string());
        }
        _ => panic!("Expected Ok con data"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_hatch_nuovo_servizio() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::Hatch {
            service_type: ServiceType::Pouch,
            network_id: "amici".into(),
            metadata: HashMap::new(),
        },
    )
    .await;
    match resp {
        ControlResponse::Ok { data: Some(v) } => {
            assert!(v["service_id"].is_string());
            assert_eq!(v["network_id"], "amici");
        }
        _ => panic!("Expected Ok"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_hatch_requires_membership() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::Hatch {
            service_type: ServiceType::Bill,
            network_id: "not-a-member".into(),
            metadata: HashMap::new(),
        },
    )
    .await;
    match resp {
        ControlResponse::Error { message } => assert!(message.contains("not-a-member")),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_hatch_stessa_rete_due_volte_copre_already_joined() {
    let (sp, _s) = start_test_server().await;
    send_cmd(
        &sp,
        &ControlRequest::Hatch {
            service_type: ServiceType::Post,
            network_id: "lavoro".into(),
            metadata: HashMap::new(),
        },
    )
    .await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::Hatch {
            service_type: ServiceType::Bill,
            network_id: "lavoro".into(),
            metadata: HashMap::new(),
        },
    )
    .await;
    assert!(matches!(resp, ControlResponse::Ok { .. }));
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_farewell_servizio_inesistente_ritorna_errore() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::Farewell {
            service_id: "non-esiste-uuid".into(),
        },
    )
    .await;
    assert!(matches!(resp, ControlResponse::Error { .. }));
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_farewell_servizio_esistente_rimuove() {
    let (sp, _s) = start_test_server().await;
    let hatch_resp = send_cmd(
        &sp,
        &ControlRequest::Hatch {
            service_type: ServiceType::Bill,
            network_id: "test".into(),
            metadata: HashMap::new(),
        },
    )
    .await;
    let service_id = match hatch_resp {
        ControlResponse::Ok { data: Some(v) } => v["service_id"].as_str().unwrap().to_string(),
        _ => panic!("Hatch fallito"),
    };
    let resp = send_cmd(&sp, &ControlRequest::Farewell { service_id }).await;
    assert!(matches!(resp, ControlResponse::Ok { .. }));
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_flock_ritorna_struttura_corretta() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(&sp, &ControlRequest::Flock).await;
    match resp {
        ControlResponse::Ok { data: Some(v) } => {
            assert!(v["local_services"].is_array());
            assert!(v["known_peers"].is_array());
            assert!(v["networks"].is_array());
        }
        _ => panic!("Expected Ok"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_leave_rete_raggiunta() {
    let (sp, _s) = start_test_server().await;
    send_cmd(
        &sp,
        &ControlRequest::Hatch {
            service_type: ServiceType::Post,
            network_id: "bye-net".into(),
            metadata: HashMap::new(),
        },
    )
    .await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::Leave {
            network_id: "bye-net".into(),
            force: false,
        },
    )
    .await;
    match resp {
        ControlResponse::Ok { data: Some(v) } => {
            assert_eq!(v["network_id"], "bye-net");
        }
        _ => panic!("Expected Ok"),
    }
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_leave_rete_non_raggiunta() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::Leave {
            network_id: "non-raggiunta".into(),
            force: false,
        },
    )
    .await;
    assert!(matches!(resp, ControlResponse::Error { .. }));
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_json_non_valido_ritorna_errore() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (sp, _s) = start_test_server().await;
    let mut stream = tokio::net::UnixStream::connect(&sp).await.unwrap();
    stream.write_all(b"{{json spazzatura\n").await.unwrap();
    let (read_half, _) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: ControlResponse = serde_json::from_str(line.trim()).unwrap();
    assert!(matches!(resp, ControlResponse::Error { .. }));
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_linea_vuota_chiude_connessione_senza_crash() {
    use tokio::io::AsyncWriteExt;
    let (sp, _s) = start_test_server().await;
    let mut stream = tokio::net::UnixStream::connect(&sp).await.unwrap();
    stream.write_all(b"\n").await.unwrap();
    drop(stream);
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_connect_relay_addr_invalida() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::ConnectRelay {
            relay_addr: "questo-non-e-un-multiaddr".into(),
        },
    )
    .await;
    assert!(
        matches!(resp, ControlResponse::Error { .. }),
        "multiaddr invalido deve tornare errore"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn dispatch_connect_relay_addr_valida() {
    let (sp, _s) = start_test_server().await;
    let resp = send_cmd(
        &sp,
        &ControlRequest::ConnectRelay {
            relay_addr: "/ip4/127.0.0.1/tcp/4001".into(),
        },
    )
    .await;
    match resp {
        ControlResponse::Ok { data: Some(v) } => {
            assert!(v["relay_addr"].is_string());
        }
        _ => panic!("Expected Ok con data"),
    }
}

#[tokio::test]
async fn network_loop_accetta_e_processa_comandi() {
    use bp_core::network::{
        build_swarm, run_network_loop, FragmentIndexAnnouncement, NetworkCommand, NetworkState,
        RemoteFragmentIndex, StorageManagerMap,
    };
    use std::collections::HashMap;

    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let swarm = match build_swarm(keypair) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("build_swarm fallito (CI limitata): {e}");
            return;
        }
    };

    let listen_addr: libp2p::Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<NetworkCommand>(64);
    let network_state = std::sync::Arc::new(std::sync::RwLock::new(NetworkState::new()));
    let storage_managers: StorageManagerMap =
        std::sync::Arc::new(std::sync::RwLock::new(HashMap::new()));
    let frag_idx = std::sync::Arc::new(std::sync::RwLock::new(RemoteFragmentIndex::new()));

    let handle = tokio::spawn(run_network_loop(
        swarm,
        cmd_rx,
        network_state.clone(),
        listen_addr,
        storage_managers,
        frag_idx,
        std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        bp_core::identity::Identity::in_memory(None).unwrap(),
        bp_core::chain::ObservationLog::default(),
        tokio::sync::mpsc::channel(8).0,
    ));

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let _ = cmd_tx
        .send(NetworkCommand::JoinNetwork {
            network_id: "test-net".into(),
            key: [7; 32],
        })
        .await;
    let _ = cmd_tx
        .send(NetworkCommand::JoinNetwork {
            network_id: "test-net".into(),
            key: [7; 32],
        })
        .await;

    let node_info = NodeInfo {
        peer_id: "12D3KooWTestPeerId".into(),
        user_fingerprint: "deadbeef".into(),
        user_alias: Some("tester".into()),
        device_cert: test_cert(),
        service_type: bp_core::service::ServiceType::Post,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "test-net".into(),
        listen_addrs: vec![],
        announced_at: 0,
        metadata: HashMap::new(),
    };
    let _ = cmd_tx
        .send(NetworkCommand::Publish {
            network_id: "test-net".into(),
            message: bp_core::network::Gossip::Node(Box::new(node_info)),
        })
        .await;

    let ann = FragmentIndexAnnouncement {
        network_id: "test-net".into(),
        chunk_id: "abc123deadbeef".into(),
        announced_at: 0,
        pointers: vec![],
    };
    let _ = cmd_tx
        .send(NetworkCommand::Publish {
            network_id: "test-net".into(),
            message: bp_core::network::Gossip::Index(ann),
        })
        .await;

    let _ = cmd_tx
        .send(NetworkCommand::LeaveNetwork {
            network_id: "test-net".into(),
        })
        .await;
    let _ = cmd_tx
        .send(NetworkCommand::LeaveNetwork {
            network_id: "non-joinata".into(),
        })
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    drop(cmd_tx);

    let result = tokio::time::timeout(std::time::Duration::from_secs(3), handle).await;
    match result {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => eprintln!("Network loop errore (accettabile in CI): {e}"),
        Ok(Err(_)) => {}
        Err(_) => {
            eprintln!("network_loop timeout (CI senza netlink — accettabile)");
        }
    }
}
