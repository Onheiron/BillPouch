use bp_dlna::{Config, Item, Library};
use futures::future::BoxFuture;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Memory(Vec<(Item, Vec<u8>)>);

impl Library for Memory {
    fn items(&self) -> BoxFuture<'_, Vec<Item>> {
        Box::pin(async move { self.0.iter().map(|(item, _)| item.clone()).collect() })
    }

    fn read(
        &self,
        id: String,
        from: u64,
        len: u64,
        mut out: tokio::io::DuplexStream,
    ) -> BoxFuture<'static, Result<(), String>> {
        let data = self
            .0
            .iter()
            .find(|(item, _)| item.id == id)
            .map(|(_, data)| data[from as usize..(from + len) as usize].to_vec());
        Box::pin(async move {
            out.write_all(&data.ok_or("missing")?)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

async fn http(port: u16, request: String) -> (String, Vec<u8>) {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (
        String::from_utf8_lossy(&response[..split]).to_lowercase(),
        response[split + 4..].to_vec(),
    )
}

#[tokio::test]
async fn a_tv_finds_the_video_and_streams_part_of_it() {
    let film: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
    let library = Memory(vec![
        (
            Item {
                id: "f1".into(),
                name: "Film.mp4".into(),
                size: film.len() as u64,
            },
            film.clone(),
        ),
        (
            Item {
                id: "f2".into(),
                name: "taxes.pdf".into(),
                size: 3,
            },
            vec![1, 2, 3],
        ),
    ]);
    let server = bp_dlna::start(
        Config {
            name: "Bill".into(),
            uuid: "00000000-0000-0000-0000-000000000001".into(),
            port: 0,
        },
        Arc::new(library),
    )
    .await
    .unwrap();
    let port = server.port;

    let (head, body) = http(
        port,
        "GET /description.xml HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(head.starts_with("http/1.1 200"));
    assert!(String::from_utf8_lossy(&body).contains("urn:schemas-upnp-org:device:MediaServer:1"));

    let soap = "<s:Envelope><s:Body><u:Browse><ObjectID>video</ObjectID>\
                <BrowseFlag>BrowseDirectChildren</BrowseFlag><StartingIndex>0</StartingIndex>\
                <RequestedCount>0</RequestedCount></u:Browse></s:Body></s:Envelope>";
    let (head, body) = http(
        port,
        format!(
            "POST /control/ContentDirectory HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
             SOAPACTION: \"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{soap}",
            soap.len()
        ),
    )
    .await;
    let body = String::from_utf8_lossy(&body);
    assert!(head.starts_with("http/1.1 200"));
    assert!(body.contains("<TotalMatches>1</TotalMatches>"));
    assert!(body.contains(&format!("http://127.0.0.1:{port}/media/f1")));
    assert!(!body.contains("taxes"));

    let (head, body) = http(
        port,
        "GET /media/f1 HTTP/1.1\r\nHost: x\r\nRange: bytes=1000-1999\r\nConnection: close\r\n\r\n"
            .into(),
    )
    .await;
    assert!(head.starts_with("http/1.1 206"));
    assert!(head.contains("content-range: bytes 1000-1999/10000"));
    assert!(head.contains("content-type: video/mp4"));
    assert_eq!(body, film[1000..2000]);

    let (head, _) = http(
        port,
        "GET /media/f2 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(head.starts_with("http/1.1 404"));
}
