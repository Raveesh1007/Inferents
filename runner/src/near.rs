use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use borsh::BorshSerialize;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(BorshSerialize)]
enum PublicKey {
    Ed25519([u8; 32]),
}

#[derive(BorshSerialize)]
enum Signature {
    Ed25519([u8; 64]),
}

#[derive(BorshSerialize)]
struct FunctionCall {
    method_name: String,
    args: Vec<u8>,
    gas: u64,
    deposit: u128,
}

// Variant order is the protocol's action index; FunctionCall must stay at 2.
#[derive(BorshSerialize)]
enum Action {
    _CreateAccount,
    _DeployContract(Vec<u8>),
    FunctionCall(FunctionCall),
}

#[derive(BorshSerialize)]
struct Transaction {
    signer_id: String,
    public_key: PublicKey,
    nonce: u64,
    receiver_id: String,
    block_hash: [u8; 32],
    actions: Vec<Action>,
}

#[derive(BorshSerialize)]
struct SignedTransaction {
    transaction: Transaction,
    signature: Signature,
}

pub struct Near {
    rpc: String,
    pub account_id: String,
    key: SigningKey,
    http: reqwest::Client,
}

impl Near {
    pub fn new(rpc: &str, account_id: &str, secret: &str) -> Result<Self> {
        let raw = bs58::decode(secret.strip_prefix("ed25519:").unwrap_or(secret)).into_vec()?;
        let seed: [u8; 32] = raw.get(..32).context("short private key")?.try_into()?;
        Ok(Self { rpc: rpc.into(), account_id: account_id.into(), key: SigningKey::from_bytes(&seed), http: reqwest::Client::new() })
    }

    pub fn from_credentials_file(rpc: &str, account_id: &str, path: &str) -> Result<Self> {
        let j: Value = serde_json::from_str(&std::fs::read_to_string(path).with_context(|| path.to_owned())?)?;
        Self::new(rpc, account_id, j["private_key"].as_str().context("private_key")?)
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let r: Value = self.http.post(&self.rpc).json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})).send().await?.json().await?;
        if let Some(e) = r.get("error") {
            bail!("rpc {method}: {e}");
        }
        Ok(r["result"].clone())
    }

    // True when `public_key` is a key of `account_id`, so a signature from it speaks for that account.
    pub async fn has_access_key(&self, account_id: &str, public_key: &str) -> bool {
        self.rpc("query", json!({"request_type": "view_access_key", "finality": "final", "account_id": account_id, "public_key": public_key})).await.is_ok()
    }

    pub async fn view(&self, contract: &str, method: &str, args: Value) -> Result<Value> {
        let r = self
            .rpc("query", json!({"request_type": "call_function", "finality": "final", "account_id": contract,
                "method_name": method, "args_base64": B64.encode(args.to_string())}))
            .await?;
        if let Some(e) = r.get("error") {
            bail!("view {contract}.{method}: {e}");
        }
        let bytes: Vec<u8> = serde_json::from_value(r["result"].clone())?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn call(&self, contract: &str, method: &str, args: Value, tgas: u64, deposit_yocto: u128) -> Result<Value> {
        let pk = self.key.verifying_key().to_bytes();
        let pk_str = format!("ed25519:{}", bs58::encode(pk).into_string());
        let ak = self.rpc("query", json!({"request_type": "view_access_key", "finality": "final", "account_id": self.account_id, "public_key": pk_str})).await?;
        let tx = Transaction {
            signer_id: self.account_id.clone(),
            public_key: PublicKey::Ed25519(pk),
            nonce: ak["nonce"].as_u64().with_context(|| format!("access key response: {ak}"))? + 1,
            receiver_id: contract.into(),
            block_hash: bs58::decode(ak["block_hash"].as_str().context("block_hash")?).into_vec()?.try_into().map_err(|_| anyhow::anyhow!("block hash length"))?,
            actions: vec![Action::FunctionCall(FunctionCall { method_name: method.into(), args: args.to_string().into_bytes(), gas: tgas * 1_000_000_000_000, deposit: deposit_yocto })],
        };
        let hash = Sha256::digest(borsh::to_vec(&tx)?);
        let signature = Signature::Ed25519(self.key.sign(&hash).to_bytes());
        let signed = borsh::to_vec(&SignedTransaction { transaction: tx, signature })?;
        let out = self.rpc("send_tx", json!({"signed_tx_base64": B64.encode(signed), "wait_until": "FINAL"})).await?;
        match &out["status"] {
            s if s.get("SuccessValue").is_some() => {
                let bytes = B64.decode(s["SuccessValue"].as_str().unwrap_or(""))?;
                Ok(if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes)? })
            }
            s => bail!("tx {contract}.{method} failed: {}", serde_json::to_string(&out["receipts_outcome"].as_array().map(|r| r.iter().map(|o| o["outcome"]["status"].clone()).collect::<Vec<_>>()).unwrap_or_default()).unwrap_or_else(|_| s.to_string())),
        }
    }
}
