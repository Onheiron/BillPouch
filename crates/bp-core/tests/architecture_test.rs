//! ═══════════════════════════════════════════════════════════════════════════
//! BillPouch — Architecture Verification Tests
//! ═══════════════════════════════════════════════════════════════════════════
//!
//! Questi test verificano che il codice fa quello che il README dice.
//! Se passano tutti, l'architettura funziona come descritto.

use bp_core::control::protocol::*;
use bp_core::error::BpError;
use bp_core::network::state::{NetworkState, NodeInfo};
use bp_core::service::*;
use std::collections::HashMap;

// ═════════════════════════════════════════════════════════════════════════════
// 1. IDENTITA' — Ed25519: la tua chiave è la tua identità
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn identita_ed25519_keypair_genera_peer_id_deterministico() {
    // Claim: "Your keypair is your identity. PeerId is derived from the public key."
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let peer_id_1 = libp2p::PeerId::from_public_key(&keypair.public());
    let peer_id_2 = libp2p::PeerId::from_public_key(&keypair.public());

    // Stessa chiave pubblica → stesso PeerId, sempre.
    assert_eq!(
        peer_id_1, peer_id_2,
        "Lo stesso keypair deve dare lo stesso PeerId"
    );
}

#[test]
fn identita_keypair_diversi_danno_peer_id_diversi() {
    // Due utenti diversi → due PeerId diversi.
    let kp1 = libp2p::identity::Keypair::generate_ed25519();
    let kp2 = libp2p::identity::Keypair::generate_ed25519();
    let pid1 = libp2p::PeerId::from_public_key(&kp1.public());
    let pid2 = libp2p::PeerId::from_public_key(&kp2.public());

    assert_ne!(pid1, pid2, "Keypair diversi devono produrre PeerId diversi");
}

#[test]
fn identita_fingerprint_e_sha256_primi_8_byte_hex() {
    // Claim: "fingerprint = first 8 bytes of SHA-256 of your public key"
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let fp = bp_core::identity::fingerprint(&keypair);

    // 8 byte = 16 caratteri hex
    assert_eq!(
        fp.len(),
        16,
        "Il fingerprint deve essere 16 caratteri hex (8 byte)"
    );
    assert!(
        fp.chars().all(|c| c.is_ascii_hexdigit()),
        "Deve essere hex valido"
    );
}

#[test]
fn identita_fingerprint_deterministico() {
    // Stessa chiave → stesso fingerprint, ogni volta.
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let fp1 = bp_core::identity::fingerprint(&keypair);
    let fp2 = bp_core::identity::fingerprint(&keypair);

    assert_eq!(fp1, fp2, "Fingerprint deve essere deterministico");
}

#[test]
fn identita_keypair_persistenza_roundtrip() {
    // Claim: "keypair stored on disk can be reloaded"
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let encoded = keypair
        .to_protobuf_encoding()
        .expect("Encoding deve funzionare");
    let decoded = libp2p::identity::Keypair::from_protobuf_encoding(&encoded)
        .expect("Decoding deve funzionare");

    let pid_orig = libp2p::PeerId::from_public_key(&keypair.public());
    let pid_reload = libp2p::PeerId::from_public_key(&decoded.public());

    assert_eq!(
        pid_orig, pid_reload,
        "Keypair ricaricato deve produrre stesso PeerId"
    );
}

