use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use bp_core::control::protocol::{
    ControlRequest, ControlResponse, FlockData, GetFileData, HatchData, PutFileData, StatusData,
};
use bp_core::control::server::{run_control_server, DaemonState};
use bp_core::identity::Identity;
use bp_core::network::state::NetworkState;
use bp_core::network::{NetworkCommand, QosRegistry, ReputationStore};
use bp_core::service::ServiceRegistry;

fn memberships_for(
    identity: &bp_core::identity::Identity,
    names: &[&str],
) -> bp_core::membership::Memberships {
    let registries = names
        .iter()
        .map(|n| {
            let (registry, _) = bp_core::membership::Registry::create(identity, n).unwrap();
            (n.to_string(), registry)
        })
        .collect();
    std::sync::Arc::new(std::sync::RwLock::new(registries))
}

fn make_daemon_state() -> (Arc<DaemonState>, mpsc::Receiver<NetworkCommand>) {
    let identity = Identity::in_memory(Some("test-pelican".to_string())).unwrap();

    let (net_tx, net_rx) = mpsc::channel::<NetworkCommand>(64);

    let state = Arc::new(DaemonState {
        identity: identity.clone(),
        services: RwLock::new(ServiceRegistry::new()),
        network_state: Arc::new(RwLock::new(NetworkState::new())),
        networks: RwLock::new(Vec::new()),
        memberships: memberships_for(
            &identity,
            &[
                "amici",
                "lavoro",
                "test-net",
                "block-net",
                "dup-net",
                "evict-net",
            ],
        ),
        net_tx,
        storage_managers: Arc::new(RwLock::new(std::collections::HashMap::new())),
        qos: Arc::new(RwLock::new(QosRegistry::new())),
        outgoing_assignments: Arc::new(RwLock::new(std::collections::HashMap::new())),
        remote_fragment_index: Arc::new(RwLock::new(bp_core::network::RemoteFragmentIndex::new())),
        chunk_cek_hints: RwLock::new(std::collections::HashMap::new()),
        reputation: Arc::new(RwLock::new(ReputationStore::new())),
        file_registry: RwLock::new(bp_core::storage::FileRegistry::default()),
    });

    (state, net_rx)
}

fn temp_socket_path() -> PathBuf {
    let id = uuid::Uuid::new_v4();
    PathBuf::from(format!("/tmp/bp-test-{}.sock", id))
}

async fn send_request(socket_path: &std::path::Path, request: &ControlRequest) -> ControlResponse {
    let stream = UnixStream::connect(socket_path)
        .await
        .expect("failed to connect to control socket");

    let (read_half, mut write_half) = stream.into_split();

    let mut json = serde_json::to_string(request).expect("failed to serialize request");
    json.push('\n');
    write_half
        .write_all(json.as_bytes())
        .await
        .expect("failed to write request");

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .expect("failed to read response");

    serde_json::from_str(line.trim()).expect("failed to deserialize response")
}

fn unwrap_ok(resp: ControlResponse) -> serde_json::Value {
    match resp {
        ControlResponse::Ok { data } => data.unwrap_or(serde_json::Value::Null),
        ControlResponse::Error { message } => panic!("expected Ok, got Error: {}", message),
    }
}

fn unwrap_err(resp: ControlResponse) -> String {
    match resp {
        ControlResponse::Error { message } => message,
        ControlResponse::Ok { .. } => panic!("expected Error, got Ok"),
    }
}

