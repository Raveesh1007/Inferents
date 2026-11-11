use crate::ai::Ai;
use crate::conversion::{Conversion, usdc};
use crate::hl::{AgentCtx, Hyperliquid, Venue, address_of};
use crate::near::Near;
use crate::strategy::{self, COINS, Plan};
use crate::vault::Vault;
use anyhow::{Context, Result};
use ethers::signers::LocalWallet;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;

pub struct Runner {
    pub near: Arc<Near>,
    pub contract: String,
    pub ai: Ai,
    pub hl: Hyperliquid,
    pub vault: Vault,
    pub conversion: Conversion,
    pub keys_dir: PathBuf,
    pub flat_window_ns: u64,
    pub max_tokens: u32,
}

const MAX_COST_PER_REQUEST: u64 = 1_000_000;

fn now_ns() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

fn num(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()).unwrap_or(0)
}

impl Runner {
    fn key_path(&self, token_id: &str) -> PathBuf {
        self.keys_dir.join(format!("agent-{token_id}.key"))
    }

    fn load_wallet(&self, token_id: &str) -> Option<LocalWallet> {
        std::fs::read_to_string(self.key_path(token_id)).ok()?.trim().parse().ok()
    }

    fn new_wallet(&self, token_id: &str) -> Result<LocalWallet> {
        let w = LocalWallet::new(&mut ethers::core::rand::thread_rng());
        std::fs::create_dir_all(&self.keys_dir)?;
        std::fs::write(self.key_path(token_id), hex::encode(w.signer().to_bytes()))?;
        Ok(w)
    }

    pub async fn agent_ids(&self) -> Result<Vec<String>> {
        let v = self.near.view(&self.contract, "agents", json!({"from_index": 0, "limit": 100})).await?;
        Ok(v.as_array().map_or(vec![], |a| a.iter().filter_map(|x| x["token_id"].as_str().map(String::from)).collect()))
    }

