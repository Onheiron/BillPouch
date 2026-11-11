use crate::error::{BpError, BpResult};
use serde::{de::DeserializeOwned, Serialize};

pub fn encode<T: Serialize>(value: &T) -> BpResult<Vec<u8>> {
    cbor4ii::serde::to_vec(Vec::new(), value).map_err(|e| BpError::Encoding(e.to_string()))
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> BpResult<T> {
    cbor4ii::serde::from_slice(bytes).map_err(|e| BpError::Encoding(e.to_string()))
}

pub fn digest<T: Serialize>(value: &T) -> String {
    blake3::hash(&encode(value).unwrap_or_default())
        .to_hex()
        .to_string()
}

pub fn signing_bytes<T: Serialize>(context: &[u8], value: &T) -> Vec<u8> {
    let mut bytes = context.to_vec();
    bytes.extend_from_slice(&encode(value).unwrap_or_default());
    bytes
}

pub mod hex_bytes {
    use serde::{de, Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(hex_text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return serializer.serialize_str(hex_text);
        }
        let bytes = hex::decode(hex_text).map_err(serde::ser::Error::custom)?;
        serializer.serialize_bytes(&bytes)
    }

    struct HexOrBytes;

    impl de::Visitor<'_> for HexOrBytes {
        type Value = String;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("bytes or a hex string")
        }

        fn visit_str<E: de::Error>(self, text: &str) -> Result<String, E> {
            Ok(text.to_string())
        }

        fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<String, E> {
            Ok(hex::encode(bytes))
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_any(HexOrBytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Signed {
        #[serde(with = "hex_bytes")]
        signature: String,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Tagged {
        Signed {
            #[serde(with = "hex_bytes")]
            signature: String,
        },
    }

    #[test]
    fn hex_fields_travel_as_bytes_in_cbor_and_as_text_in_json() {
        let signed = Signed {
            signature: "ab".repeat(64),
        };
        let binary = encode(&signed).unwrap();
        assert!(binary.len() < 64 + 16);
        assert_eq!(decode::<Signed>(&binary).unwrap(), signed);
        let json = serde_json::to_string(&signed).unwrap();
        assert!(json.contains(&signed.signature));
        assert_eq!(serde_json::from_str::<Signed>(&json).unwrap(), signed);
    }

    #[test]
    fn hex_fields_survive_internally_tagged_enums() {
        let tagged = Tagged::Signed {
            signature: "0f".repeat(32),
        };
        assert_eq!(decode::<Tagged>(&encode(&tagged).unwrap()).unwrap(), tagged);
        let json = serde_json::to_vec(&tagged).unwrap();
        assert_eq!(serde_json::from_slice::<Tagged>(&json).unwrap(), tagged);
    }

    #[test]
    fn a_hex_field_that_is_not_hex_does_not_encode() {
        let signed = Signed {
            signature: "not hex".into(),
        };
        assert!(encode(&signed).is_err());
    }
}
