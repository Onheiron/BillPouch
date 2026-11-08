use base64::Engine as _;
use bp_core::config;
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "bp-api",
    about = "🦤 BillPouch REST API gateway",
    version,
    long_about = None
)]
struct Args {
    #[arg(help = "TCP port to listen on")]
    #[arg(long, default_value_t = 3000)]
    port: u16,

    #[arg(help = "Host address to bind")]
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
}

fn daemon_user_key() -> anyhow::Result<String> {
    let cert: bp_core::identity::DeviceCert =
        serde_json::from_slice(&std::fs::read(config::device_cert_path()?)?)?;
    Ok(cert.user_key)
}

fn load_or_create_tls() -> anyhow::Result<(PathBuf, PathBuf, String)> {
    let dir = config::base_dir()?;
    let (cert_path, key_path) = (dir.join("api_cert.pem"), dir.join("api_key.pem"));
    if !cert_path.exists() || !key_path.exists() {
        std::fs::create_dir_all(&dir)?;
        let generated =
            rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])?;
        std::fs::write(&cert_path, generated.cert.pem())?;
        std::fs::write(&key_path, generated.key_pair.serialize_pem())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let pem = std::fs::read_to_string(&cert_path)?;
    let der: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(der)?;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(&der);
    let fingerprint = digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok((cert_path, key_path, fingerprint))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bp_api=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    let args = Args::parse();
    let user_key = daemon_user_key().map_err(|e| {
        anyhow::anyhow!("cannot read this device's identity ({e}): run `bp login` first")
    })?;
    let (cert_path, key_path, fingerprint) = load_or_create_tls()?;
    let app = bp_api::router(config::socket_path()?, Some(user_key));
    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address: {e}"))?;

    tracing::info!("bp-api listening on https://{addr}");
    tracing::info!("TLS certificate SHA-256 fingerprint: {fingerprint}");
    tracing::info!("Open the dashboard with: bp dashboard --port {}", args.port);
    tracing::info!(
        "Proxying requests to daemon at {:?}",
        config::socket_path()?
    );

    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;
    axum_server::bind_rustls(addr, tls)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
