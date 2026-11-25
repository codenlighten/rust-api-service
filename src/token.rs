//! Signed bearer tokens: `v1.<base64url(claims json)>.<base64url(HMAC-SHA256)>`.
//!
//! The MAC covers the version prefix and the encoded claims. Verification compares MACs in
//! constant time and checks the expiry against a caller-supplied clock.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// User id.
    pub sub: i64,
    /// Expiry, seconds since the Unix epoch.
    pub exp: u64,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("token is malformed")]
    Malformed,
    #[error("token signature is invalid")]
    BadSignature,
    #[error("token has expired")]
    Expired,
}

pub fn sign(secret: &[u8], claims: &Claims) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims serialize"));
    let signed = format!("{VERSION}.{payload}");
    let mac = mac_for(secret, signed.as_bytes()).finalize().into_bytes();
    format!("{signed}.{}", URL_SAFE_NO_PAD.encode(mac))
}

pub fn verify(secret: &[u8], token: &str, now: u64) -> Result<Claims, TokenError> {
    let mut parts = token.split('.');
    let (Some(version), Some(payload), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(TokenError::Malformed);
    };
    if version != VERSION {
        return Err(TokenError::Malformed);
    }
    let sig = URL_SAFE_NO_PAD
        .decode(sig)
        .map_err(|_| TokenError::Malformed)?;

    mac_for(secret, format!("{version}.{payload}").as_bytes())
        .verify_slice(&sig)
        .map_err(|_| TokenError::BadSignature)?;

    let raw = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TokenError::Malformed)?;
    let claims: Claims = serde_json::from_slice(&raw).map_err(|_| TokenError::Malformed)?;
    if claims.exp <= now {
        return Err(TokenError::Expired);
    }
    Ok(claims)
}

fn mac_for(secret: &[u8], data: &[u8]) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(data);
    mac
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";
    const CLAIMS: Claims = Claims { sub: 7, exp: 1_000 };

    #[test]
    fn round_trip() {
        let token = sign(SECRET, &CLAIMS);
        assert_eq!(verify(SECRET, &token, 999), Ok(CLAIMS));
    }

    #[test]
    fn expiry_is_enforced_at_the_boundary() {
        let token = sign(SECRET, &CLAIMS);
        assert_eq!(verify(SECRET, &token, 1_000), Err(TokenError::Expired));
        assert_eq!(verify(SECRET, &token, 2_000), Err(TokenError::Expired));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token = sign(SECRET, &CLAIMS);
        assert_eq!(
            verify(b"another-secret-of-32-bytes-or-more!", &token, 1),
            Err(TokenError::BadSignature)
        );
    }

    #[test]
    fn tampered_claims_are_rejected() {
        let token = sign(SECRET, &CLAIMS);
        let forged = sign(SECRET, &Claims { sub: 8, exp: 1_000 });
        let sig = token.rsplit('.').next().unwrap();
        let forged_payload = forged.split('.').nth(1).unwrap();
        let spliced = format!("v1.{forged_payload}.{sig}");
        assert_eq!(verify(SECRET, &spliced, 1), Err(TokenError::BadSignature));
    }

    #[test]
    fn garbage_is_malformed() {
        for bad in ["", "abc", "v1.a.b.c", "v2.a.b", "v1.!!.@@"] {
            assert_eq!(verify(SECRET, bad, 1), Err(TokenError::Malformed), "{bad}");
        }
    }
}
