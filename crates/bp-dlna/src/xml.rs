use crate::{
    media::{self, Kind},
    Config, Item,
};

pub const CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
pub const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
pub const MEDIA_SERVER: &str = "urn:schemas-upnp-org:device:MediaServer:1";

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

pub fn arg(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let mut rest = body;
    while let Some(at) = rest.find(&open) {
        rest = &rest[at + open.len()..];
        match rest.chars().next() {
            Some('/') if rest.starts_with("/>") => return Some(String::new()),
            Some(c) if c == '>' || c.is_whitespace() => {
                let start = rest.find('>')? + 1;
                if rest[..start].ends_with("/>") {
                    return Some(String::new());
                }
                let end = rest.find(&format!("</{name}>"))?;
                return Some(unescape(rest.get(start..end)?.trim()));
            }
            _ => {}
        }
    }
    None
}

pub fn description(config: &Config) -> String {
    let service = |name: &str| {
        format!(
            "<service><serviceType>urn:schemas-upnp-org:service:{name}:1</serviceType>\
             <serviceId>urn:upnp-org:serviceId:{name}</serviceId>\
             <SCPDURL>/{name}.xml</SCPDURL>\
             <controlURL>/control/{name}</controlURL>\
             <eventSubURL>/event/{name}</eventSubURL></service>"
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <root xmlns=\"urn:schemas-upnp-org:device-1-0\" xmlns:dlna=\"urn:schemas-dlna-org:device-1-0\">\
         <specVersion><major>1</major><minor>0</minor></specVersion>\
         <device><deviceType>{MEDIA_SERVER}</deviceType>\
         <friendlyName>{}</friendlyName>\
         <manufacturer>BillPouch</manufacturer>\
         <modelName>BillPouch</modelName>\
         <modelNumber>{}</modelNumber>\
         <UDN>uuid:{}</UDN>\
         <dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC>\
         <serviceList>{}{}</serviceList></device></root>",
        escape(&config.name),
        env!("CARGO_PKG_VERSION"),
        config.uuid,
        service("ContentDirectory"),
        service("ConnectionManager"),
    )
}

type Action<'a> = (&'a str, &'a [(&'a str, bool, &'a str)]);
type Variable<'a> = (&'a str, &'a str, bool, &'a [&'a str]);

fn scpd(actions: &[Action], variables: &[Variable]) -> String {
    let actions: String = actions
        .iter()
        .map(|(name, args)| {
            let args: String = args
                .iter()
                .map(|(arg, input, variable)| {
                    format!(
                        "<argument><name>{arg}</name><direction>{}</direction>\
                         <relatedStateVariable>{variable}</relatedStateVariable></argument>",
                        if *input { "in" } else { "out" }
                    )
                })
                .collect();
            format!("<action><name>{name}</name><argumentList>{args}</argumentList></action>")
        })
        .collect();
    let variables: String = variables
        .iter()
        .map(|(name, kind, events, allowed)| {
            let allowed = if allowed.is_empty() {
                String::new()
            } else {
                let values: String = allowed
                    .iter()
                    .map(|v| format!("<allowedValue>{v}</allowedValue>"))
                    .collect();
                format!("<allowedValueList>{values}</allowedValueList>")
            };
            format!(
                "<stateVariable sendEvents=\"{}\"><name>{name}</name><dataType>{kind}</dataType>{allowed}</stateVariable>",
                if *events { "yes" } else { "no" }
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <scpd xmlns=\"urn:schemas-upnp-org:service-1-0\">\
         <specVersion><major>1</major><minor>0</minor></specVersion>\
         <actionList>{actions}</actionList>\
         <serviceStateTable>{variables}</serviceStateTable></scpd>"
    )
}

pub fn content_directory_scpd() -> String {
    scpd(
        &[
            (
                "Browse",
                &[
                    ("ObjectID", true, "A_ARG_TYPE_ObjectID"),
                    ("BrowseFlag", true, "A_ARG_TYPE_BrowseFlag"),
                    ("Filter", true, "A_ARG_TYPE_Filter"),
                    ("StartingIndex", true, "A_ARG_TYPE_Index"),
                    ("RequestedCount", true, "A_ARG_TYPE_Count"),
                    ("SortCriteria", true, "A_ARG_TYPE_SortCriteria"),
                    ("Result", false, "A_ARG_TYPE_Result"),
                    ("NumberReturned", false, "A_ARG_TYPE_Count"),
                    ("TotalMatches", false, "A_ARG_TYPE_Count"),
                    ("UpdateID", false, "A_ARG_TYPE_UpdateID"),
                ],
            ),
            (
                "GetSearchCapabilities",
                &[("SearchCaps", false, "SearchCapabilities")],
            ),
            (
                "GetSortCapabilities",
                &[("SortCaps", false, "SortCapabilities")],
            ),
            ("GetSystemUpdateID", &[("Id", false, "SystemUpdateID")]),
        ],
        &[
            ("A_ARG_TYPE_ObjectID", "string", false, &[]),
            ("A_ARG_TYPE_Result", "string", false, &[]),
            (
                "A_ARG_TYPE_BrowseFlag",
                "string",
                false,
                &["BrowseMetadata", "BrowseDirectChildren"],
            ),
            ("A_ARG_TYPE_Filter", "string", false, &[]),
            ("A_ARG_TYPE_SortCriteria", "string", false, &[]),
            ("A_ARG_TYPE_Index", "ui4", false, &[]),
            ("A_ARG_TYPE_Count", "ui4", false, &[]),
            ("A_ARG_TYPE_UpdateID", "ui4", false, &[]),
            ("SearchCapabilities", "string", false, &[]),
            ("SortCapabilities", "string", false, &[]),
            ("SystemUpdateID", "ui4", true, &[]),
        ],
    )
}

pub fn connection_manager_scpd() -> String {
    scpd(
        &[
            (
                "GetProtocolInfo",
                &[
                    ("Source", false, "SourceProtocolInfo"),
                    ("Sink", false, "SinkProtocolInfo"),
                ],
            ),
            (
                "GetCurrentConnectionIDs",
                &[("ConnectionIDs", false, "CurrentConnectionIDs")],
            ),
            (
                "GetCurrentConnectionInfo",
                &[
                    ("ConnectionID", true, "A_ARG_TYPE_ConnectionID"),
                    ("RcsID", false, "A_ARG_TYPE_RcsID"),
                    ("AVTransportID", false, "A_ARG_TYPE_AVTransportID"),
                    ("ProtocolInfo", false, "A_ARG_TYPE_ProtocolInfo"),
                    (
                        "PeerConnectionManager",
                        false,
                        "A_ARG_TYPE_ConnectionManager",
                    ),
                    ("PeerConnectionID", false, "A_ARG_TYPE_ConnectionID"),
                    ("Direction", false, "A_ARG_TYPE_Direction"),
                    ("Status", false, "A_ARG_TYPE_ConnectionStatus"),
                ],
            ),
        ],
        &[
            ("SourceProtocolInfo", "string", true, &[]),
            ("SinkProtocolInfo", "string", true, &[]),
            ("CurrentConnectionIDs", "string", true, &[]),
            (
                "A_ARG_TYPE_ConnectionStatus",
                "string",
                false,
                &[
                    "OK",
                    "ContentFormatMismatch",
                    "InsufficientBandwidth",
                    "UnreliableChannel",
                    "Unknown",
                ],
            ),
            ("A_ARG_TYPE_ConnectionManager", "string", false, &[]),
            (
                "A_ARG_TYPE_Direction",
                "string",
                false,
                &["Input", "Output"],
            ),
            ("A_ARG_TYPE_ProtocolInfo", "string", false, &[]),
            ("A_ARG_TYPE_ConnectionID", "i4", false, &[]),
            ("A_ARG_TYPE_AVTransportID", "i4", false, &[]),
            ("A_ARG_TYPE_RcsID", "i4", false, &[]),
        ],
    )
}

pub fn protocol_info() -> String {
    media::mime_types()
        .into_iter()
        .map(|mime| format!("http-get:*:{mime}:*"))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn soap(service: &str, action: &str, args: &[(&str, String)]) -> String {
    let args: String = args
        .iter()
        .map(|(name, value)| format!("<{name}>{}</{name}>", escape(value)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
         <u:{action}Response xmlns:u=\"{service}\">{args}</u:{action}Response>\
         </s:Body></s:Envelope>"
    )
}

pub fn fault(code: u16, description: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><s:Fault>\
         <faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail>\
         <UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\">\
         <errorCode>{code}</errorCode><errorDescription>{}</errorDescription>\
         </UPnPError></detail></s:Fault></s:Body></s:Envelope>",
        escape(description)
    )
}

pub enum Object<'a> {
    Root(&'a str, usize),
    Folder(Kind, usize),
    File(Kind, &'a str, &'a Item),
}

pub fn didl(objects: &[Object], base: &str) -> String {
    let body: String = objects
        .iter()
        .map(|object| match object {
            Object::Root(name, children) => format!(
                "<container id=\"0\" parentID=\"-1\" restricted=\"1\" childCount=\"{children}\">\
                 <dc:title>{}</dc:title><upnp:class>object.container.storageFolder</upnp:class></container>",
                escape(name)
            ),
            Object::Folder(kind, children) => format!(
                "<container id=\"{}\" parentID=\"0\" restricted=\"1\" childCount=\"{children}\">\
                 <dc:title>{}</dc:title><upnp:class>object.container.storageFolder</upnp:class></container>",
                kind.container(),
                kind.title()
            ),
            Object::File(kind, mime, item) => format!(
                "<item id=\"{}\" parentID=\"{}\" restricted=\"1\"><dc:title>{}</dc:title>\
                 <upnp:class>{}</upnp:class>\
                 <res protocolInfo=\"http-get:*:{mime}:{}\" size=\"{}\">{base}/media/{}</res></item>",
                escape(&item.id),
                kind.container(),
                escape(&item.name),
                kind.class(),
                media::DLNA_FEATURES,
                item.size,
                escape(&item.id),
            ),
        })
        .collect();
    format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">{body}</DIDL-Lite>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_read_from_a_soap_body() {
        let body = "<s:Body><u:Browse xmlns:u=\"x\"><ObjectID>video</ObjectID>\
                    <BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter>\
                    <StartingIndex>2</StartingIndex><RequestedCount>10</RequestedCount>\
                    <SortCriteria/><ObjectIDs>no</ObjectIDs></u:Browse></s:Body>";
        assert_eq!(arg(body, "ObjectID").as_deref(), Some("video"));
        assert_eq!(arg(body, "StartingIndex").as_deref(), Some("2"));
        assert_eq!(arg(body, "SortCriteria").as_deref(), Some(""));
        assert_eq!(arg(body, "Missing"), None);
        assert_eq!(
            arg("<Filter>dc:title,&lt;x&gt;</Filter>", "Filter").as_deref(),
            Some("dc:title,<x>")
        );
    }

    #[test]
    fn names_are_escaped_in_the_listing() {
        let item = Item {
            id: "ab".into(),
            name: "Tom & <Jerry>.mp4".into(),
            size: 7,
        };
        let didl = didl(
            &[Object::File(Kind::Video, "video/mp4", &item)],
            "http://10.0.0.2:8200",
        );
        assert!(didl.contains("<dc:title>Tom &amp; &lt;Jerry&gt;.mp4</dc:title>"));
        assert!(didl.contains("size=\"7\">http://10.0.0.2:8200/media/ab</res>"));
    }
}
