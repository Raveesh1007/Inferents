use crate::agent::Runner;
use crate::arena::{Row, kind_label};
use crate::nep413::{self, Signed};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

type Reply = Result<Json<Value>, (StatusCode, String)>;

struct App {
    runner: Arc<Runner>,
    cache: Mutex<Option<(Instant, Vec<Row>)>>,
    refreshed: Mutex<std::collections::HashMap<String, Instant>>,
    history: Mutex<Option<(Instant, Value)>>,
}

fn err<E: std::fmt::Display>(code: StatusCode) -> impl Fn(E) -> (StatusCode, String) {
    move |e| (code, format!("{e:#}"))
}

fn unauth<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::UNAUTHORIZED, format!("{e:#}"))
}

impl App {
    // Leaderboard reads Hyperliquid and NEAR for every agent, so serve a result at most 15 s old.
    async fn rows(&self) -> Result<Vec<Row>, (StatusCode, String)> {
        let mut cache = self.cache.lock().await;
        if let Some((at, rows)) = cache.as_ref() {
            if at.elapsed() < Duration::from_secs(15) {
                return Ok(rows.clone());
            }
        }
        let rows = self.runner.leaderboard().await.map_err(err(StatusCode::BAD_GATEWAY))?;
        *cache = Some((Instant::now(), rows.clone()));
        Ok(rows)
    }
}

async fn agents(State(app): State<Arc<App>>) -> Reply {
    let mut rows = app.rows().await?;
    rows.sort_by_key(|r| r.token_id.parse::<u64>().unwrap_or(0));
    Ok(Json(json!(rows)))
}

async fn leaderboard(State(app): State<Arc<App>>) -> Reply {
    Ok(Json(json!(app.rows().await?)))
}

async fn agent(State(app): State<Arc<App>>, Path(id): Path<String>) -> Reply {
    let row = app.rows().await?.into_iter().find(|r| r.token_id == id).ok_or((StatusCode::NOT_FOUND, "no such agent".into()))?;
    Ok(Json(json!(row)))
}

// On-chain receipts for one agent, each with the label the frontend shows.
async fn receipts(State(app): State<Arc<App>>, Path(id): Path<String>) -> Reply {
    let (near, contract) = (&app.runner.near, &app.runner.contract);
    let mut out = vec![];
    let mut from = 0u64;
    loop {
        let page = near.view(contract, "receipts", json!({"from_index": from, "limit": 100})).await.map_err(err(StatusCode::BAD_GATEWAY))?;
        let list = page.as_array().cloned().unwrap_or_default();
        for (i, mut r) in list.iter().cloned().enumerate() {
            if r["token_id"] == id.as_str() {
                r["index"] = json!(from + i as u64);
                r["label"] = json!(match r["kind"].as_str() {
                    Some("Gateway") => "gateway-verified, provider not attested",
                    Some(_) => "model-verified",
                    None => kind_label(r["model"].as_str().unwrap_or("")),
                });
                out.push(r);
            }
        }
        if list.len() < 100 {
            break;
        }
        from += 100;
    }
    Ok(Json(json!(out)))
}

// Reasoning goes only to the NFT owner or a viewing-key holder, proven by a NEP-413 signature from one of their account keys.
async fn reasoning(State(app): State<Arc<App>>, headers: HeaderMap, Path((token, receipt)): Path<(String, String)>) -> Reply {
    let raw = headers.get("x-nep413").and_then(|v| v.to_str().ok()).ok_or((StatusCode::UNAUTHORIZED, "send the NEP-413 signature in x-nep413 (base64 JSON)".into()))?;
    let signed: Signed = serde_json::from_slice(&B64.decode(raw).map_err(unauth)?).map_err(unauth)?;
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    nep413::verify(&signed, now_ms).map_err(unauth)?;
    let (t, r) = nep413::requested(&signed).map_err(unauth)?;
    if t != token || r != receipt {
        return Err((StatusCode::UNAUTHORIZED, "the signed message is for a different receipt".into()));
    }
    let r = &app.runner;
    if !r.near.has_access_key(&signed.account_id, &signed.public_key).await {
        return Err((StatusCode::UNAUTHORIZED, "that key does not belong to the account".into()));
    }
    let allowed = r.near.view(&r.contract, "has_access", json!({"token_id": token, "account": signed.account_id})).await.map_err(err(StatusCode::BAD_GATEWAY))?;
    if allowed != true {
        return Err((StatusCode::FORBIDDEN, "no viewing key and not the owner".into()));
    }
    let mut entry = r.vault.load(&token, &receipt).map_err(err(StatusCode::NOT_FOUND))?;
    let owner = r.near.view(&r.contract, "agent", json!({"token_id": token})).await.map_err(err(StatusCode::BAD_GATEWAY))?["owner"].clone();
    if owner == signed.account_id.as_str() {
        entry["strategy"] = json!(r.vault.load_strategy(&token).ok());
    } else if let Some(body) = entry["request_body"].as_str() {
        entry["request_body"] = json!(crate::strategy::redact_request(body));
        entry["strategy_redacted"] = json!(true);
    }
    Ok(Json(entry))
}

