#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Video,
    Audio,
    Image,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Video, Kind::Audio, Kind::Image];

    pub fn container(self) -> &'static str {
        match self {
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Image => "image",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Video => "Video",
            Kind::Audio => "Music",
            Kind::Image => "Pictures",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Kind::Video => "object.item.videoItem",
            Kind::Audio => "object.item.audioItem.musicTrack",
            Kind::Image => "object.item.imageItem.photo",
        }
    }

    pub fn from_container(id: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.container() == id)
    }
}

const TYPES: &[(&str, &str, Kind)] = &[
    ("mp4", "video/mp4", Kind::Video),
    ("m4v", "video/mp4", Kind::Video),
    ("mkv", "video/x-matroska", Kind::Video),
    ("webm", "video/webm", Kind::Video),
    ("avi", "video/x-msvideo", Kind::Video),
    ("mov", "video/quicktime", Kind::Video),
    ("ts", "video/mp2t", Kind::Video),
    ("m2ts", "video/mp2t", Kind::Video),
    ("mpg", "video/mpeg", Kind::Video),
    ("mpeg", "video/mpeg", Kind::Video),
    ("mp3", "audio/mpeg", Kind::Audio),
    ("flac", "audio/flac", Kind::Audio),
    ("m4a", "audio/mp4", Kind::Audio),
    ("aac", "audio/aac", Kind::Audio),
    ("ogg", "audio/ogg", Kind::Audio),
    ("oga", "audio/ogg", Kind::Audio),
    ("opus", "audio/ogg", Kind::Audio),
    ("wav", "audio/wav", Kind::Audio),
    ("jpg", "image/jpeg", Kind::Image),
    ("jpeg", "image/jpeg", Kind::Image),
    ("png", "image/png", Kind::Image),
    ("gif", "image/gif", Kind::Image),
];

pub fn kind(name: &str) -> Option<(&'static str, Kind)> {
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    TYPES
        .iter()
        .find(|(e, _, _)| *e == ext)
        .map(|(_, mime, kind)| (*mime, *kind))
}

pub fn mime_types() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = Vec::new();
    for (_, mime, _) in TYPES {
        if !all.contains(mime) {
            all.push(mime);
        }
    }
    all
}

pub const DLNA_FEATURES: &str =
    "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_is_recognised_by_extension() {
        assert_eq!(kind("Film.MKV"), Some(("video/x-matroska", Kind::Video)));
        assert_eq!(kind("song.flac"), Some(("audio/flac", Kind::Audio)));
        assert_eq!(kind("notes.txt"), None);
        assert_eq!(kind("mp4"), None);
    }

    #[test]
    fn each_mime_type_is_listed_once() {
        let all = mime_types();
        assert!(all.contains(&"video/mp4"));
        assert_eq!(all.iter().filter(|m| **m == "video/mp4").count(), 1);
    }
}