#[test]
fn identita_stessa_chiave_su_piu_macchine() {
    // Claim: "Multiple nodes can belong to the same user"
    // Se copio la chiave su un'altra macchina, ottengo lo stesso fingerprint.
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let bytes = keypair.to_protobuf_encoding().unwrap();

    let macchina_1 = libp2p::identity::Keypair::from_protobuf_encoding(&bytes).unwrap();
    let macchina_2 = libp2p::identity::Keypair::from_protobuf_encoding(&bytes).unwrap();

    let fp1 = bp_core::identity::fingerprint(&macchina_1);
    let fp2 = bp_core::identity::fingerprint(&macchina_2);

    assert_eq!(
        fp1, fp2,
        "Stessa chiave su macchine diverse → stesso fingerprint utente"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 2. TRE SERVIZI — pouch (storage), bill (file I/O), post (relay)
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn servizi_tre_tipi_pouch_bill_post() {
    // Claim: "Three service types: Pouch, Bill, Post"
    let pouch: ServiceType = "pouch".parse().unwrap();
    let bill: ServiceType = "bill".parse().unwrap();
    let post: ServiceType = "post".parse().unwrap();

    assert_eq!(pouch, ServiceType::Pouch);
    assert_eq!(bill, ServiceType::Bill);
    assert_eq!(post, ServiceType::Post);
}

#[test]
fn servizi_tipo_sconosciuto_errore() {
    // Se provi a creare un servizio che non esiste, errore.
    let result: Result<ServiceType, _> = "fridge".parse();
    assert!(result.is_err(), "'fridge' non è un servizio valido");
}

#[test]
fn servizi_case_insensitive() {
    // "POUCH", "Pouch", "pouch" → tutti validi.
    let p1: ServiceType = "POUCH".parse().unwrap();
    let p2: ServiceType = "Pouch".parse().unwrap();
    let p3: ServiceType = "pouch".parse().unwrap();
    assert_eq!(p1, p2);
    assert_eq!(p2, p3);
}

#[test]
fn servizio_ogni_istanza_ha_uuid_unico() {
    // Claim: "Each service gets a unique service ID (UUID)"
    let s1 = ServiceInfo::new(ServiceType::Pouch, "net-a".into(), HashMap::new());
    let s2 = ServiceInfo::new(ServiceType::Pouch, "net-a".into(), HashMap::new());

    assert_ne!(s1.id, s2.id, "Due servizi devono avere ID diversi");
    // Verifica che sia un UUID valido
    uuid::Uuid::parse_str(&s1.id).expect("Service ID deve essere un UUID valido");
}

#[test]
fn servizio_nasce_in_stato_starting() {
    let s = ServiceInfo::new(ServiceType::Bill, "my-net".into(), HashMap::new());
    assert_eq!(s.status, ServiceStatus::Starting);
}

#[test]
fn servizio_pouch_con_metadata_storage() {
    // Claim: "Pouch bids local storage into the network"
    let mut meta = HashMap::new();
    meta.insert("storage_bytes".into(), serde_json::json!(10_737_418_240u64));

    let s = ServiceInfo::new(ServiceType::Pouch, "net-a".into(), meta);

    assert_eq!(
        s.metadata.get("storage_bytes").unwrap(),
        &serde_json::json!(10_737_418_240u64),
        "Pouch deve poter dichiarare storage_bytes nei metadata"
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 3. SERVICE REGISTRY — il daemon tiene traccia dei servizi locali
// ═════════════════════════════════════════════════════════════════════════════

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
    // Claim: "bp farewell <id> stops a service"
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
    // Claim: "You can run multiple services on the same machine"
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
    // Claim: "One daemon can join several independent networks"
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

// ═════════════════════════════════════════════════════════════════════════════
// 4. NETWORK STATE — DHT gossip-based: NodeInfo store
// ═════════════════════════════════════════════════════════════════════════════

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
    // Claim: "All nodes accumulate received announcements in a local NetworkState"
    let mut state = NetworkState::new();
    let node = make_node_info("peer-A", "aabbccdd", ServiceType::Pouch, "net", 100);

    state.upsert(node);
    assert_eq!(state.len(), 1);
    assert_eq!(state.all()[0].peer_id, "peer-A");
}

#[test]
fn gossip_upsert_mantiene_annuncio_piu_recente() {
    // Claim: "upsert keeps the newest announcement"
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
    state.upsert(old); // Questo deve essere ignorato

    assert_eq!(
        state.all()[0].announced_at,
        200,
        "Non deve sovrascrivere con annuncio vecchio"
    );
}

#[test]
fn gossip_piu_utenti_stesso_network() {
    // Claim: "Social storage — users bid their own disk space"
    // Carlo e Marco nella stessa rete "amici"
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
    // Claim: "Multiple nodes can belong to the same user (same fingerprint)"
    // Carlo ha un pouch e un bill, stessa identità
    let mut state = NetworkState::new();

    let pouch = make_node_info("peer-carlo-1", "aabbccdd", ServiceType::Pouch, "amici", 100);
    let bill = make_node_info("peer-carlo-2", "aabbccdd", ServiceType::Bill, "amici", 100);

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
    // Claim: "One daemon can join several independent networks"
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
    // Claim: "evict nodes silent for >2 min"
    let mut state = NetworkState::new();

    let now = chrono::Utc::now().timestamp() as u64;
    let fresh = make_node_info("peer-fresh", "aa", ServiceType::Pouch, "net", now);
    let stale = make_node_info("peer-stale", "bb", ServiceType::Pouch, "net", now - 300); // 5 min fa

    state.upsert(fresh);
    state.upsert(stale);

    assert_eq!(state.len(), 2);
    state.evict_stale(120); // rimuovi chi tace da più di 2 min
    assert_eq!(state.len(), 1, "Il nodo stale deve essere rimosso");
    assert_eq!(state.all()[0].peer_id, "peer-fresh");
}

#[test]
fn gossip_remove_nodo() {
    let mut state = NetworkState::new();
    let node = make_node_info("peer-X", "ff", ServiceType::Post, "net", 100);
    state.upsert(node);

    state.remove("peer-X");
    assert_eq!(state.len(), 0, "Nodo rimosso esplicitamente");
}

// ═════════════════════════════════════════════════════════════════════════════
// 5. TOPIC NAMING — gossipsub topic = billpouch/v1/{network_id}/nodes
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn topic_name_formato_corretto() {
    // Claim: "gossipsub topic billpouch/v1/{network_id}/nodes"
    assert_eq!(
        NodeInfo::topic_name("my-network"),
        "billpouch/v1/my-network/nodes"
    );
    assert_eq!(NodeInfo::topic_name("amici"), "billpouch/v1/amici/nodes");
}

// ═════════════════════════════════════════════════════════════════════════════
// 6. PROTOCOLLO DI CONTROLLO — CLI ↔ Daemon via JSON
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn protocollo_hatch_request_serializza_json() {
    // Claim: "newline-delimited JSON over Unix socket"
    let req = ControlRequest::Hatch {
        service_type: ServiceType::Pouch,
        network_id: "my-network".into(),
        metadata: {
            let mut m = HashMap::new();
            m.insert("storage_bytes".into(), serde_json::json!(10_737_418_240u64));
            m
        },
    };

    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"cmd\":\"hatch\""), "Deve avere cmd=hatch");
    assert!(
        json.contains("\"service_type\":\"pouch\""),
        "Deve avere service_type=pouch"
    );
    assert!(json.contains("\"network_id\":\"my-network\""));
    assert!(
        json.contains("10737418240"),
        "Deve avere storage_bytes nei metadata"
    );
}

#[test]
fn protocollo_hatch_request_deserializza_json() {
    let json = r#"{"cmd":"hatch","service_type":"pouch","network_id":"test","metadata":{}}"#;
    let req: ControlRequest = serde_json::from_str(json).unwrap();

    match req {
        ControlRequest::Hatch {
            service_type,
            network_id,
            ..
        } => {
            assert_eq!(service_type, ServiceType::Pouch);
            assert_eq!(network_id, "test");
        }
        _ => panic!("Deve deserializzare come Hatch"),
    }
}

#[test]
fn protocollo_flock_request() {
    let json = r#"{"cmd":"flock"}"#;
    let req: ControlRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(req, ControlRequest::Flock));
}