// Reports the agent's equity on chain so a buy can follow within the contract's two-minute window; returns the quoted price.
async fn refresh(State(app): State<Arc<App>>, Path(id): Path<String>) -> Reply {
    let r = &app.runner;
    // Anyone can call this and it sends a transaction, so one report per agent per 30 seconds is enough for a buy.
    let mut refreshed = app.refreshed.lock().await;
    if refreshed.get(&id).is_none_or(|at| at.elapsed() > Duration::from_secs(30)) {
        r.refresh_equity(&id).await.map_err(err(StatusCode::BAD_GATEWAY))?;
        refreshed.insert(id.clone(), Instant::now());
    }
    drop(refreshed);
    let price = r.near.view(&r.contract, "price", json!({"token_id": id})).await.map_err(err(StatusCode::BAD_GATEWAY))?;
    Ok(Json(json!({"token_id": id, "price_micro_usd": price, "valid_for_secs": 120})))
}

// Whether the runner already holds this agent's strategy. Says nothing about the text itself.
async fn strategy_status(State(app): State<Arc<App>>, Path(token): Path<String>) -> Reply {
    Ok(Json(json!({"token_id": token, "stored": app.runner.vault.load_strategy(&token).is_ok()})))
}

// After minting, the strategy text is handed to the runner. No signature is needed: the contract already holds the
// text's SHA-256, so only the exact committed text is accepted, and whoever can send it already knows it.
async fn upload_strategy(State(app): State<Arc<App>>, Path(token): Path<String>, body: String) -> Reply {
    let r = &app.runner;
    let a = r.near.view(&r.contract, "agent", json!({"token_id": token})).await.map_err(err(StatusCode::BAD_GATEWAY))?;
    let committed = a["strategy_hash"].as_str().ok_or((StatusCode::NOT_FOUND, "no such agent".to_string()))?;
    use sha2::Digest;
    if hex::encode(sha2::Sha256::digest(body.as_bytes())) != committed {
        return Err((StatusCode::BAD_REQUEST, "the text does not hash to the on-chain strategy_hash".into()));
    }
    r.vault.store_strategy(&token, &body).map_err(err(StatusCode::INTERNAL_SERVER_ERROR))?;
    Ok(Json(json!({"stored": true})))
}

async fn activity(State(app): State<Arc<App>>) -> Reply {
    Ok(Json(json!(app.runner.activity(40).await.map_err(err(StatusCode::BAD_GATEWAY))?)))
}

// Hyperliquid portfolio history changes slowly; one read a minute is plenty.
async fn history(State(app): State<Arc<App>>) -> Reply {
    let mut cache = app.history.lock().await;
    if let Some((at, v)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return Ok(Json(v.clone()));
        }
    }
    let v = json!(app.runner.history().await.map_err(err(StatusCode::BAD_GATEWAY))?);
    *cache = Some((Instant::now(), v.clone()));
    Ok(Json(v))
}

// The NFT's image. The contract's token metadata points `media` here, so wallets and marketplaces show the live card.
async fn nft_image(State(app): State<Arc<App>>, Path(file): Path<String>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let id = file.trim_end_matches(".svg");
    let row = app.rows().await?.into_iter().find(|r| r.token_id == id).ok_or((StatusCode::NOT_FOUND, "no such agent".into()))?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let updated = format!("{:02}:{:02} UTC", secs / 3600 % 24, secs / 60 % 60);
    Ok((
        [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=30")],
        crate::card::render(&row, &updated),
    ))
}

pub async fn serve(runner: Arc<Runner>, port: u16) -> anyhow::Result<()> {
    let app = Arc::new(App { runner, cache: Mutex::new(None), refreshed: Default::default(), history: Mutex::new(None) });
    let router = Router::new()
        .route("/agents", get(agents))
        .route("/agents/{id}", get(agent))
        .route("/leaderboard", get(leaderboard))
        .route("/activity", get(activity))
        .route("/nft/{file}", get(nft_image))
        .route("/history", get(history))
        .route("/receipts/{id}", get(receipts))
        .route("/reasoning/{token}/{receipt}", get(reasoning))
        .route("/agents/{id}/refresh", post(refresh))
        .route("/strategy/{token}", post(upload_strategy).get(strategy_status))
        .layer(CorsLayer::permissive())
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    println!("API listening on :{port}");
    axum::serve(listener, router).await?;
    Ok(())
}
