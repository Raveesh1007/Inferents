use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use borsh::BorshSerialize;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const RECIPIENT: &str = "inferents-reasoning";
const TAG: u32 = (1 << 31) + 413;
const MAX_AGE_MS: u64 = 5 * 60 * 1000;

#[derive(BorshSerialize)]
struct Payload {
    message: String,
    nonce: [u8; 32],
    recipient: String,
    callback_url: Option<String>,
}

// What the wallet's signMessage returns, plus the message and nonce it was called with.
#[derive(Deserialize)]
pub struct Signed {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub signature: String,
    pub message: String,
    pub nonce: String,
}

pub fn digest(message: &str, nonce: [u8; 32], recipient: &str) -> Result<[u8; 32]> {
    let payload = Payload { message: message.to_owned(), nonce, recipient: recipient.to_owned(), callback_url: None };
    let mut bytes = borsh::to_vec(&TAG)?;
    bytes.extend(borsh::to_vec(&payload)?);
    Ok(Sha256::digest(bytes).into())
}

// Checks the signature only; the caller must still confirm that `public_key` is an access key of `account_id`.
pub fn verify(s: &Signed, now_ms: u64) -> Result<()> {
    let nonce: [u8; 32] = B64.decode(&s.nonce)?.try_into().map_err(|_| anyhow::anyhow!("nonce must be 32 bytes"))?;
    let key = s.public_key.strip_prefix("ed25519:").context("only ed25519 keys")?;
    let key: [u8; 32] = bs58::decode(key).into_vec()?.try_into().map_err(|_| anyhow::anyhow!("bad public key"))?;
    let sig: [u8; 64] = B64.decode(&s.signature)?.try_into().map_err(|_| anyhow::anyhow!("bad signature length"))?;
    VerifyingKey::from_bytes(&key)?
        .verify(&digest(&s.message, nonce, RECIPIENT)?, &Signature::from_bytes(&sig))
        .map_err(|_| anyhow::anyhow!("signature does not verify"))?;
    let ts: u64 = serde_json::from_str::<serde_json::Value>(&s.message)?["ts"].as_u64().context("message has no ts")?;
    ensure!(now_ms.abs_diff(ts) <= MAX_AGE_MS, "signed message is older than 5 minutes");
    Ok(())
}

// The signed message names exactly what is being requested, so a signature for one receipt cannot be replayed for another.
pub fn requested(s: &Signed) -> Result<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(&s.message)?;
    match (v["token"].as_str(), v["receipt"].as_str()) {
        (Some(t), Some(r)) => Ok((t.to_owned(), r.to_owned())),
        _ => bail!("message must name token and receipt"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(msg: &str, key: &SigningKey) -> Signed {
        let nonce = [7u8; 32];
        let sig = key.sign(&digest(msg, nonce, RECIPIENT).unwrap());
        Signed {
            account_id: "alice.testnet".into(),
            public_key: format!("ed25519:{}", bs58::encode(key.verifying_key().to_bytes()).into_string()),
            signature: B64.encode(sig.to_bytes()),
            message: msg.into(),
            nonce: B64.encode(nonce),
        }
    }

    #[test]
    fn valid_fresh_signature_verifies_and_names_its_request() {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let s = signed(r#"{"token":"3","receipt":"abc","ts":1000000}"#, &key);
        verify(&s, 1_000_500).unwrap();
        assert_eq!(requested(&s).unwrap(), ("3".to_string(), "abc".to_string()));
    }

    #[test]
    fn stale_tampered_or_foreign_signatures_are_rejected() {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let s = signed(r#"{"token":"3","receipt":"abc","ts":1000000}"#, &key);
        assert!(verify(&s, 1_000_000 + 6 * 60 * 1000).is_err());
        let tampered = Signed { message: r#"{"token":"4","receipt":"abc","ts":1000000}"#.into(), ..s };
        assert!(verify(&tampered, 1_000_000).is_err());
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let foreign = Signed { public_key: signed("x", &other).public_key, ..signed(r#"{"token":"3","receipt":"abc","ts":1000000}"#, &key) };
        assert!(verify(&foreign, 1_000_000).is_err());
    }
}
