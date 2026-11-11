use alloy::dyn_abi::TypedData;
use alloy::primitives::{Address, B256, Signature, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result, bail};
use base64::Engine;
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf};

const RPC: &str = "https://rpc.testnet.near.org";

fn var(k: &str) -> Result<String> {
    env::var(k).with_context(|| format!("missing env {k}"))
}

fn agent_key(token_id: &str) -> Result<PrivateKeySigner> {
    let path = PathBuf::from(format!(".secrets/agent-{token_id}.key"));
    if let Ok(hex) = fs::read_to_string(&path) {
        return Ok(hex.trim().parse()?);
    }
    let k = PrivateKeySigner::random();
    fs::create_dir_all(".secrets")?;
    fs::write(&path, alloy::hex::encode(k.to_bytes()))?;
    Ok(k)
}

async fn view(contract: &str, method: &str, args: Value) -> Result<Value> {
    let args_b64 = base64::engine::general_purpose::STANDARD.encode(args.to_string());
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "query", "params": {
        "request_type": "call_function", "finality": "final", "account_id": contract,
        "method_name": method, "args_base64": args_b64}});
    let resp: Value = reqwest::Client::new().post(RPC).json(&req).send().await?.json().await?;
    let bytes: Vec<u8> = serde_json::from_value(resp["result"]["result"].clone()).with_context(|| format!("{resp}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn derived_address(mpc: &str, predecessor: &str, path: &str) -> Result<Address> {
    let pk = view(mpc, "derived_public_key", json!({"path": path, "predecessor": predecessor, "domain_id": 0})).await?;
    let b58 = pk.as_str().and_then(|s| s.strip_prefix("secp256k1:")).context("not a secp256k1 key")?;
    Ok(Address::from_raw_public_key(&bs58::decode(b58).into_vec()?))
}

fn approve_agent_hash(chain: &str, agent: Address, nonce: u64) -> Result<B256> {
    let td: TypedData = serde_json::from_value(json!({
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"}, {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"}, {"name": "verifyingContract", "type": "address"}
            ],
            "HyperliquidTransaction:ApproveAgent": [
                {"name": "hyperliquidChain", "type": "string"}, {"name": "agentAddress", "type": "address"},
                {"name": "agentName", "type": "string"}, {"name": "nonce", "type": "uint64"}
            ]
        },
        "primaryType": "HyperliquidTransaction:ApproveAgent",
        "domain": {"name": "HyperliquidSignTransaction", "version": "1", "chainId": 0x66eee,
                   "verifyingContract": Address::ZERO},
        "message": {"hyperliquidChain": chain, "agentAddress": agent, "agentName": "", "nonce": nonce}
    }))?;
    Ok(td.eip712_signing_hash()?)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = env::args().collect();
    let (cmd, token_id) = (args.get(1).map(String::as_str), args.get(2).cloned().unwrap_or("1".into()));
    let hl_address = derived_address(&var("MPC_CONTRACT")?, &var("CONTRACT_ID")?, &format!("agent-{token_id}")).await?;
    let agent = agent_key(&token_id)?;

    match cmd {
        Some("prepare") => {
            println!("{}", json!({"hl_address": hl_address, "agent_address": agent.address()}));
        }
        Some("submit") => {
            let out: Value = serde_json::from_str(&fs::read_to_string(args.get(3).context("signed action file")?)?)?;
            let sig = &out["signature"];
            let nonce = out["nonce"].as_u64().context("nonce")?;
            let r: U256 = sig["r"].as_str().context("r")?.parse()?;
            let s: U256 = sig["s"].as_str().context("s")?.parse()?;
            let parity = sig["v"].as_u64().context("v")? == 28;
            let chain = out["action"]["hyperliquidChain"].as_str().context("chain")?;
            let recovered = Signature::new(r, s, parity).recover_address_from_prehash(&approve_agent_hash(chain, agent.address(), nonce)?)?;
            println!("derived {hl_address}\nrecovered {recovered}");
            if recovered != hl_address {
                bail!("MPC signature does not recover to the NFT's derived address");
            }
            let body = json!({"action": out["action"], "nonce": nonce, "signature": sig});
            let resp: Value = reqwest::Client::new()
                .post(format!("{}/exchange", var("HL_API")?))
                .json(&body)
                .send()
                .await?
                .json()
                .await?;
            println!("hyperliquid: {resp}");
        }
        _ => bail!("usage: gonogo_approve prepare|submit <token_id> [signed.json]"),
    }
    Ok(())
}