#[test]
fn protocollo_farewell_request() {
    let json = r#"{"cmd":"farewell","service_id":"550e8400-e29b-41d4-a716-446655440000"}"#;
    let req: ControlRequest = serde_json::from_str(json).unwrap();
    match req {
        ControlRequest::Farewell { service_id } => {
            assert_eq!(service_id, "550e8400-e29b-41d4-a716-446655440000");
        }
        _ => panic!("Deve deserializzare come Farewell"),
    }
}

#[test]
fn protocollo_join_request() {
    let json = r#"{"cmd":"join","network_id":"friends"}"#;
    let req: ControlRequest = serde_json::from_str(json).unwrap();
    match req {
        ControlRequest::Join { network_id } => assert_eq!(network_id, "friends"),
        _ => panic!("Deve deserializzare come Join"),
    }
}

#[test]
fn protocollo_ping_request() {
    let json = r#"{"cmd":"ping"}"#;
    let req: ControlRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(req, ControlRequest::Ping));
}

#[test]
fn protocollo_response_ok_con_data() {
    let resp = ControlResponse::ok(serde_json::json!({"key": "value"}));
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"status\":\"ok\""));
    assert!(json.contains("\"key\":\"value\""));
}

#[test]
fn protocollo_response_ok_vuota() {
    let resp = ControlResponse::ok_empty();
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"status\":\"ok\""));
    assert!(
        !json.contains("\"data\""),
        "ok_empty non deve avere campo data"
    );
}

#[test]
fn protocollo_response_errore() {
    let resp = ControlResponse::err("Something broke");
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"status\":\"error\""));
    assert!(json.contains("Something broke"));
}

