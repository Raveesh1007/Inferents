use crate::agent::{Runner, num};
use crate::ai::Ai;
use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Serialize, Clone)]
pub struct Row {
    pub token_id: String,
    pub owner: String,
    pub model: String,
    pub receipt_kind: &'static str,
    pub strategy_hash: String,
    pub hl_address: String,
    pub equity_usd: f64,
    pub realized_pnl_usd: f64,
    pub pnl_usd: f64,
    pub thinking_spent_usd: f64,
    // P&L divided by what the agent spent on thinking; none until it has spent something.
    pub return_on_thought: Option<f64>,
    pub trades: u32,
    pub credits_usd: f64,
    pub price_usd: f64,
    pub acquired_by: Option<String>,
    pub epoch_end_ns: String,
    pub positions: Vec<Value>,
}

pub fn kind_label(model: &str) -> &'static str {
    if Ai::is_gateway_model(model) { "gateway-verified, provider not attested" } else { "model-verified" }
}

fn usd(v: &Value) -> f64 {
    num(v) as f64 / 1e6
}

impl Runner {
    async fn trade_counts(&self) -> HashMap<String, u32> {
        let mut counts = HashMap::new();
        let mut from = 0u64;
        while let Ok(page) = self.near.view(&self.contract, "receipts", json!({"from_index": from, "limit": 100})).await {
            let list = page.as_array().cloned().unwrap_or_default();
            for r in &list {
                *counts.entry(r["token_id"].as_str().unwrap_or("").to_owned()).or_insert(0) += 1;
            }
            if list.len() < 100 {
                break;
            }
            from += 100;
        }
        counts
    }

    async fn row(&self, a: &Value, counts: &HashMap<String, u32>, premium: f64) -> Row {
        let id = a["token_id"].as_str().unwrap_or("").to_owned();
        let address = a["hl_address"].as_str().unwrap_or("").to_owned();
        let state = if address.is_empty() { None } else { self.hl.perp_state(&address).await.ok() };
        let unrealized: f64 = state.as_ref().map_or(0.0, |s| s.positions.iter().map(|p| p.unrealized).sum());
        let realized = a["pnl_micro_usd"].as_str().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) as f64 / 1e6;
        let equity = state.as_ref().map_or_else(|| usd(&a["equity_micro_usd"]), |s| s.account_value);
        // The contract's formula on the live account value: what a buyer is quoted once the runner reports it.
        let price = equity + usd(&a["credits_micro_usd"]) + usd(&a["margin_micro_usd"]) + premium * realized.max(0.0);
        let spent = usd(&a["spent_micro_usd"]);
        let model = a["model"].as_str().unwrap_or("").to_owned();
        Row {
            owner: a["owner"].as_str().unwrap_or("").to_owned(),
            receipt_kind: kind_label(&model),
            strategy_hash: a["strategy_hash"].as_str().unwrap_or("").to_owned(),
            equity_usd: equity,
            realized_pnl_usd: realized,
            pnl_usd: realized + unrealized,
            thinking_spent_usd: spent,
            return_on_thought: (spent > 0.0).then(|| (realized + unrealized) / spent),
            trades: counts.get(&id).copied().unwrap_or(0),
            credits_usd: usd(&a["credits_micro_usd"]),
            price_usd: price,
            acquired_by: a["acquired_by"].as_str().map(String::from),
            epoch_end_ns: a["epoch_end_ns"].as_str().unwrap_or("").to_owned(),
            positions: state.map_or(vec![], |s| s.positions.iter().map(|p| json!({"coin": p.coin, "size": p.size, "entry_px": p.entry_px, "unrealized": p.unrealized})).collect()),
            hl_address: address,
            model,
            token_id: id,
        }
    }

    pub async fn leaderboard(&self) -> Result<Vec<Row>> {
        let (agents, counts, bps) = futures::join!(
            self.arena_agents(),
            self.trade_counts(),
            self.near.view(&self.contract, "premium_bps", json!({})),
        );
        let premium = bps.ok().and_then(|v| v.as_f64()).unwrap_or(2000.0) / 10_000.0;
        let agents = agents?;
        let mut rows = futures::future::join_all(agents.iter().map(|a| self.row(a, &counts, premium))).await;
        rows.sort_by(|a, b| b.pnl_usd.total_cmp(&a.pnl_usd));
        Ok(rows)
    }

    // The latest decisions across the arena, newest first. Only the public outcome is exposed (what was traded, held or
    // bought, which is visible on Hyperliquid anyway), never the reasoning or the strategy.
    pub async fn activity(&self, limit: usize) -> Result<Vec<Value>> {
        let mut out = vec![];
        for a in self.arena_agents().await? {
            let id = a["token_id"].as_str().unwrap_or("").to_owned();
            for rid in self.vault.list(&id).unwrap_or_default().iter().rev().take(12) {
                let Ok(e) = self.vault.load(&id, rid) else { continue };
                out.push(json!({"token_id": id, "model": a["model"], "summary": e["summary"], "label": e["receipt_label"],
                    "at_ns": e["at_ns"].as_str().unwrap_or("0"), "receipt": e["order_id"].is_string()}));
            }
        }
        out.sort_by_key(|e| std::cmp::Reverse(e["at_ns"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0)));
        out.truncate(limit);
        Ok(out)
    }

    // Each agent's P&L over the past week, from Hyperliquid's public portfolio history.
    pub async fn history(&self) -> Result<Vec<Value>> {
        let agents = self.arena_agents().await?;
        Ok(futures::future::join_all(agents.iter().filter(|a| a["hl_address"].as_str().is_some_and(|s| !s.is_empty())).map(|a| async move {
            let p = self.hl.info(json!({"type": "portfolio", "user": a["hl_address"]})).await.unwrap_or_default();
            let week = p.as_array().and_then(|l| l.iter().find(|x| x[0] == "perpWeek")).map(|x| x[1]["pnlHistory"].clone()).unwrap_or(json!([]));
            let points: Vec<Value> = week.as_array().cloned().unwrap_or_default().iter()
                .map(|pt| json!([pt[0], pt[1].as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0)])).collect();
            json!({"token_id": a["token_id"], "model": a["model"], "points": points})
        }))
        .await)
    }

    // What an agent sees of the others, from public data; it never includes their strategy or reasoning.
    pub async fn leaderboard_snapshot(&self, for_token: &str) -> Result<String> {
        let rows = self.leaderboard().await?;
        let mut s = String::from("Arena leaderboard (other agents, best P&L first):\n");
        for r in rows.iter().filter(|r| r.token_id != for_token).take(3) {
            let pos = if r.positions.is_empty() { "flat".to_owned() } else { r.positions.iter().map(|p| format!("{} {}", p["coin"].as_str().unwrap_or("?"), if p["size"].as_f64().unwrap_or(0.0) > 0.0 { "long" } else { "short" })).collect::<Vec<_>>().join(", ") };
            s += &format!("#{} ({}): P&L ${:.2}, equity ${:.2}, positions: {pos}\n", r.token_id, r.model, r.pnl_usd, r.equity_usd);
        }
        Ok(s)
    }
}
