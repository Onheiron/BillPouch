use crate::{
    media::{self, Kind},
    xml::{self, Object, CONNECTION_MANAGER, CONTENT_DIRECTORY},
    Config, Item, Library,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Router,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

#[derive(Clone)]
struct Shared {
    config: Arc<Config>,
    library: Arc<dyn Library>,
}

pub fn router(config: Arc<Config>, library: Arc<dyn Library>) -> Router {
    Router::new()
        .route("/description.xml", get(description))
        .route("/ContentDirectory.xml", get(content_directory))
        .route("/ConnectionManager.xml", get(connection_manager))
        .route("/control/{service}", post(control))
        .route("/event/{service}", any(event))
        .route("/media/{id}", get(media))
        .layer(axum::middleware::from_fn(local_only))
        .with_state(Shared { config, library })
}

fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_local(IpAddr::V4(v4)))
        }
    }
}

async fn local_only(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if is_local(peer.ip()) {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn xml_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/xml; charset=\"utf-8\"")],
        body,
    )
        .into_response()
}

async fn description(State(shared): State<Shared>) -> Response {
    xml_response(StatusCode::OK, xml::description(&shared.config))
}

async fn content_directory() -> Response {
    xml_response(StatusCode::OK, xml::content_directory_scpd())
}

async fn connection_manager() -> Response {
    xml_response(StatusCode::OK, xml::connection_manager_scpd())
}

