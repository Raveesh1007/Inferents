use alloy::primitives::{Address, Signature};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, time::Duration};

const API: &str = "https://cloud-api.near.ai";

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 { format!("{}…", &s[..160]) } else { s }
}

fn addresses(v: &Value, out: &mut Vec<(String, Address)>, path: String) {
    match v {
        Value::String(s) if s.len() == 42 && s.starts_with("0x") => {
            if let Ok(a) = s.parse::<Address>() {
                out.push((path, a));
            }
        }
        Value::Object(m) => m.iter().for_each(|(k, x)| addresses(x, out, format!("{path}.{k}"))),
        Value::Array(a) => a.iter().enumerate().for_each(|(i, x)| addresses(x, out, format!("{path}[{i}]"))),
        _ => {}
    }
}

// usage: gatewayprobe survey            -> which chat models expose an ECDSA signing_address
//        gatewayprobe <model-id>        -> one completion, then try to verify its signature
#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let http = reqwest::Client::new();
    let arg = env::args().nth(1).context("usage: gatewayprobe survey | <model-id>")?;

    if arg == "survey" {
        let models: Value = http.get(format!("{API}/v1/models")).send().await?.json().await?;
        for m in models["data"].as_array().context("models")? {
            let id = m["id"].as_str().unwrap_or("?");
            if id.contains("embedding") || id.contains("Reranker") || id.contains("whisper") || id.contains("FLUX") || id.contains("privacy-filter") {
                continue;
            }
            let r: Value = http.get(format!("{API}/v1/attestation/report")).query(&[("model", id), ("signing_algo", "ecdsa")]).send().await?.json().await?;
            let list = r["model_attestations"].as_array();
            let addr = list.and_then(|l| l.first()).and_then(|a| a["signing_address"].as_str()).unwrap_or("-");
            println!("{id:52} attestations={:<2} signing_address={addr}", list.map_or(0, Vec::len));
        }
        return Ok(());
    }

    let model = arg;
    let key = env::var("NEAR_AI_API_KEY").context("NEAR_AI_API_KEY missing")?;

    let gw: Value = http.get(format!("{API}/v1/attestation/report")).query(&[("signing_algo", "ecdsa")]).send().await?.json().await?;
    println!("== gateway report (no model): keys {:?}", gw.as_object().map(|o| o.keys().collect::<Vec<_>>()));
    let mw: Value = http.get(format!("{API}/v1/attestation/report")).query(&[("model", model.as_str()), ("signing_algo", "ecdsa")]).send().await?.json().await?;
    println!("== report for {model}: keys {:?}", mw.as_object().map(|o| o.keys().collect::<Vec<_>>()));
    let mut candidates = vec![];
    addresses(&gw, &mut candidates, "gateway".into());
    addresses(&mw, &mut candidates, "model".into());
    for (p, a) in &candidates {
        println!("   candidate {p} = {a}");
    }

    let req = serde_json::to_vec(&json!({"model": model, "messages": [{"role": "user", "content": "Reply with one word: BTC bullish or bearish?"}], "max_tokens": 400}))?;
    let resp = http
        .post(format!("{API}/v1/chat/completions"))
        .bearer_auth(&key)
        .header("Content-Type", "application/json")
        .header("Accept-Encoding", "identity")
        .header("x-no-aliasing", "true")
        .body(req.clone())
        .send()
        .await?;
    println!("completion status {}", resp.status());
    let inference_id = resp.headers().get("inference-id").and_then(|v| v.to_str().ok()).map(String::from);
    let hdrs: Vec<String> = resp.headers().iter().filter(|(k, _)| k.as_str().contains("id") || k.as_str().contains("sign") || k.as_str().contains("attest")).map(|(k, v)| format!("{k}={}", v.to_str().unwrap_or("?"))).collect();
    println!("interesting headers: {hdrs:?}");
    let bytes = resp.bytes().await?;
    let body: Value = serde_json::from_slice(&bytes).with_context(|| String::from_utf8_lossy(&bytes).chars().take(300).collect::<String>())?;
    let chat_id = body["id"].as_str().context("completion id")?.to_owned();
    println!("chat_id {chat_id}; reply: {}", short(&body["choices"][0]["message"]["content"]));
    let (rq, rs) = (hex::encode(Sha256::digest(&req)), hex::encode(Sha256::digest(&bytes)));

    for variant in [vec![("signing_algo", "ecdsa")], vec![("signing_algo", "ecdsa"), ("model", model.as_str())]] {
        let mut sig = Value::Null;
        for _ in 0..8 {
            let r = http.get(format!("{API}/v1/signature/{chat_id}")).query(&variant).bearer_auth(&key).send().await?;
            let status = r.status();
            let v: Value = r.json().await.unwrap_or(Value::Null);
            if status.is_success() && v.get("error_code").is_none() && !v.is_null() {
                sig = v;
                break;
            }
            println!("   signature {variant:?}: {status} {}", short(&v));
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if sig.is_null() {
            continue;
        }
        println!("== signature {variant:?}: {sig}");
        let text = sig["text"].as_str().unwrap_or("");
        println!("   text covers our bytes: {}", text.contains(&rq) && text.contains(&rs));
        let Some(raw) = sig["signature"].as_str().and_then(|s| hex::decode(s.trim_start_matches("0x")).ok()) else { continue };
        let Ok(parsed) = Signature::from_raw(&raw) else { println!("   signature is not 65 bytes ({})", raw.len()); continue };
        let recovered = parsed.recover_address_from_msg(text.as_bytes())?;
        println!("   recovered {recovered}");
        let hit = candidates.iter().find(|(_, a)| *a == recovered);
        println!("   matches attested address: {}", hit.map_or("NO".to_owned(), |(p, _)| format!("YES ({p})")));
        if let (Some(_), true, Some(inf)) = (hit, sig["signature_kind"] == "gateway", inference_id.as_ref()) {
            let fx = json!({"model": model, "request_id": inf, "chat_id": chat_id, "request_body": String::from_utf8_lossy(&req), "request_hash": rq, "response_hash": rs,
                "text": text, "signature": sig["signature"], "signing_address": recovered});
            std::fs::write("runner/fixtures/receipt_gateway.json", serde_json::to_string_pretty(&fx)?)?;
            println!("   fixture saved to runner/fixtures/receipt_gateway.json");
        }
    }
    Ok(())
}