#[tokio::test]
async fn full_user_journey() {
    let socket_path = temp_socket_path();
    let (state, mut _net_rx) = make_daemon_state();

    let expected_peer_id = state.identity.peer_id.to_string();
    let expected_fingerprint = state.identity.fingerprint.clone();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "amici".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert(
                    "storage_bytes".to_string(),
                    serde_json::json!(10_737_418_240u64),
                );
                m
            },
        },
    )
    .await;
    let hatch_pouch: HatchData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(hatch_pouch.network_id, "amici");
    assert_eq!(
        hatch_pouch.service_type,
        bp_core::service::ServiceType::Pouch
    );
    let pouch_id = hatch_pouch.service_id.clone();

    let resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Bill,
            network_id: "amici".to_string(),
            metadata: HashMap::new(),
        },
    )
    .await;
    let hatch_bill: HatchData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(hatch_bill.network_id, "amici");
    assert_eq!(hatch_bill.service_type, bp_core::service::ServiceType::Bill);
    let _bill_id = hatch_bill.service_id.clone();

    let resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Post,
            network_id: "amici".to_string(),
            metadata: HashMap::new(),
        },
    )
    .await;
    let hatch_post: HatchData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(hatch_post.service_type, bp_core::service::ServiceType::Post);
    let post_id = hatch_post.service_id.clone();

    let resp = send_request(&socket_path, &ControlRequest::Flock).await;
    let flock: FlockData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(flock.local_services.len(), 3, "expected 3 local services");
    assert!(flock.networks.contains(&"amici".to_string()));

    let resp = send_request(&socket_path, &ControlRequest::Status).await;
    let status: StatusData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(status.peer_id, expected_peer_id);
    assert_eq!(status.fingerprint, expected_fingerprint);
    assert_eq!(status.alias, Some("test-pelican".to_string()));
    assert_eq!(status.local_services.len(), 3);
    assert!(status.networks.contains(&"amici".to_string()));

    let resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Bill,
            network_id: "lavoro".to_string(),
            metadata: HashMap::new(),
        },
    )
    .await;
    let hatch_lavoro: HatchData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(hatch_lavoro.network_id, "lavoro");

    let resp = send_request(&socket_path, &ControlRequest::Status).await;
    let status: StatusData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(status.networks.len(), 2);
    assert!(status.networks.contains(&"lavoro".to_string()));

    let resp = send_request(
        &socket_path,
        &ControlRequest::Farewell {
            service_id: post_id.clone(),
        },
    )
    .await;
    let farewell_data = unwrap_ok(resp);
    assert_eq!(farewell_data["service_id"], post_id);

    let resp = send_request(&socket_path, &ControlRequest::Flock).await;
    let flock: FlockData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(
        flock.local_services.len(),
        3,
        "expected 3 services after farewell"
    );
    assert!(
        flock.local_services.iter().any(|s| s.id == pouch_id),
        "pouch service should still be present"
    );

    let resp = send_request(&socket_path, &ControlRequest::Ping).await;
    let pong = unwrap_ok(resp);
    assert_eq!(pong, "pong");

    let resp = send_request(
        &socket_path,
        &ControlRequest::Farewell {
            service_id: "does-not-exist".to_string(),
        },
    )
    .await;
    let err_msg = unwrap_err(resp);
    assert!(
        err_msg.contains("does-not-exist"),
        "error should mention the missing service id"
    );

    let resp = send_request(
        &socket_path,
        &ControlRequest::CreateNetwork {
            name: "amici".to_string(),
        },
    )
    .await;
    let err_msg = unwrap_err(resp);
    assert!(
        err_msg.contains("amici"),
        "error should mention the network name"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn put_file_get_file_roundtrip() {
    let socket_path = temp_socket_path();
    let (state, mut _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "test-net".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
                m
            },
        },
    )
    .await;

    let original: Vec<u8> = b"BillPouch put/get roundtrip test payload".to_vec();
    let put_resp = send_request(
        &socket_path,
        &ControlRequest::PutFile {
            chunk_data: original.clone(),
            ph: Some(0.99),
            q_target: Some(1.0),
            network_id: "test-net".to_string(),
            file_name: None,
        },
    )
    .await;

    let put_data: PutFileData =
        serde_json::from_value(unwrap_ok(put_resp)).expect("PutFile should succeed");

    assert!(!put_data.chunk_id.is_empty(), "chunk_id must not be empty");
    assert!(put_data.k >= 1, "k must be at least 1");
    assert!(
        put_data.fragments_stored >= put_data.k,
        "must store at least k fragments"
    );

    let get_resp = send_request(
        &socket_path,
        &ControlRequest::GetFile {
            chunk_id: put_data.chunk_id.clone(),
            network_id: "test-net".to_string(),
        },
    )
    .await;

    let get_data: GetFileData =
        serde_json::from_value(unwrap_ok(get_resp)).expect("GetFile should succeed");

    assert_eq!(
        get_data.data, original,
        "decoded data must match original bytes"
    );
    assert!(
        get_data.fragments_used >= put_data.k,
        "should have used at least k fragments"
    );
    assert_eq!(get_data.fragments_remote, 0, "no remote peers in this test");

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn put_file_without_pouch_returns_error() {
    let socket_path = temp_socket_path();
    let (state, mut _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let resp = send_request(
        &socket_path,
        &ControlRequest::PutFile {
            chunk_data: b"test".to_vec(),
            ph: None,
            q_target: None,
            network_id: "test-net".to_string(),
            file_name: None,
        },
    )
    .await;

    let err_msg = unwrap_err(resp);
    assert!(
        err_msg.to_lowercase().contains("no active pouch")
            || err_msg.to_lowercase().contains("no storage")
            || err_msg.to_lowercase().contains("no pouches available")
            || err_msg.to_lowercase().contains("pouch"),
        "expected a 'no pouch' error, got: {err_msg}"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn get_file_unknown_chunk_returns_error() {
    let socket_path = temp_socket_path();
    let (state, mut _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "test-net".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
                m
            },
        },
    )
    .await;

    let resp = send_request(
        &socket_path,
        &ControlRequest::GetFile {
            chunk_id: "deadbeef12345678".to_string(),
            network_id: "test-net".to_string(),
        },
    )
    .await;

    let err_msg = unwrap_err(resp);
    assert!(
        err_msg.contains("deadbeef") || err_msg.to_lowercase().contains("not found"),
        "expected not-found error, got: {err_msg}"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn get_file_missing_cek_hint_returns_error() {
    let socket_path = temp_socket_path();
    let (state, mut _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "test-net".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
                m
            },
        },
    )
    .await;

    let put_resp = send_request(
        &socket_path,
        &ControlRequest::PutFile {
            chunk_data: b"CEK hint test payload".to_vec(),
            ph: Some(0.99),
            q_target: Some(1.0),
            network_id: "test-net".to_string(),
            file_name: None,
        },
    )
    .await;
    let put_data: PutFileData =
        serde_json::from_value(unwrap_ok(put_resp)).expect("PutFile must succeed");

    state.chunk_cek_hints.write().unwrap().clear();

    let get_resp = send_request(
        &socket_path,
        &ControlRequest::GetFile {
            chunk_id: put_data.chunk_id.clone(),
            network_id: "test-net".to_string(),
        },
    )
    .await;
    let err_msg = unwrap_err(get_resp);
    assert!(
        err_msg.to_lowercase().contains("cek") || err_msg.to_lowercase().contains("daemon"),
        "error must mention CEK/daemon restart: {err_msg}"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn pause_resume_roundtrip() {
    let socket_path = temp_socket_path();
    let (state, _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let hatch_resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "test-net".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
                m
            },
        },
    )
    .await;
    let hatch_data: HatchData =
        serde_json::from_value(unwrap_ok(hatch_resp)).expect("Hatch must succeed");
    let svc_id = hatch_data.service_id.clone();

    let pause_resp = send_request(
        &socket_path,
        &ControlRequest::Pause {
            service_id: svc_id.clone(),
            eta_minutes: 45,
        },
    )
    .await;
    let pause_data = unwrap_ok(pause_resp);
    assert_eq!(
        pause_data["status"].as_str().unwrap_or(""),
        "paused",
        "pause response must have status=paused"
    );
    assert_eq!(pause_data["eta_minutes"].as_u64().unwrap_or(0), 45);

    let status_resp = send_request(&socket_path, &ControlRequest::Status).await;
    let status_data: StatusData =
        serde_json::from_value(unwrap_ok(status_resp)).expect("Status must succeed");
    let svc = status_data
        .local_services
        .iter()
        .find(|s| s.id == svc_id)
        .expect("service must appear in status");
    assert!(
        svc.status.to_string().contains("paused"),
        "service status must be paused: {}",
        svc.status
    );

    let resume_resp = send_request(
        &socket_path,
        &ControlRequest::Resume {
            service_id: svc_id.clone(),
        },
    )
    .await;
    let resume_data = unwrap_ok(resume_resp);
    assert_eq!(
        resume_data["status"].as_str().unwrap_or(""),
        "running",
        "resume response must have status=running"
    );

    let status_resp2 = send_request(&socket_path, &ControlRequest::Status).await;
    let status_data2: StatusData =
        serde_json::from_value(unwrap_ok(status_resp2)).expect("Status must succeed");
    let svc2 = status_data2
        .local_services
        .iter()
        .find(|s| s.id == svc_id)
        .expect("service must appear in status");
    assert_eq!(
        svc2.status.to_string(),
        "running",
        "service must be running after resume"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn farewell_evict_removes_service() {
    let socket_path = temp_socket_path();
    let (state, _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let hatch_resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "evict-net".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
                m
            },
        },
    )
    .await;
    let hatch_data: HatchData =
        serde_json::from_value(unwrap_ok(hatch_resp)).expect("Hatch must succeed");
    let svc_id = hatch_data.service_id.clone();

    let evict_resp = send_request(
        &socket_path,
        &ControlRequest::FarewellEvict {
            service_id: svc_id.clone(),
        },
    )
    .await;
    let evict_data = unwrap_ok(evict_resp);
    assert_eq!(
        evict_data["service_id"].as_str().unwrap_or(""),
        svc_id,
        "evict response must echo the service_id"
    );

    let flock_resp = send_request(&socket_path, &ControlRequest::Flock).await;
    let flock_data: FlockData =
        serde_json::from_value(unwrap_ok(flock_resp)).expect("Flock must succeed");
    let still_present = flock_data.local_services.iter().any(|s| s.id == svc_id);
    assert!(!still_present, "evicted service must not appear in Flock");

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn leave_blocked_by_active_service() {
    let socket_path = temp_socket_path();
    let (state, _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let hatch_resp = send_request(
        &socket_path,
        &ControlRequest::Hatch {
            service_type: bp_core::service::ServiceType::Pouch,
            network_id: "block-net".to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
                m
            },
        },
    )
    .await;
    let hatch_data: HatchData =
        serde_json::from_value(unwrap_ok(hatch_resp)).expect("Hatch must succeed");

    let leave_resp = send_request(
        &socket_path,
        &ControlRequest::Leave {
            network_id: "block-net".to_string(),
            force: false,
        },
    )
    .await;
    let leave_data = unwrap_ok(leave_resp);
    assert!(
        leave_data["blocked"].as_bool().unwrap_or(false),
        "leave must be blocked when active services remain"
    );
    let blocking = leave_data["blocking_services"]
        .as_array()
        .expect("blocking_services must be an array");
    assert!(
        !blocking.is_empty(),
        "blocking_services must name the active service"
    );
    let ids: Vec<&str> = blocking.iter().filter_map(|v| v["id"].as_str()).collect();
    assert!(
        ids.contains(&hatch_data.service_id.as_str()),
        "active service must appear in blocking_services"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn hatch_second_pouch_same_network_rejected() {
    let socket_path = temp_socket_path();
    let (state, _net_rx) = make_daemon_state();

    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;

    let hatch_pouch = || ControlRequest::Hatch {
        service_type: bp_core::service::ServiceType::Pouch,
        network_id: "dup-net".to_string(),
        metadata: {
            let mut m = HashMap::new();
            m.insert("storage_bytes".to_string(), serde_json::json!(1_048_576u64));
            m
        },
    };

    let first = send_request(&socket_path, &hatch_pouch()).await;
    unwrap_ok(first);

    let second = send_request(&socket_path, &hatch_pouch()).await;
    let err_msg = unwrap_err(second);
    assert!(
        err_msg.to_lowercase().contains("already")
            || err_msg.to_lowercase().contains("pouch")
            || err_msg.to_lowercase().contains("dup-net"),
        "second hatch must be rejected with a meaningful error: {err_msg}"
    );

    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}