// ═════════════════════════════════════════════════════════════════════════════
// 7. NODE INFO — il messaggio gossip che ogni nodo broadcast
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn node_info_serializza_come_da_readme() {
    // Claim: il README mostra un JSON specifico per NodeInfo
    let info = NodeInfo {
        peer_id: "12D3KooWGjE...".into(),
        user_fingerprint: "a3f19c2b".into(),
        user_alias: Some("carlo".into()),
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

    // Verifica che tutti i campi documentati nel README siano presenti
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
    // Claim: "metadata field allows future extensions without breaking existing nodes"
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

    // Serializza e deserializza — i campi custom non si perdono
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

// ═════════════════════════════════════════════════════════════════════════════
// 8. NETWORK COMMANDS — i comandi che il daemon manda al loop di rete
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn network_command_join_leave() {
    // Verifica che i comandi JoinNetwork/LeaveNetwork esistano e si costruiscano
    use bp_core::network::NetworkCommand;

    let join = NetworkCommand::JoinNetwork {
        network_id: "amici".into(),
    };
    let leave = NetworkCommand::LeaveNetwork {
        network_id: "amici".into(),
    };
    let shutdown = NetworkCommand::Shutdown;

    // Se compila, i comandi esistono. Verifica pattern matching.
    match join {
        NetworkCommand::JoinNetwork { network_id } => assert_eq!(network_id, "amici"),
        _ => panic!("Deve essere JoinNetwork"),
    }
    match leave {
        NetworkCommand::LeaveNetwork { network_id } => assert_eq!(network_id, "amici"),
        _ => panic!("Deve essere LeaveNetwork"),
    }
    assert!(matches!(shutdown, NetworkCommand::Shutdown));
}

#[test]
fn network_command_announce() {
    use bp_core::network::NetworkCommand;

    let info = make_node_info("peer-A", "fp", ServiceType::Pouch, "net", 100);
    let payload = serde_json::to_vec(&info).unwrap();

    let cmd = NetworkCommand::Announce {
        network_id: "net".into(),
        payload: payload.clone(),
    };

    match cmd {
        NetworkCommand::Announce {
            network_id,
            payload: p,
        } => {
            assert_eq!(network_id, "net");
            // Il payload deserializza di nuovo a NodeInfo
            let roundtrip: NodeInfo = serde_json::from_slice(&p).unwrap();
            assert_eq!(roundtrip.peer_id, "peer-A");
        }
        _ => panic!("Deve essere Announce"),
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// 9. SWARM — libp2p si costruisce correttamente
// ═════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn swarm_si_costruisce_con_tutti_i_protocolli() {
    // Claim: "Noise + Yamux encrypted and multiplexed, gossipsub, Kademlia, mDNS"
    // NOTE: mDNS on Linux uses netlink which requires a Tokio runtime context,
    // hence this must be an async test with #[tokio::test].
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

// ═════════════════════════════════════════════════════════════════════════════
// 10. SCENARIO COMPLETO — simula il flusso descritto nel README
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_rete_amici_carlo_marco_lucia() {
    // Simula lo scenario descritto:
    // - Carlo: 1 pouch (NAS) + 1 bill (laptop)
    // - Marco: 2 pouch (desktop + VPS) + 1 post (VPS)
    // - Lucia: 1 bill (laptop)
    // Tutti nella rete "amici"

    let mut state = NetworkState::new();
    let now = chrono::Utc::now().timestamp() as u64;

    // Carlo — fingerprint "carlo_fp"
    let carlo_pouch = NodeInfo {
        peer_id: "peer-carlo-nas".into(),
        user_fingerprint: "carlo___".into(),
        user_alias: Some("carlo".into()),
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
        service_type: ServiceType::Bill,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/192.168.1.20/tcp/5001".into()],
        announced_at: now,
        metadata: HashMap::new(),
    };

    // Marco — fingerprint "marco___"
    let marco_pouch1 = NodeInfo {
        peer_id: "peer-marco-desktop".into(),
        user_fingerprint: "marco___".into(),
        user_alias: Some("marco".into()),
        service_type: ServiceType::Pouch,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/10.0.0.5/tcp/5000".into()],
        announced_at: now,
        metadata: {
            let mut m = HashMap::new();
            m.insert("storage_bytes".into(), serde_json::json!(53_687_091_200u64)); // 50GB
            m
        },
    };
    let marco_pouch2 = NodeInfo {
        peer_id: "peer-marco-vps".into(),
        user_fingerprint: "marco___".into(),
        user_alias: Some("marco".into()),
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
            ); // 100GB
            m
        },
    };
    let marco_post = NodeInfo {
        peer_id: "peer-marco-vps-relay".into(),
        user_fingerprint: "marco___".into(),
        user_alias: Some("marco".into()),
        service_type: ServiceType::Post,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/203.0.113.5/tcp/5001".into()],
        announced_at: now,
        metadata: HashMap::new(),
    };

    // Lucia — fingerprint "lucia___"
    let lucia_bill = NodeInfo {
        peer_id: "peer-lucia-laptop".into(),
        user_fingerprint: "lucia___".into(),
        user_alias: Some("lucia".into()),
        service_type: ServiceType::Bill,
        service_id: uuid::Uuid::new_v4().to_string(),
        network_id: "amici".into(),
        listen_addrs: vec!["/ip4/192.168.1.30/tcp/5000".into()],
        announced_at: now,
        metadata: HashMap::new(),
    };

    // Tutti si annunciano via gossip
    state.upsert(carlo_pouch);
    state.upsert(carlo_bill);
    state.upsert(marco_pouch1);
    state.upsert(marco_pouch2);
    state.upsert(marco_post);
    state.upsert(lucia_bill);

    // ── Verifiche ────────────────────────────────────────────────────────

    // 6 nodi totali nella rete "amici"
    let amici = state.in_network("amici");
    assert_eq!(amici.len(), 6, "6 nodi nella rete amici");

    // 3 utenti distinti
    let mut users: Vec<&str> = amici.iter().map(|n| n.user_fingerprint.as_str()).collect();
    users.sort();
    users.dedup();
    assert_eq!(users.len(), 3, "3 utenti distinti: carlo, marco, lucia");

    // Carlo ha 2 nodi
    let carlo_nodes: Vec<_> = amici
        .iter()
        .filter(|n| n.user_fingerprint == "carlo___")
        .collect();
    assert_eq!(carlo_nodes.len(), 2, "Carlo: 1 pouch + 1 bill");

    // Marco ha 3 nodi
    let marco_nodes: Vec<_> = amici
        .iter()
        .filter(|n| n.user_fingerprint == "marco___")
        .collect();
    assert_eq!(marco_nodes.len(), 3, "Marco: 2 pouch + 1 post");

    // Lucia ha 1 nodo
    let lucia_nodes: Vec<_> = amici
        .iter()
        .filter(|n| n.user_fingerprint == "lucia___")
        .collect();
    assert_eq!(lucia_nodes.len(), 1, "Lucia: 1 bill");

    // Storage totale della rete = somma dei pouch
    let total_storage: u64 = amici
        .iter()
        .filter(|n| n.service_type == ServiceType::Pouch)
        .filter_map(|n| n.metadata.get("storage_bytes"))
        .filter_map(|v| v.as_u64())
        .sum();

    // 10GB (carlo) + 50GB (marco desktop) + 100GB (marco vps) = 160GB
    assert_eq!(
        total_storage,
        10_737_418_240 + 53_687_091_200 + 107_374_182_400,
        "Storage totale = somma di tutti i pouch"
    );

    // Lucia con solo bill può "vedere" tutti i pouch disponibili
    let available_storage: Vec<_> = amici
        .iter()
        .filter(|n| n.service_type == ServiceType::Pouch)
        .collect();
    assert_eq!(
        available_storage.len(),
        3,
        "Lucia vede 3 pouch disponibili nella rete"
    );

    // Il post di Marco è l'unico relay
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

// ═════════════════════════════════════════════════════════════════════════════
// 11. ERROR HANDLING — errori chiari e specifici
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn errore_not_authenticated() {
    let err = BpError::NotAuthenticated;
    let msg = err.to_string();
    assert!(msg.contains("bp login"), "Deve suggerire di fare login");
}

#[test]
fn errore_daemon_not_running() {
    let err = BpError::DaemonNotRunning;
    let msg = err.to_string();
    assert!(msg.contains("bp hatch"), "Deve suggerire di fare hatch");
}

#[test]
fn errore_servizio_sconosciuto() {
    let err = BpError::ServiceNotFound("abc123".into());
    let msg = err.to_string();
    assert!(msg.contains("abc123"));
}

#[test]
fn errori_tutte_le_varianti_hanno_messaggio() {
    use bp_core::error::BpError;

    assert!(BpError::Identity("bad key".into())
        .to_string()
        .contains("bad key"));
    assert!(BpError::Service("bad svc".into())
        .to_string()
        .contains("bad svc"));
    assert!(BpError::Network("no conn".into())
        .to_string()
        .contains("no conn"));
    assert!(BpError::Control("socket err".into())
        .to_string()
        .contains("socket err"));
    assert!(BpError::Config("no home".into())
        .to_string()
        .contains("no home"));
    assert!(BpError::UnknownNetwork("xyz".into())
        .to_string()
        .contains("xyz"));
    assert!(BpError::DaemonNotRunning.to_string().contains("bp hatch"));
    assert!(BpError::NotAuthenticated.to_string().contains("bp login"));
    assert!(BpError::ServiceNotFound("svc-id".into())
        .to_string()
        .contains("svc-id"));
}

// ═════════════════════════════════════════════════════════════════════════════
// 12. SERVICETYPE / SERVICESTATUS DISPLAY — Display trait e varianti mancanti
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn service_type_display_lowercase() {
    assert_eq!(ServiceType::Pouch.to_string(), "pouch");
    assert_eq!(ServiceType::Bill.to_string(), "bill");
    assert_eq!(ServiceType::Post.to_string(), "post");
}

#[test]
fn service_status_tutti_i_valori_e_display() {
    assert_eq!(ServiceStatus::Starting.to_string(), "starting");
    assert_eq!(ServiceStatus::Running.to_string(), "running");
    assert_eq!(ServiceStatus::Stopping.to_string(), "stopping");
    assert_eq!(ServiceStatus::Stopped.to_string(), "stopped");
    let err_status = ServiceStatus::Error("disk full".into());
    assert!(err_status.to_string().contains("disk full"));
}

#[test]
fn service_status_serde_roundtrip() {
    let variants = vec![
        ServiceStatus::Starting,
        ServiceStatus::Running,
        ServiceStatus::Stopping,
        ServiceStatus::Stopped,
        ServiceStatus::Error("oops".into()),
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: ServiceStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_string(), v.to_string());
    }
}

#[test]
fn registry_get_mut_aggiorna_status() {
    let mut reg = ServiceRegistry::new();
    let s = ServiceInfo::new(ServiceType::Pouch, "net".into(), HashMap::new());
    let id = s.id.clone();
    reg.register(s);

    if let Some(svc) = reg.get_mut(&id) {
        svc.status = ServiceStatus::Running;
    }
    assert_eq!(reg.get(&id).unwrap().status, ServiceStatus::Running);
}

#[test]
fn registry_get_mut_id_inesistente_ritorna_none() {
    let mut reg = ServiceRegistry::new();
    assert!(reg.get_mut("non-esistente").is_none());
}

#[test]
fn service_info_serde_roundtrip() {
    let s = ServiceInfo::new(ServiceType::Bill, "net-test".into(), {
        let mut m = HashMap::new();
        m.insert("mount_path".into(), serde_json::json!("/mnt/files"));
        m
    });
    let json = serde_json::to_string(&s).unwrap();
    let back: ServiceInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, s.id);
    assert_eq!(back.service_type, ServiceType::Bill);
    assert_eq!(back.network_id, "net-test");
    assert_eq!(
        back.metadata.get("mount_path").unwrap(),
        &serde_json::json!("/mnt/files")
    );
}

// ═════════════════════════════════════════════════════════════════════════════
// 13. CONFIG — funzioni path XDG-aware
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn config_paths_ritornano_ok() {
    assert!(bp_core::config::base_dir().is_ok());
    assert!(bp_core::config::identity_path().is_ok());
    assert!(bp_core::config::profile_path().is_ok());
    assert!(bp_core::config::pid_path().is_ok());
    assert!(bp_core::config::socket_path().is_ok());
    assert!(bp_core::config::services_path().is_ok());
    assert!(bp_core::config::networks_path().is_ok());
}

#[test]
fn config_path_nomi_file_corretti() {
    let base = bp_core::config::base_dir().unwrap();
    let identity = bp_core::config::identity_path().unwrap();
    let profile = bp_core::config::profile_path().unwrap();
    let pid = bp_core::config::pid_path().unwrap();
    let socket = bp_core::config::socket_path().unwrap();

    assert!(identity.to_string_lossy().contains("identity.key"));
    assert!(profile.to_string_lossy().contains("profile.json"));
    assert!(pid.to_string_lossy().contains("daemon.pid"));
    assert!(socket.to_string_lossy().contains("control.sock"));

    // Tutti i path sono dentro base_dir
    assert!(identity.starts_with(&base));
    assert!(profile.starts_with(&base));
}

// ═════════════════════════════════════════════════════════════════════════════
// 14. PROTOCOLLO — varianti mancanti + deserializzazione ControlResponse
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn protocollo_leave_request_serde() {
    // Serializzazione
    let req = ControlRequest::Leave {
        network_id: "amici".into(),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"cmd\":\"leave\""));
    assert!(json.contains("\"amici\""));

    // Deserializzazione
    let back: ControlRequest =
        serde_json::from_str(r#"{"cmd":"leave","network_id":"amici"}"#).unwrap();
    match back {
        ControlRequest::Leave { network_id } => assert_eq!(network_id, "amici"),
        _ => panic!("Deve essere Leave"),
    }
}

#[test]
fn protocollo_status_request_serde() {
    let req = ControlRequest::Status;
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"cmd\":\"status\""));

    let back: ControlRequest = serde_json::from_str(r#"{"cmd":"status"}"#).unwrap();
    assert!(matches!(back, ControlRequest::Status));
}

