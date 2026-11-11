use crate::xml::{CONNECTION_MANAGER, CONTENT_DIRECTORY, MEDIA_SERVER};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};
use tokio::net::UdpSocket;

const GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const PORT: u16 = 1900;
const MAX_AGE_SECS: u64 = 1800;
const ALIVE_EVERY: Duration = Duration::from_secs(600);
const SERVER: &str = concat!("Linux UPnP/1.0 BillPouch/", env!("CARGO_PKG_VERSION"));

pub struct Ssdp {
    socket: UdpSocket,
    uuid: String,
    port: u16,
}

struct Farewell(Vec<String>);

impl Drop for Farewell {
    fn drop(&mut self) {
        if let Ok(socket) = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
            for message in &self.0 {
                let _ = socket.send_to(message.as_bytes(), (GROUP, PORT));
            }
        }
    }
}

fn search_target(packet: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(packet).ok()?;
    let mut lines = text.lines();
    if !lines
        .next()?
        .trim()
        .to_ascii_uppercase()
        .starts_with("M-SEARCH")
    {
        return None;
    }
    lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("ST")
            .then(|| value.trim().to_string())
    })
}

fn local_ip(toward: SocketAddr) -> Option<std::net::IpAddr> {
    let probe = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    probe.connect(toward).ok()?;
    Some(probe.local_addr().ok()?.ip())
}

impl Ssdp {
    pub fn bind(uuid: String, port: u16) -> std::io::Result<Self> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT).into())?;
        socket.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?;
        socket.set_multicast_ttl_v4(4)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket: UdpSocket::from_std(socket.into())?,
            uuid,
            port,
        })
    }

    fn targets(&self) -> Vec<(String, String)> {
        let udn = format!("uuid:{}", self.uuid);
        let mut targets = vec![(udn.clone(), udn.clone())];
        for nt in [
            "upnp:rootdevice",
            MEDIA_SERVER,
            CONTENT_DIRECTORY,
            CONNECTION_MANAGER,
        ] {
            targets.push((nt.to_string(), format!("{udn}::{nt}")));
        }
        targets
    }

    fn location(&self, toward: SocketAddr) -> Option<String> {
        local_ip(toward).map(|ip| format!("http://{ip}:{}/description.xml", self.port))
    }

    fn answers(&self, target: &str, location: &str) -> Vec<String> {
        self.targets()
            .into_iter()
            .filter(|(nt, _)| target == "ssdp:all" || target == nt)
            .map(|(st, usn)| {
                format!(
                    "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age={MAX_AGE_SECS}\r\nEXT:\r\n\
                     LOCATION: {location}\r\nSERVER: {SERVER}\r\nST: {st}\r\nUSN: {usn}\r\n\r\n"
                )
            })
            .collect()
    }

    fn notices(&self, alive: Option<&str>) -> Vec<String> {
        self.targets()
            .into_iter()
            .map(|(nt, usn)| match alive {
                Some(location) => format!(
                    "NOTIFY * HTTP/1.1\r\nHOST: {GROUP}:{PORT}\r\nCACHE-CONTROL: max-age={MAX_AGE_SECS}\r\n\
                     LOCATION: {location}\r\nNT: {nt}\r\nNTS: ssdp:alive\r\nSERVER: {SERVER}\r\nUSN: {usn}\r\n\r\n"
                ),
                None => format!(
                    "NOTIFY * HTTP/1.1\r\nHOST: {GROUP}:{PORT}\r\nNT: {nt}\r\nNTS: ssdp:byebye\r\nUSN: {usn}\r\n\r\n"
                ),
            })
            .collect()
    }

    async fn send(&self, messages: Vec<String>, to: SocketAddr) {
        for message in messages {
            if let Err(e) = self.socket.send_to(message.as_bytes(), to).await {
                tracing::debug!("SSDP send to {to} failed: {e}");
            }
        }
    }

    pub async fn run(self) {
        let group = SocketAddr::from((GROUP, PORT));
        let _farewell = Farewell(self.notices(None));
        let mut alive = tokio::time::interval(ALIVE_EVERY);
        let mut packet = [0u8; 2048];
        loop {
            tokio::select! {
                _ = alive.tick() => {
                    if let Some(location) = self.location(group) {
                        self.send(self.notices(Some(&location)), group).await;
                    }
                }
                received = self.socket.recv_from(&mut packet) => match received {
                    Ok((n, from)) => {
                        let target = search_target(&packet[..n]);
                        if let (Some(target), Some(location)) = (target, self.location(from)) {
                            self.send(self.answers(&target, &location), from).await;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("SSDP receive failed: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_are_recognised() {
        let search = b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nst: urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n";
        assert_eq!(search_target(search).as_deref(), Some(MEDIA_SERVER));
        assert_eq!(
            search_target(b"NOTIFY * HTTP/1.1\r\nNT: upnp:rootdevice\r\n\r\n"),
            None
        );
    }

    #[tokio::test]
    async fn a_search_is_answered_for_each_matching_target() {
        let ssdp = Ssdp {
            socket: UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap(),
            uuid: "abc".into(),
            port: 8200,
        };
        let location = "http://10.0.0.2:8200/description.xml";
        assert_eq!(ssdp.answers("ssdp:all", location).len(), 5);
        let root = ssdp.answers("upnp:rootdevice", location);
        assert_eq!(root.len(), 1);
        assert!(root[0].contains("USN: uuid:abc::upnp:rootdevice\r\n"));
        assert!(root[0].contains("LOCATION: http://10.0.0.2:8200/description.xml\r\n"));
        assert!(ssdp.answers("urn:other", location).is_empty());
        assert_eq!(ssdp.notices(None).len(), 5);
    }
}
