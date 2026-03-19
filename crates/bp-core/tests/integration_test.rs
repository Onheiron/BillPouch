//! Integration test: spins up a real control server on a temp Unix socket,
//! connects as a client, sends JSON commands, and verifies responses.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use bp_core::control::protocol::{
    ControlRequest, ControlResponse, FlockData, HatchData, StatusData,
};
use bp_core::control::server::{run_control_server, DaemonState};
use bp_core::identity::{fingerprint, Identity, UserProfile};
use bp_core::network::state::NetworkState;
use bp_core::network::NetworkCommand;
use bp_core::service::ServiceRegistry;

/// Build a `DaemonState` in-memory without touching disk.
fn make_daemon_state() -> (Arc<DaemonState>, mpsc::Receiver<NetworkCommand>) {
    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let peer_id = libp2p::PeerId::from_public_key(&keypair.public());
    let fp = fingerprint(&keypair);

    let identity = Identity {
        keypair,
        peer_id,
        fingerprint: fp.clone(),
        profile: UserProfile {
            fingerprint: fp,
            alias: Some("test-pelican".to_string()),
            created_at: chrono::Utc::now(),
        },
    };

    let (net_tx, net_rx) = mpsc::channel::<NetworkCommand>(64);

    let state = Arc::new(DaemonState {
        identity,
        services: RwLock::new(ServiceRegistry::new()),
        network_state: Arc::new(RwLock::new(NetworkState::new())),
        networks: RwLock::new(Vec::new()),
        net_tx,
        storage_managers: Arc::new(RwLock::new(std::collections::HashMap::new())),
    });

    (state, net_rx)
}

/// Unique socket path for this test run.
fn temp_socket_path() -> PathBuf {
    let id = uuid::Uuid::new_v4();
    PathBuf::from(format!("/tmp/bp-test-{}.sock", id))
}

/// Connect to the control socket, send a `ControlRequest` as JSON + newline,
/// read the response line, and deserialize it as `ControlResponse`.
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

/// Extract the `data` payload from an Ok response, panicking on Error.
fn unwrap_ok(resp: ControlResponse) -> serde_json::Value {
    match resp {
        ControlResponse::Ok { data } => data.unwrap_or(serde_json::Value::Null),
        ControlResponse::Error { message } => panic!("expected Ok, got Error: {}", message),
    }
}

/// Assert the response is an error and return the message.
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

    // Keep a copy of peer_id / fingerprint for assertions later.
    let expected_peer_id = state.identity.peer_id.to_string();
    let expected_fingerprint = state.identity.fingerprint.clone();

    // 1. Start daemon (spawn the control server).
    let server_state = Arc::clone(&state);
    let server_path = socket_path.clone();
    let server_handle = tokio::spawn(async move {
        let _ = run_control_server(&server_path, server_state).await;
    });

    // Give the listener a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // ── 2. Hatch a pouch service on network "amici" with 10GB storage ────────
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

    // ── 3. Hatch a bill service on "amici" ───────────────────────────────────
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

    // ── 4. Hatch a post service on "amici" ───────────────────────────────────
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

    // ── 5. Flock — verify all 3 services are listed ──────────────────────────
    let resp = send_request(&socket_path, &ControlRequest::Flock).await;
    let flock: FlockData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(flock.local_services.len(), 3, "expected 3 local services");
    assert!(flock.networks.contains(&"amici".to_string()));

    // ── 6. Status — verify identity info ─────────────────────────────────────
    let resp = send_request(&socket_path, &ControlRequest::Status).await;
    let status: StatusData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(status.peer_id, expected_peer_id);
    assert_eq!(status.fingerprint, expected_fingerprint);
    assert_eq!(status.alias, Some("test-pelican".to_string()));
    assert_eq!(status.local_services.len(), 3);
    assert!(status.networks.contains(&"amici".to_string()));

    // ── 7. Join a second network "lavoro" ────────────────────────────────────
    let resp = send_request(
        &socket_path,
        &ControlRequest::Join {
            network_id: "lavoro".to_string(),
        },
    )
    .await;
    let join_data = unwrap_ok(resp);
    assert_eq!(join_data["network_id"], "lavoro");

    // Verify networks list now has both.
    let resp = send_request(&socket_path, &ControlRequest::Status).await;
    let status: StatusData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(status.networks.len(), 2);
    assert!(status.networks.contains(&"lavoro".to_string()));

    // ── 8. Farewell (stop) the post service ──────────────────────────────────
    let resp = send_request(
        &socket_path,
        &ControlRequest::Farewell {
            service_id: post_id.clone(),
        },
    )
    .await;
    let farewell_data = unwrap_ok(resp);
    assert_eq!(farewell_data["service_id"], post_id);

    // ── 9. Flock again — only 2 services remain ─────────────────────────────
    let resp = send_request(&socket_path, &ControlRequest::Flock).await;
    let flock: FlockData = serde_json::from_value(unwrap_ok(resp)).unwrap();
    assert_eq!(
        flock.local_services.len(),
        2,
        "expected 2 services after farewell"
    );
    // The pouch should still be there.
    assert!(
        flock.local_services.iter().any(|s| s.id == pouch_id),
        "pouch service should still be present"
    );

    // ── 10. Ping → pong ─────────────────────────────────────────────────────
    let resp = send_request(&socket_path, &ControlRequest::Ping).await;
    let pong = unwrap_ok(resp);
    assert_eq!(pong, "pong");

    // ── 11a. Error case: farewell a non-existent service ─────────────────────
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

    // ── 11b. Error case: join an already-joined network ──────────────────────
    let resp = send_request(
        &socket_path,
        &ControlRequest::Join {
            network_id: "amici".to_string(),
        },
    )
    .await;
    let err_msg = unwrap_err(resp);
    assert!(
        err_msg.contains("amici"),
        "error should mention the network name"
    );

    // ── Cleanup ──────────────────────────────────────────────────────────────
    server_handle.abort();
    let _ = std::fs::remove_file(&socket_path);
}