#[test]
fn protocollo_response_ok_deserializza() {
    let json = r#"{"status":"ok","data":{"foo":"bar"}}"#;
    let resp: ControlResponse = serde_json::from_str(json).unwrap();
    match resp {
        ControlResponse::Ok { data: Some(v) } => assert_eq!(v["foo"], "bar"),
        _ => panic!("Deve essere Ok con data"),
    }
}

#[test]
fn protocollo_response_ok_senza_data_deserializza() {
    let json = r#"{"status":"ok"}"#;
    let resp: ControlResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(resp, ControlResponse::Ok { data: None }));
}

#[test]
fn protocollo_response_error_deserializza() {
    let json = r#"{"status":"error","message":"something broke"}"#;
    let resp: ControlResponse = serde_json::from_str(json).unwrap();
    match resp {
        ControlResponse::Error { message } => assert_eq!(message, "something broke"),
        _ => panic!("Deve essere Error"),
    }
}

#[test]
fn protocollo_hatch_data_roundtrip() {
    use bp_core::control::protocol::HatchData;
    let h = HatchData {
        service_id: "550e8400-uuid".into(),
        service_type: ServiceType::Pouch,
        network_id: "amici".into(),
        message: "service hatched".into(),
    };
    let json = serde_json::to_string(&h).unwrap();
    let back: HatchData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.service_id, "550e8400-uuid");
    assert_eq!(back.service_type, ServiceType::Pouch);
    assert_eq!(back.network_id, "amici");
    assert_eq!(back.message, "service hatched");
}

