use crate::{
    control::{files, server::DaemonState},
    service::{ServiceInfo, ServiceType},
};
use futures::future::BoxFuture;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub type Plugins = Mutex<HashMap<String, bp_dlna::Server>>;

struct BillLibrary {
    state: Arc<DaemonState>,
    network_id: String,
}

impl bp_dlna::Library for BillLibrary {
    fn items(&self) -> BoxFuture<'_, Vec<bp_dlna::Item>> {
        Box::pin(async move {
            files::list_files(&self.state, &self.network_id)
                .files
                .into_iter()
                .map(|f| bp_dlna::Item {
                    id: f.file_id,
                    name: f.file_name,
                    size: f.size_bytes,
                })
                .collect()
        })
    }

    fn read(
        &self,
        id: String,
        from: u64,
        len: u64,
        mut out: tokio::io::DuplexStream,
    ) -> BoxFuture<'static, Result<(), String>> {
        let (state, network_id) = (Arc::clone(&self.state), self.network_id.clone());
        Box::pin(async move {
            let read = files::FileRead::open(&state, &network_id, &id, Some((from, len)))?;
            read.copy_to(&state, &mut out).await.map(|_| ())
        })
    }
}

pub fn port(service: &ServiceInfo) -> Option<u16> {
    service
        .metadata
        .get("dlna")
        .and_then(|v| v.as_u64())
        .and_then(|p| u16::try_from(p).ok())
}

pub async fn start(state: &Arc<DaemonState>, service: &ServiceInfo) -> Result<(), String> {
    let Some(port) = port(service) else {
        return Ok(());
    };
    if service.service_type != ServiceType::Bill {
        return Err("only a Bill serves media over DLNA".into());
    }
    let config = bp_dlna::Config {
        name: format!(
            "BillPouch {} · {}",
            state.identity.profile.alias.as_deref().unwrap_or("Bill"),
            service.network_id
        ),
        uuid: service.id.clone(),
        port,
    };
    let library = Arc::new(BillLibrary {
        state: Arc::clone(state),
        network_id: service.network_id.clone(),
    });
    let server = bp_dlna::start(config, library)
        .await
        .map_err(|e| format!("cannot serve DLNA on port {port}: {e}"))?;
    tracing::info!(service_id = %service.id, port, "DLNA media server started");
    state
        .plugins
        .lock()
        .unwrap()
        .insert(service.id.clone(), server);
    Ok(())
}

pub fn stop(state: &DaemonState, service_id: &str) {
    if state.plugins.lock().unwrap().remove(service_id).is_some() {
        tracing::info!(service_id, "DLNA media server stopped");
    }
}

pub async fn restore(state: &Arc<DaemonState>) {
    let services: Vec<ServiceInfo> = state
        .services
        .read()
        .unwrap()
        .all()
        .into_iter()
        .filter(|s| port(s).is_some())
        .cloned()
        .collect();
    for service in services {
        if let Err(e) = start(state, &service).await {
            tracing::warn!(service_id = %service.id, "{e}");
        }
    }
}