    // One pass for one agent. Returns a one-line summary for the log.
    pub async fn tick(&self, token_id: &str) -> Result<String> {
        let a = self.near.view(&self.contract, "agent", json!({"token_id": token_id})).await?;
        let address = a["hl_address"].as_str().unwrap_or("").to_owned();
        if address.is_empty() {
            return Ok("waiting for the contract to derive the Hyperliquid address".into());
        }

        let mut wallet = self.load_wallet(token_id);
        let registered = a["agent_address"].as_str().map(str::to_lowercase);
        let stale = wallet.as_ref().map(|w| Some(address_of(w).to_lowercase()) != registered).unwrap_or(true);
        if a["needs_rotation"] == true || stale {
            let fresh = self.new_wallet(token_id)?;
            self.hl.rotate_agent_key(token_id, &address_of(&fresh)).await.context("agent key rotation")?;
            wallet = Some(fresh);
        }
        let ctx = AgentCtx { token_id: token_id.to_owned(), address, wallet: wallet.context("no agent key")? };

        let (now, end) = (now_ns(), num(&a["epoch_end_ns"]));
        let credits = num(&a["credits_micro_usd"]);
        let owed = credits + num(&a["margin_micro_usd"]);
        if now >= end {
            self.hl.close_all(&ctx).await?;
            if owed == 0 {
                return Ok("epoch over, nothing to convert".into());
            }
            let msg = self.conversion.payout(&self.near, &self.contract, &self.hl, &ctx, owed).await?;
            return Ok(msg);
        }
        let positions = self.hl.positions(&ctx).await?;
        if a["flat_at_epoch_end"] == true && end - now <= self.flat_window_ns {
            if !positions.positions.is_empty() {
                let fills = self.hl.close_all(&ctx).await?;
                self.report_pnl(&ctx).await?;
                return Ok(format!("flattened {} position(s) before epoch end", fills.len()));
            }
            return Ok("flat, waiting for epoch end".into());
        }
        if credits == 0 {
            return Ok("no credits left, not thinking".into());
        }

        let max_leverage = a["max_leverage"].as_u64().unwrap_or(1) as u32;
        let prompt = self.market_prompt(&positions.positions, positions.account_value, max_leverage).await?;
        let thought = self.ai.think(strategy::SYSTEM_PROMPT, &prompt, self.max_tokens).await?;
        let decision = strategy::parse_decision(&thought.content);
        let plan = decision.as_ref().map_or(Plan::Hold("model reply was not a valid decision".into()), |d| strategy::plan(d, positions.account_value, max_leverage, &positions.positions));

        let mut order_id = None;
        let summary = match &plan {
            Plan::Open { coin, is_buy, notional, leverage } => {
                let f = self.hl.place_order(&ctx, coin, *is_buy, *notional, *leverage).await?;
                order_id = Some(f.order_id.clone());
                format!("{} {coin} ${notional:.2} at {leverage}x, oid {} @ {}", if *is_buy { "LONG" } else { "SHORT" }, f.order_id, f.avg_px)
            }
            Plan::Close { coin } => {
                let fills = self.hl.close_all(&ctx).await?;
                order_id = fills.first().map(|f| f.order_id.clone());
                format!("CLOSE {coin} ({} fill(s))", fills.len())
            }
            Plan::Hold(why) => format!("HOLD: {why}"),
        };

        let cost = thought.cost_micro_usd.min(MAX_COST_PER_REQUEST);
        self.near.call(&self.contract, "report_usage", json!({"token_id": token_id, "request_ids": [thought.request_id], "cost_micro_usd": cost.to_string()}), 30, 0).await?;
        if let Some(oid) = &order_id {
            let receipt = json!({"receipt": {"token_id": token_id, "order_id": oid, "request_id": thought.request_id, "model": thought.model,
                "request_hash": thought.request_hash, "response_hash": thought.response_hash, "signature": thought.signature, "verified": false}});
            self.near.call(&self.contract, "submit_receipt", receipt, 30, 0).await?;
        }
        self.report_pnl(&ctx).await?;
        self.vault.store(
            token_id,
            &thought.request_id,
            &json!({"request_id": thought.request_id, "chat_id": thought.chat_id, "order_id": order_id, "summary": summary, "reason": decision.as_ref().map(|d| d.reason.clone()),
                "content": thought.content, "reasoning": thought.reasoning, "at_ns": now.to_string()}),
        )?;
        Ok(format!("{summary} | thinking cost {} micro-USD", cost))
    }

    async fn report_pnl(&self, ctx: &AgentCtx) -> Result<()> {
        let pnl = (self.hl.pnl(&ctx.address).await? * 1e6).round() as i64;
        self.near.call(&self.contract, "report_pnl", json!({"token_id": ctx.token_id, "pnl_micro_usd": pnl.to_string()}), 30, 0).await?;
        Ok(())
    }

    async fn market_prompt(&self, positions: &[crate::hl::Position], margin: f64, max_leverage: u32) -> Result<String> {
        let mut s = String::from("Market (price now, then last hourly closes):\n");
        for coin in COINS {
            let closes = self.hl.recent_closes(coin, 8).await.unwrap_or_default();
            s += &format!("{coin}: {:.2}, closes {:?}\n", self.hl.mid(coin).await?, closes.iter().map(|c| (c * 100.0).round() / 100.0).collect::<Vec<_>>());
        }
        s += &format!("Your margin: ${margin:.2}. Max leverage you may use: {max_leverage}x.\n");
        if positions.is_empty() {
            s += "Open positions: none.\n";
        }
        for p in positions {
            s += &format!("Open position: {} size {} entry {} unrealised PnL ${:.2}\n", p.coin, p.size, p.entry_px, p.unrealized);
        }
        Ok(s)
    }

    pub async fn owed_summary(&self, token_id: &str) -> Result<String> {
        let a = self.near.view(&self.contract, "agent", json!({"token_id": token_id})).await?;
        Ok(format!("credits ${}, margin ${}", usdc(num(&a["credits_micro_usd"])), usdc(num(&a["margin_micro_usd"]))))
    }
}
