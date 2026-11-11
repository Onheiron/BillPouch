mod http;
mod media;
mod ssdp;
mod xml;

use futures::future::BoxFuture;
use std::sync::Arc;

pub use media::Kind;

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub size: u64,
}

pub trait Library: Send + Sync + 'static {
    fn items(&self) -> BoxFuture<'_, Vec<Item>>;

    fn read(
        &self,
        id: String,
        from: u64,
        len: u64,
        out: tokio::io::DuplexStream,
    ) -> BoxFuture<'static, Result<(), String>>;
}

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub uuid: String,
    pub port: u16,
}

pub struct Server {
    task: tokio::task::JoinHandle<()>,
    pub port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start(config: Config, library: Arc<dyn Library>) -> std::io::Result<Server> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await?;
    let port = listener.local_addr()?.port();
    let ssdp = ssdp::Ssdp::bind(config.uuid.clone(), port)?;
    let app = http::router(Arc::new(config), library);
    let task = tokio::spawn(async move {
        let http = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        );
        tokio::select! {
            result = http => if let Err(e) = result {
                tracing::warn!("DLNA HTTP server stopped: {e}");
            },
            _ = ssdp.run() => {}
        }
    });
    Ok(Server { task, port })
}
