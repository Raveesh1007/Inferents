use alloy::primitives::{Address, Signature};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, time::Duration};

const API: &str = "https://cloud-api.near.ai";
const MODEL: &str = "deepseek-ai/DeepSeek-V4.1-Flash-1017";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let key = env::var("NEAR_AI_API_KEY").context("NEAR_AI_API_KEY missing")?;
    let http = reqwest::Client::new();

    let report: Value = http.get(format!("{API}/v1/attestation/report")).query(&[("model", MODEL), ("signing_algo", "ecdsa")]).send().await?.json().await?;
    let attested = report["model_attestations"].as_array().context("no model_attestations")?;
    ensure!(attested.len() == 1, "expected exactly one model attestation, got {}", attested.len());
    let signing_address: Address = attested[0]["signing_address"].as_str().context("signing_address")?.parse()?;
    println!("attested signing_address: {signing_address}");

    // The signed text hashes these exact request bytes, so they are built once and sent verbatim.
    let req_bytes = serde_json::to_vec(&json!({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Reply with one word: BTC bullish or bearish?"}],
        "max_tokens": 40
    }))?;
    let resp = http
        .post(format!("{API}/v1/chat/completions"))
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .header("Accept-Encoding", "identity")
        .header("x-no-aliasing", "true")
        .body(req_bytes.clone())
        .send()
        .await?;
    ensure!(resp.status().is_success(), "completion failed: {}", resp.status());
    let inference_id = resp.headers().get("inference-id").context("no inference-id header")?.to_str()?.to_owned();
    let resp_bytes = resp.bytes().await?;
    let body: Value = serde_json::from_slice(&resp_bytes)?;
    let chat_id = body["id"].as_str().context("no completion id")?.to_owned();
    println!("chat_id {chat_id}\ninference-id {inference_id}\nreply: {}", body["choices"][0]["message"]["content"].as_str().unwrap_or("").trim());

    let req_hash = hex::encode(Sha256::digest(&req_bytes));
    let resp_hash = hex::encode(Sha256::digest(&resp_bytes));

    let mut sig = Value::Null;
    for _ in 0..10 {
        let r = http
            .get(format!("{API}/v1/signature/{chat_id}"))
            .query(&[("signing_algo", "ecdsa")])
            .header("Authorization", format!("Bearer {key}"))
            .send()
            .await?;
        if r.status().is_success() {
            let v: Value = r.json().await?;
            if v.get("error_code").is_none() {
                sig = v;
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    ensure!(!sig.is_null(), "signature never became available");
    let kind = sig["signature_kind"].as_str().unwrap_or("absent");
    let text = sig["text"].as_str().context("text")?;
    println!("signature_kind: {kind}\nsigned text: {text}");
    let expected = format!("{MODEL}:{req_hash}:{resp_hash}");
    if kind != "provider_tee" || text != expected {
        bail!("signature is not a provider_tee signature over our exact bytes (expected text {expected})");
    }

    let sig_bytes = hex::decode(sig["signature"].as_str().context("signature")?.trim_start_matches("0x"))?;
    ensure!(sig_bytes.len() == 65, "signature is {} bytes", sig_bytes.len());
    let recovered = Signature::from_raw(&sig_bytes)?.recover_address_from_msg(text.as_bytes())?;
    println!("recovered signer:         {recovered}");
    ensure!(recovered == signing_address, "recovered signer does not match the attested signing_address");
    ensure!(sig["signing_address"].as_str().map(|a| a.parse::<Address>().ok()) == Some(Some(signing_address)), "signature signing_address differs from attestation");

    let mut cost = Value::Null;
    for _ in 0..10 {
        let v: Value = http
            .post(format!("{API}/v1/billing/costs"))
            .header("Authorization", format!("Bearer {key}"))
            .json(&json!({"requestIds": [inference_id]}))
            .send()
            .await?
            .json()
            .await?;
        if v["requests"][0]["costNanoUsd"].as_i64().unwrap_or(0) > 0 {
            cost = v;
            break;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    ensure!(!cost.is_null(), "cost never became visible");
    println!("cost: {} nano-USD", cost["requests"][0]["costNanoUsd"]);

    let receipt = json!({
        "model": MODEL, "chat_id": chat_id, "request_id": inference_id,
        "request_hash": req_hash, "response_hash": resp_hash, "text": text,
        "signature": sig["signature"], "signing_address": signing_address,
        "cost_nano_usd": cost["requests"][0]["costNanoUsd"]
    });
    fs::write("runner/fixtures/receipt1.json", serde_json::to_string_pretty(&receipt)?)?;
    println!("receipt saved to runner/fixtures/receipt1.json");
    Ok(())
}