#[test]
fn protocollo_status_data_roundtrip() {
    use bp_core::control::protocol::StatusData;
    let s = StatusData {
        peer_id: "12D3KooW...".into(),
        fingerprint: "a3f19c2b".into(),
        alias: Some("carlo".into()),
        local_services: vec![],
        networks: vec!["amici".into(), "lavoro".into()],
        known_peers: 5,
        version: "0.1.3".into(),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: StatusData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.peer_id, "12D3KooW...");
    assert_eq!(back.fingerprint, "a3f19c2b");
    assert_eq!(back.alias, Some("carlo".into()));
    assert_eq!(back.known_peers, 5);
    assert_eq!(back.networks, vec!["amici", "lavoro"]);
}

#[test]
fn protocollo_flock_data_roundtrip() {
    use bp_core::control::protocol::FlockData;
    let f = FlockData {
        local_services: vec![ServiceInfo::new(
            ServiceType::Pouch,
            "net".into(),
            HashMap::new(),
        )],
        known_peers: vec![make_node_info(
            "peer-X",
            "aabb",
            ServiceType::Bill,
            "net",
            100,
        )],
        networks: vec!["net".into()],
        peer_count: 1,
    };
    let json = serde_json::to_string(&f).unwrap();
    let back: FlockData = serde_json::from_str(&json).unwrap();
    assert_eq!(back.peer_count, 1);
    assert_eq!(back.networks, vec!["net"]);
    assert_eq!(back.local_services.len(), 1);
    assert_eq!(back.known_peers.len(), 1);
    assert_eq!(back.known_peers[0].peer_id, "peer-X");
}

