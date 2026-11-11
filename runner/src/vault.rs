use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Result, bail};
use serde_json::Value;
use std::path::PathBuf;

// Full reasoning is stored only encrypted; the key lives in the runner and is never logged.
pub struct Vault {
    dir: PathBuf,
    cipher: Aes256Gcm,
}

fn safe(s: &str) -> Result<&str> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("invalid vault name");
    }
    Ok(s)
}

impl Vault {
    pub fn open(dir: &str, key_file: &str) -> Result<Self> {
        let key = match std::fs::read_to_string(key_file) {
            Ok(h) => hex::decode(h.trim())?,
            Err(_) => {
                let mut k = [0u8; 32];
                OsRng.fill_bytes(&mut k);
                if let Some(parent) = std::path::Path::new(key_file).parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(key_file, hex::encode(k))?;
                k.to_vec()
            }
        };
        std::fs::create_dir_all(dir)?;
        Ok(Self { dir: dir.into(), cipher: Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("{e}"))? })
    }

    pub fn store(&self, token_id: &str, request_id: &str, entry: &Value) -> Result<()> {
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ct = self.cipher.encrypt(Nonce::from_slice(&nonce), serde_json::to_vec(entry)?.as_slice()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let dir = self.dir.join(safe(token_id)?);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{}.bin", safe(request_id)?)), [nonce.to_vec(), ct].concat())?;
        Ok(())
    }

    pub fn load(&self, token_id: &str, request_id: &str) -> Result<Value> {
        let raw = std::fs::read(self.dir.join(safe(token_id)?).join(format!("{}.bin", safe(request_id)?)))?;
        if raw.len() < 13 {
            bail!("corrupt vault entry");
        }
        let pt = self.cipher.decrypt(Nonce::from_slice(&raw[..12]), &raw[12..]).map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(serde_json::from_slice(&pt)?)
    }

    pub fn list(&self, token_id: &str) -> Result<Vec<String>> {
        let dir = self.dir.join(safe(token_id)?);
        let mut ids: Vec<(std::time::SystemTime, String)> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path().file_stem()?.to_str()?.to_owned())))
                .collect(),
            Err(_) => vec![],
        };
        ids.sort();
        Ok(ids.into_iter().map(|(_, id)| id).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_and_are_not_plaintext_on_disk() {
        let dir = std::env::temp_dir().join(format!("vault-test-{}", std::process::id()));
        let v = Vault::open(dir.to_str().unwrap(), dir.join("k").to_str().unwrap()).unwrap();
        let entry = serde_json::json!({"reasoning": "secret plan: long BTC"});
        v.store("1", "req-1", &entry).unwrap();
        assert_eq!(v.load("1", "req-1").unwrap(), entry);
        let disk = std::fs::read(dir.join("1").join("req-1.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&disk).contains("secret plan"));
        assert_eq!(v.list("1").unwrap(), vec!["req-1".to_string()]);
        assert!(v.store("../x", "r", &entry).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
