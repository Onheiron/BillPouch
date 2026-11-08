use crate::{
    error::{BpError, BpResult},
    identity::{parse_user_key, Identity},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use serde::{Deserialize, Serialize};

pub const SESSION_SECS: u64 = 12 * 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Session {
    user_key: String,
    expires: u64,
    nonce: u64,
    signature: String,
}

fn message(user_key: &str, expires: u64, nonce: u64) -> Vec<u8> {
    let mut msg = b"billpouch/api-session/v1".to_vec();
    msg.extend_from_slice(&serde_json::to_vec(&(user_key, expires, nonce)).unwrap_or_default());
    msg
}

pub fn issue(identity: &Identity, now: u64) -> BpResult<String> {
    let expires = now + SESSION_SECS;
    let nonce = rand::random();
    let signature = identity
        .user
        .sign(&message(&identity.user_key, expires, nonce))
        .map_err(|e| BpError::Identity(format!("cannot sign the session: {e}")))?;
    let session = Session {
        user_key: identity.user_key.clone(),
        expires,
        nonce,
        signature: hex::encode(signature),
    };
    Ok(B64.encode(serde_json::to_vec(&session)?))
}

pub fn verify(token: &str, user_key: &str, now: u64) -> BpResult<()> {
    let bytes = B64
        .decode(token.trim())
        .map_err(|_| BpError::Identity("malformed session".into()))?;
    let session: Session = serde_json::from_slice(&bytes)?;
    if session.user_key != user_key {
        return Err(BpError::Identity("session of another user".into()));
    }
    if now >= session.expires || session.expires > now + SESSION_SECS {
        return Err(BpError::Identity("session expired".into()));
    }
    let (public, _) = parse_user_key(&session.user_key)?;
    let signature =
        hex::decode(&session.signature).map_err(|_| BpError::Identity("bad signature".into()))?;
    if !public.verify(
        &message(&session.user_key, session.expires, session.nonce),
        &signature,
    ) {
        return Err(BpError::Identity("invalid session signature".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_fresh_session_signed_by_the_daemons_user_passes() {
        let carlo = Identity::in_memory(None).unwrap();
        let marco = Identity::in_memory(None).unwrap();
        let token = issue(&carlo, 1000).unwrap();
        assert!(verify(&token, &carlo.user_key, 1001).is_ok());
        assert!(verify(&token, &marco.user_key, 1001).is_err());
        assert!(verify(&token, &carlo.user_key, 1000 + SESSION_SECS).is_err());
        assert!(verify(&issue(&marco, 1000).unwrap(), &carlo.user_key, 1001).is_err());

        let mut forged: Session = serde_json::from_slice(&B64.decode(&token).unwrap()).unwrap();
        forged.expires += 10;
        let forged = B64.encode(serde_json::to_vec(&forged).unwrap());
        assert!(verify(&forged, &carlo.user_key, 1001).is_err());
        assert!(verify("nonsense", &carlo.user_key, 1001).is_err());
    }
}