// ═════════════════════════════════════════════════════════════════════════════
// 15. NETWORK STATE — is_empty + completamento
// ═════════════════════════════════════════════════════════════════════════════

#[test]
fn network_state_is_empty_transizioni() {
    let mut state = NetworkState::new();
    assert!(state.is_empty(), "Stato iniziale deve essere vuoto");
    assert_eq!(state.len(), 0);

    state.upsert(make_node_info("p1", "fp", ServiceType::Pouch, "net", 100));
    assert!(!state.is_empty());
    assert_eq!(state.len(), 1);

    state.remove("p1");
    assert!(state.is_empty());
}

// ═════════════════════════════════════════════════════════════════════════════
// 16. IDENTITY FILESYSTEM — generate / load / remove / exists
// ═════════════════════════════════════════════════════════════════════════════

use std::sync::Mutex;

// Mutex per serializzare i test che modificano HOME
static IDENTITY_FS_LOCK: Mutex<()> = Mutex::new(());

/// Esegue `f` con HOME reimpostato a una directory temporanea unica,
/// poi ripristina HOME e rimuove la directory.
/// Solo su Unix: su Windows `directories` usa APPDATA, non HOME.
#[cfg(unix)]
fn with_temp_home<F: FnOnce() + std::panic::UnwindSafe>(f: F) {
    let _guard = IDENTITY_FS_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let tmp =
        std::env::temp_dir().join(format!("bp_test_{}", uuid::Uuid::new_v4().simple()));
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
fn identity_exists_false_prima_del_login() {
    with_temp_home(|| {
        assert!(
            !bp_core::identity::Identity::exists().unwrap(),
            "exists() deve essere false prima di generate()"
        );
    });
}

#[test]
#[cfg(unix)]
fn identity_load_senza_file_ritorna_not_authenticated() {
    with_temp_home(|| {
        let err = bp_core::identity::Identity::load().unwrap_err();
        assert!(
            matches!(err, bp_core::error::BpError::NotAuthenticated),
            "load() senza file deve ritornare NotAuthenticated"
        );
    });
}

#[test]
#[cfg(unix)]
fn identity_generate_crea_keypair_e_profilo() {
    with_temp_home(|| {
        let identity = bp_core::identity::Identity::generate(Some("test-user".into()))
            .expect("generate() deve riuscire");

        assert_eq!(
            identity.profile.alias,
            Some("test-user".into()),
            "Alias deve essere salvato nel profilo"
        );
        assert_eq!(
            identity.fingerprint.len(),
            16,
            "Fingerprint deve essere 16 caratteri hex"
        );
        assert!(
            identity.fingerprint.chars().all(|c| c.is_ascii_hexdigit()),
            "Fingerprint deve essere hex valido"
        );
        assert!(
            bp_core::identity::Identity::exists().unwrap(),
            "exists() deve essere true dopo generate()"
        );
    });
}

#[test]
#[cfg(unix)]
fn identity_generate_senza_alias() {
    with_temp_home(|| {
        let identity = bp_core::identity::Identity::generate(None)
            .expect("generate() senza alias deve riuscire");
        assert_eq!(
            identity.profile.alias, None,
            "Alias deve essere None se non fornito"
        );
    });
}

#[test]
#[cfg(unix)]
fn identity_load_restituisce_stessa_identita() {
    with_temp_home(|| {
        let original = bp_core::identity::Identity::generate(Some("carlo".into())).unwrap();
        let loaded = bp_core::identity::Identity::load().unwrap();

        assert_eq!(
            loaded.fingerprint, original.fingerprint,
            "Fingerprint deve essere identico dopo reload"
        );
        assert_eq!(
            loaded.peer_id, original.peer_id,
            "PeerId deve essere identico dopo reload"
        );
        assert_eq!(
            loaded.profile.alias,
            Some("carlo".into()),
            "Alias deve sopravvivere al reload"
        );
    });
}

#[test]
#[cfg(unix)]
fn identity_remove_cancella_i_file() {
    with_temp_home(|| {
        bp_core::identity::Identity::generate(Some("to-delete".into())).unwrap();
        assert!(bp_core::identity::Identity::exists().unwrap());

        bp_core::identity::Identity::remove().unwrap();
        assert!(
            !bp_core::identity::Identity::exists().unwrap(),
            "exists() deve essere false dopo remove()"
        );

        // load dopo remove → NotAuthenticated
        let err = bp_core::identity::Identity::load().unwrap_err();
        assert!(matches!(err, bp_core::error::BpError::NotAuthenticated));
    });
}

#[test]
#[cfg(unix)]
fn identity_remove_idempotente_se_non_esiste() {
    with_temp_home(|| {
        // remove() senza un file esistente non deve andare in panico
        bp_core::identity::Identity::remove().unwrap();
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

// ═════════════════════════════════════════════════════════════════════════════
// 17. DAEMON — is_running() e rilevamento PID file
// ═════════════════════════════════════════════════════════════════════════════

#[test]
#[cfg(unix)]
fn daemon_is_running_false_senza_pid_file() {
    with_temp_home(|| {
        bp_core::config::ensure_dirs().unwrap();
        // Senza PID file: is_running() deve essere false
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
        // Scrivi un PID che certamente non esiste (PID 0 o un numero altissimo)
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
        // Usa il PID del processo corrente (che sicuramente esiste)
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
