use alloy::primitives::{Address, Signature};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

const API: &str = "https://cloud-api.near.ai";

pub struct Ai {
    http: reqwest::Client,
    key: String,
    pub model: String,
    model_signer: tokio::sync::OnceCell<Address>,
    gateway_signer: tokio::sync::OnceCell<Address>,
}

pub struct Thought {
    pub content: String,
    pub reasoning: String,
    pub chat_id: String,
    pub request_id: String,
    pub model: String,
    pub request_hash: String,
    pub response_hash: String,
    pub signature: String,
    pub cost_micro_usd: u64,
    pub kind: &'static str,
    pub label: &'static str,
    pub request_body: String,
    pub response_body: String,
}

impl Ai {
    pub fn new(key: String, model: String) -> Self {
        Self { http: reqwest::Client::new(), key, model, model_signer: Default::default(), gateway_signer: Default::default() }
    }

    // Closed frontier models go through NEAR AI's gateway, which signs the relayed reply instead of the model itself.
    pub fn is_gateway_model(model: &str) -> bool {
        ["anthropic/", "openai/", "google/", "x-ai/"].iter().any(|p| model.starts_with(p))
    }

    // Closed models spend part of the budget on hidden reasoning before the visible reply.
    pub fn default_max_tokens(model: &str) -> u32 {
        if Self::is_gateway_model(model) { 6000 } else { 4000 }
    }

    async fn report_signer(&self, model: Option<&str>) -> Result<Address> {
        let mut q = vec![("signing_algo", "ecdsa")];
        q.extend(model.map(|m| ("model", m)));
        let r: Value = self.http.get(format!("{API}/v1/attestation/report")).query(&q).send().await?.json().await?;
        match model {
            Some(_) => {
                let list = r["model_attestations"].as_array().context("no model_attestations")?;
                ensure!(list.len() == 1, "expected one model attestation, got {}", list.len());
                Ok(list[0]["signing_address"].as_str().context("signing_address")?.parse()?)
            }
            None => Ok(r["gateway_attestation"]["signing_address"].as_str().context("gateway signing_address")?.parse()?),
        }
    }

    pub async fn think(&self, system: &str, user: &str, max_tokens: u32) -> Result<Thought> {
        // The signature covers these exact request bytes, so they are built once and sent verbatim.
        let req = serde_json::to_vec(&json!({
            "model": self.model,
            "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "max_tokens": max_tokens
        }))?;
        let resp = self
            .http
            .post(format!("{API}/v1/chat/completions"))
            .bearer_auth(&self.key)
            .header("Content-Type", "application/json")
            .header("Accept-Encoding", "identity")
            .header("x-no-aliasing", "true")
            .body(req.clone())
            .send()
            .await?;
        ensure!(resp.status().is_success(), "completion failed: {}", resp.status());
        let request_id = resp.headers().get("inference-id").context("no inference-id header")?.to_str()?.to_owned();
        let unattested = resp.headers().get("x-serving-provider").and_then(|v| v.to_str().ok()) == Some("non-attested");
        let bytes = resp.bytes().await?;
        let body: Value = serde_json::from_slice(&bytes)?;
        let chat_id = body["id"].as_str().context("completion id")?.to_owned();
        let msg = &body["choices"][0]["message"];
        let request_hash = hex::encode(Sha256::digest(&req));
        let response_hash = hex::encode(Sha256::digest(&bytes));

        let sig = self.signature(&chat_id).await?;
        let text = sig["text"].as_str().context("text")?;
        let (kind, label, signer, expected_text) = match sig["signature_kind"].as_str() {
            Some("provider_tee") => ("ProviderTee", "model-verified", self.model_signer.get_or_try_init(|| self.report_signer(Some(&self.model))).await?, format!("{}:{request_hash}:{response_hash}", self.model)),
            Some("gateway") => (
                "Gateway",
                if unattested { "gateway-verified, provider not attested" } else { "gateway-verified" },
                self.gateway_signer.get_or_try_init(|| self.report_signer(None)).await?,
                format!("{request_hash}:{response_hash}"),
            ),
            other => anyhow::bail!("unknown signature kind {other:?}"),
        };
        ensure!(text == expected_text, "signature does not cover our exact bytes");
        let raw = hex::decode(sig["signature"].as_str().context("signature")?.trim_start_matches("0x"))?;
        ensure!(Signature::from_raw(&raw)?.recover_address_from_msg(text.as_bytes())? == *signer, "signer is not the attested key");

        Ok(Thought {
            content: msg["content"].as_str().unwrap_or("").to_owned(),
            reasoning: msg["reasoning_content"].as_str().or_else(|| msg["reasoning"].as_str()).unwrap_or("").to_owned(),
            cost_micro_usd: self.cost(&request_id).await?,
            chat_id,
            request_id,
            model: self.model.clone(),
            request_hash,
            response_hash,
            signature: sig["signature"].as_str().unwrap_or_default().to_owned(),
            kind,
            label,
            request_body: String::from_utf8(req)?,
            response_body: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }

    async fn signature(&self, chat_id: &str) -> Result<Value> {
        for _ in 0..10 {
            let r = self.http.get(format!("{API}/v1/signature/{chat_id}")).query(&[("signing_algo", "ecdsa")]).bearer_auth(&self.key).send().await?;
            if r.status().is_success() {
                let v: Value = r.json().await?;
                if v.get("error_code").is_none() {
                    return Ok(v);
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        anyhow::bail!("signature never became available")
    }

    // Billing is in nano-USD; the contract works in micro-USD, rounded up so the protocol is never short.
    async fn cost(&self, request_id: &str) -> Result<u64> {
        for _ in 0..10 {
            let v: Value = self.http.post(format!("{API}/v1/billing/costs")).bearer_auth(&self.key).json(&json!({"requestIds": [request_id]})).send().await?.json().await?;
            let nano = v["requests"][0]["costNanoUsd"].as_u64().unwrap_or(0);
            if nano > 0 {
                return Ok(nano.div_ceil(1000));
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        anyhow::bail!("cost never became visible")
    }
}