async fn event(method: Method, headers: HeaderMap) -> Response {
    match method.as_str() {
        "SUBSCRIBE" => {
            let sid = headers
                .get("sid")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
                .unwrap_or_else(|| format!("uuid:{}", uuid::Uuid::new_v4()));
            (
                StatusCode::OK,
                [("SID", sid), ("TIMEOUT", "Second-1800".to_string())],
            )
                .into_response()
        }
        "UNSUBSCRIBE" => StatusCode::OK.into_response(),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn library_media(items: &[Item]) -> Vec<(Kind, &'static str, &Item)> {
    let mut media: Vec<_> = items
        .iter()
        .filter_map(|item| media::kind(&item.name).map(|(mime, kind)| (kind, mime, item)))
        .collect();
    media.sort_by_key(|(_, _, item)| item.name.to_lowercase());
    media
}

fn update_id(items: &[Item]) -> u32 {
    items
        .iter()
        .flat_map(|item| item.id.bytes())
        .fold(2_166_136_261u32, |h, b| {
            (h ^ b as u32).wrapping_mul(16_777_619)
        })
}

fn browse(
    config: &Config,
    items: &[Item],
    body: &str,
    base: &str,
) -> Result<String, (u16, &'static str)> {
    let object = xml::arg(body, "ObjectID").unwrap_or_else(|| "0".into());
    let children = match xml::arg(body, "BrowseFlag").as_deref() {
        Some("BrowseMetadata") => false,
        Some("BrowseDirectChildren") => true,
        _ => return Err((402, "Invalid Args")),
    };
    let number = |name| {
        xml::arg(body, name)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
    };
    let (start, count) = (number("StartingIndex"), number("RequestedCount"));
    let media = library_media(items);
    let of_kind = |kind: Kind| media.iter().filter(move |(k, _, _)| *k == kind);
    let folders: Vec<(Kind, usize)> = Kind::ALL
        .into_iter()
        .map(|kind| (kind, of_kind(kind).count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    let objects: Vec<Object> = match (object.as_str(), Kind::from_container(&object)) {
        ("0", _) if children => folders
            .iter()
            .map(|(k, n)| Object::Folder(*k, *n))
            .collect(),
        ("0", _) => vec![Object::Root(&config.name, folders.len())],
        (_, Some(kind)) if children => of_kind(kind)
            .map(|(k, mime, item)| Object::File(*k, mime, item))
            .collect(),
        (_, Some(kind)) => vec![Object::Folder(kind, of_kind(kind).count())],
        (id, None) => match media.iter().find(|(_, _, item)| item.id == id) {
            Some(_) if children => return Err((710, "No such container")),
            Some((k, mime, item)) => vec![Object::File(*k, mime, item)],
            None => return Err((701, "No such object")),
        },
    };
    let total = objects.len();
    let page: Vec<Object> = objects
        .into_iter()
        .skip(start)
        .take(if count == 0 { usize::MAX } else { count })
        .collect();
    Ok(xml::soap(
        CONTENT_DIRECTORY,
        "Browse",
        &[
            ("Result", xml::didl(&page, base)),
            ("NumberReturned", page.len().to_string()),
            ("TotalMatches", total.to_string()),
            ("UpdateID", update_id(items).to_string()),
        ],
    ))
}

async fn control(
    State(shared): State<Shared>,
    Path(service): Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let action = headers
        .get("soapaction")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_matches('"').rsplit_once('#'))
        .map(|(_, action)| action.to_string())
        .unwrap_or_default();
    let base = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|host| format!("http://{host}"))
        .unwrap_or_default();
    let reply = match (service.as_str(), action.as_str()) {
        ("ContentDirectory", "Browse") => {
            let items = shared.library.items().await;
            browse(&shared.config, &items, &body, &base)
        }
        ("ContentDirectory", "GetSystemUpdateID") => {
            let items = shared.library.items().await;
            Ok(xml::soap(
                CONTENT_DIRECTORY,
                &action,
                &[("Id", update_id(&items).to_string())],
            ))
        }
        ("ContentDirectory", "GetSearchCapabilities") => Ok(xml::soap(
            CONTENT_DIRECTORY,
            &action,
            &[("SearchCaps", String::new())],
        )),
        ("ContentDirectory", "GetSortCapabilities") => Ok(xml::soap(
            CONTENT_DIRECTORY,
            &action,
            &[("SortCaps", String::new())],
        )),
        ("ConnectionManager", "GetProtocolInfo") => Ok(xml::soap(
            CONNECTION_MANAGER,
            &action,
            &[("Source", xml::protocol_info()), ("Sink", String::new())],
        )),
        ("ConnectionManager", "GetCurrentConnectionIDs") => Ok(xml::soap(
            CONNECTION_MANAGER,
            &action,
            &[("ConnectionIDs", "0".into())],
        )),
        ("ConnectionManager", "GetCurrentConnectionInfo") => Ok(xml::soap(
            CONNECTION_MANAGER,
            &action,
            &[
                ("RcsID", "-1".into()),
                ("AVTransportID", "-1".into()),
                ("ProtocolInfo", String::new()),
                ("PeerConnectionManager", String::new()),
                ("PeerConnectionID", "-1".into()),
                ("Direction", "Output".into()),
                ("Status", "OK".into()),
            ],
        )),
        _ => Err((401, "Invalid Action")),
    };
    match reply {
        Ok(body) => xml_response(StatusCode::OK, body),
        Err((code, text)) => {
            xml_response(StatusCode::INTERNAL_SERVER_ERROR, xml::fault(code, text))
        }
    }
}

fn parse_range(spec: &str, size: u64) -> Option<(u64, u64)> {
    let spec = spec.trim().strip_prefix("bytes=")?;
    let (from, to) = spec.split(',').next()?.trim().split_once('-')?;
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() {
        let tail = to.parse::<u64>().ok()?.min(size);
        return if tail > 0 {
            Some((size - tail, tail))
        } else {
            None
        };
    }
    let from: u64 = from.parse().ok()?;
    if from >= size {
        return None;
    }
    let to = if to.is_empty() {
        size - 1
    } else {
        to.parse::<u64>().ok()?.min(size - 1)
    };
    if to >= from {
        Some((from, to - from + 1))
    } else {
        None
    }
}

async fn media(
    State(shared): State<Shared>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let items = shared.library.items().await;
    let Some((mime, item)) = items
        .into_iter()
        .find(|item| item.id == id)
        .and_then(|item| media::kind(&item.name).map(|(mime, _)| (mime, item)))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let size = item.size;
    let range = match headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        None => None,
        Some(spec) => match parse_range(spec, size) {
            Some(range) => Some(range),
            None => {
                return (
                    StatusCode::RANGE_NOT_SATISFIABLE,
                    [(header::CONTENT_RANGE, format!("bytes */{size}"))],
                )
                    .into_response()
            }
        },
    };
    let (from, len) = range.unwrap_or((0, size));
    let body = if method == Method::HEAD || len == 0 {
        Body::empty()
    } else {
        let (writer, reader) = tokio::io::duplex(1 << 20);
        let read = shared.library.read(item.id, from, len, writer);
        tokio::spawn(async move {
            if let Err(e) = read.await {
                tracing::debug!("DLNA stream ended early: {e}");
            }
        });
        Body::from_stream(tokio_util::io::ReaderStream::new(reader))
    };
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        "transferMode.dlna.org",
        HeaderValue::from_static(if mime.starts_with("image/") {
            "Interactive"
        } else {
            "Streaming"
        }),
    );
    headers.insert(
        "contentFeatures.dlna.org",
        HeaderValue::from_static(media::DLNA_FEATURES),
    );
    if range.is_some() {
        if let Ok(value) = HeaderValue::from_str(&format!("bytes {from}-{}/{size}", from + len - 1))
        {
            headers.insert(header::CONTENT_RANGE, value);
        }
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_clamped_to_the_file() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 100)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 10)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 10)));
        assert_eq!(parse_range("bytes=-30", 100), Some((70, 30)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=20-10", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }

    #[test]
    fn only_local_clients_are_served() {
        for ip in [
            "192.168.1.20",
            "10.0.0.3",
            "127.0.0.1",
            "169.254.1.1",
            "fe80::1",
            "fd00::2",
        ] {
            assert!(is_local(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "100.64.0.1", "2001:db8::1"] {
            assert!(!is_local(ip.parse().unwrap()), "{ip}");
        }
    }

    fn items() -> Vec<Item> {
        ["b.mkv", "a.mp4", "song.mp3", "notes.txt"]
            .iter()
            .enumerate()
            .map(|(i, name)| Item {
                id: format!("id{i}"),
                name: name.to_string(),
                size: 10,
            })
            .collect()
    }

    fn request(object: &str, flag: &str, start: usize, count: usize) -> String {
        format!(
            "<ObjectID>{object}</ObjectID><BrowseFlag>{flag}</BrowseFlag>\
             <StartingIndex>{start}</StartingIndex><RequestedCount>{count}</RequestedCount>"
        )
    }

    fn config() -> Config {
        Config {
            name: "Bill".into(),
            uuid: "u".into(),
            port: 0,
        }
    }

    #[test]
    fn the_root_holds_one_folder_per_kind_of_media() {
        let reply = browse(
            &config(),
            &items(),
            &request("0", "BrowseDirectChildren", 0, 0),
            "http://h",
        )
        .unwrap();
        assert!(reply.contains("<TotalMatches>2</TotalMatches>"));
        assert!(reply.contains("id=&quot;video&quot;"));
        assert!(reply.contains("id=&quot;audio&quot;"));
        assert!(!reply.contains("id=&quot;image&quot;"));
    }

    #[test]
    fn a_folder_lists_its_media_by_name_a_page_at_a_time() {
        let reply = browse(
            &config(),
            &items(),
            &request("video", "BrowseDirectChildren", 0, 1),
            "http://h",
        )
        .unwrap();
        assert!(reply.contains("<NumberReturned>1</NumberReturned><TotalMatches>2</TotalMatches>"));
        assert!(reply.contains("a.mp4"));
        assert!(!reply.contains("b.mkv"));
        let next = browse(
            &config(),
            &items(),
            &request("video", "BrowseDirectChildren", 1, 1),
            "http://h",
        )
        .unwrap();
        assert!(next.contains("b.mkv"));
    }

    #[test]
    fn unknown_objects_and_files_that_are_not_media_are_refused() {
        assert_eq!(
            browse(
                &config(),
                &items(),
                &request("id3", "BrowseMetadata", 0, 0),
                "http://h"
            )
            .unwrap_err()
            .0,
            701
        );
        assert_eq!(
            browse(
                &config(),
                &items(),
                &request("id0", "BrowseDirectChildren", 0, 0),
                "http://h"
            )
            .unwrap_err()
            .0,
            710
        );
        assert!(browse(
            &config(),
            &items(),
            &request("id0", "BrowseMetadata", 0, 0),
            "http://h"
        )
        .unwrap()
        .contains("http://h/media/id0"));
    }
}
