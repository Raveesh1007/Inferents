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
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct Runner {
    pub near: Arc<Near>,
    pub contract: String,
    pub ai_key: String,
    pub ais: Mutex<HashMap<String, Arc<Ai>>>,
    pub hl: Hyperliquid,
    pub vault: Vault,
    pub conversion: Conversion,
    pub keys_dir: PathBuf,
    pub flat_window_ns: u64,
    pub max_tokens: Option<u32>,
    // Agents left out of the arena (HIDDEN_AGENTS): not listed, not drawn, not run. They stay on chain.
    pub hidden: std::collections::HashSet<String>,
}

const MAX_COST_PER_REQUEST: u64 = 1_000_000;

fn now_ns() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

pub(crate) fn num(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()).unwrap_or(0)
}

impl Runner {
    fn ai_for(&self, model: &str) -> Arc<Ai> {
        let mut ais = self.ais.lock().unwrap();
        ais.entry(model.to_owned()).or_insert_with(|| Arc::new(Ai::new(self.ai_key.clone(), model.to_owned()))).clone()
    }

    // The strategy text is private; only its hash is on-chain, and the request bytes (stored with each receipt) include the text.
    fn strategy_for(&self, token_id: &str, on_chain_hash: &str) -> Result<Option<String>> {
        if on_chain_hash.is_empty() || on_chain_hash.bytes().all(|b| b == b'0') {
            return Ok(None);
        }
        let text = self.vault.load_strategy(token_id).context("strategy committed on-chain but not in the vault")?;
        anyhow::ensure!(hex::encode(Sha256::digest(text.as_bytes())) == on_chain_hash, "vault strategy does not match the on-chain strategy_hash");
        Ok(Some(text))
    }

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

    // Seconds since this agent's last completed pass, kept across restarts so a restart does not make every agent think (and pay) again.
    pub fn pass_age_secs(&self, token_id: &str) -> Option<u64> {
        let at: u64 = self.vault.load(token_id, "_pass").ok()?.as_str()?.parse().ok()?;
        Some(now_ns().saturating_sub(at) / 1_000_000_000)
    }

    pub fn mark_pass(&self, token_id: &str) {
        let _ = self.vault.store(token_id, "_pass", &json!(now_ns().to_string()));
    }

    pub async fn arena_agents(&self) -> Result<Vec<Value>> {
        let v = self.near.view(&self.contract, "agents", json!({"from_index": 0, "limit": 100})).await?;
        Ok(v.as_array().cloned().unwrap_or_default().into_iter().filter(|a| !self.hidden.contains(a["token_id"].as_str().unwrap_or(""))).collect())
    }

    pub async fn agent_ids(&self) -> Result<Vec<String>> {
        Ok(self.arena_agents().await?.iter().filter_map(|x| x["token_id"].as_str().map(String::from)).collect())
    }

    // One pass for one agent. Returns a one-line summary for the log.
    pub async fn tick(&self, token_id: &str) -> Result<String> {
        let a = self.near.view(&self.contract, "agent", json!({"token_id": token_id})).await?;
        let address = a["hl_address"].as_str().unwrap_or("").to_owned();
        if address.is_empty() {
            return Ok("waiting for the contract to derive the Hyperliquid address".into());
        }

        let (now, end) = (now_ns(), num(&a["epoch_end_ns"]));
        let mut wallet = self.load_wallet(token_id);
        // Hyperliquid refuses approveAgent for an account that has never been funded, so margin goes in before the key rotation.
        let margin = num(&a["margin_micro_usd"]);
        if margin > 0 && now < end {
            let w = match wallet.clone() {
                Some(w) => w,
                None => self.new_wallet(token_id)?,
            };
            let funding = AgentCtx { token_id: token_id.to_owned(), address: address.clone(), wallet: w };
            let msg = self.conversion.payout(&self.near, &self.contract, "fund_margin", &self.hl, &funding, margin).await?;
            println!("agent {token_id}: {msg}");
        }
        if let Some(moved) = self.hl.sweep_spot(token_id, &address).await? {
            println!("agent {token_id}: moved ${moved} from spot to perps");
        }
        let registered = a["agent_address"].as_str().map(str::to_lowercase);
        let mut stale = wallet.as_ref().map(|w| Some(address_of(w).to_lowercase()) != registered).unwrap_or(true);
        if let (false, Some(w)) = (stale, wallet.as_ref()) {
            stale = !self.hl.is_agent_approved(&address, &address_of(w)).await;
        }
        if a["needs_rotation"] == true || stale {
            let fresh = self.new_wallet(token_id)?;
            self.hl.rotate_agent_key(token_id, &address_of(&fresh)).await.context("agent key rotation")?;
            wallet = Some(fresh);
        }
        let ctx = AgentCtx { token_id: token_id.to_owned(), address, wallet: wallet.context("no agent key")? };

        let credits = num(&a["credits_micro_usd"]);
        let owed = credits + num(&a["margin_micro_usd"]);
        if now >= end {
            self.hl.close_all(&ctx).await?;
            if owed == 0 {
                return Ok("epoch over, nothing to convert".into());
            }
            let msg = self.conversion.payout(&self.near, &self.contract, "release_margin", &self.hl, &ctx, owed).await?;
            return Ok(msg);
        }
        let positions = self.hl.positions(&ctx).await?;
        let equity = (positions.account_value * 1e6).round() as u64;
        self.near.call(&self.contract, "report_equity", json!({"token_id": token_id, "equity_micro_usd": equity.to_string(), "timestamp_ns": now.to_string()}), 30, 0).await?;
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
        if num(&a["acquisition_budget_micro_usd"]) > 0 && self.vault.load(token_id, "_acq").ok().and_then(|v| v.as_str().map(String::from)).as_deref() != a["epoch_end_ns"].as_str() {
            let note = self.decide_acquisition(token_id, &a).await?;
            self.vault.store(token_id, "_acq", &json!(a["epoch_end_ns"]))?;
            return Ok(note);
        }

        let max_leverage = a["max_leverage"].as_u64().unwrap_or(1) as u32;
        let allowed = self.hl.tradable_coins(&COINS).await?;
        let model = a["model"].as_str().context("agent has no model")?;
        let strategy = self.strategy_for(token_id, a["strategy_hash"].as_str().unwrap_or(""))?;
        let mut prompt = self.market_prompt(&allowed, &positions.positions, positions.account_value, max_leverage).await?;
        if strategy.as_deref().is_some_and(|s| s.contains(strategy::LEADERBOARD_MARKER)) {
            prompt += &self.leaderboard_snapshot(token_id).await.unwrap_or_default();
        }
        let ai = self.ai_for(model);
        let thought = ai.think(&strategy::system_prompt(strategy.as_deref()), &prompt, self.max_tokens.unwrap_or_else(|| Ai::default_max_tokens(model))).await?;

        // The model call is paid for whether or not the trade happens, so it is charged first.
        let cost = thought.cost_micro_usd.min(MAX_COST_PER_REQUEST);
        self.near.call(&self.contract, "report_usage", json!({"token_id": token_id, "request_ids": [thought.request_id], "cost_micro_usd": cost.to_string()}), 30, 0).await?;

        let decision = strategy::parse_decision(&thought.content);
        let plan = decision.as_ref().map_or(Plan::Hold("model reply was not a valid decision".into()), |d| strategy::plan(d, positions.account_value, max_leverage, &positions.positions, &allowed));
        let executed: Result<(Option<String>, String)> = match &plan {
            Plan::Open { coin, is_buy, notional, leverage } => self.hl.place_order(&ctx, coin, *is_buy, *notional, *leverage).await.map(|f| {
                (Some(f.order_id.clone()), format!("{} {coin} ${notional:.2} at {leverage}x, oid {} @ {}", if *is_buy { "LONG" } else { "SHORT" }, f.order_id, f.avg_px))
            }),
            Plan::Close { coin } => self.hl.close_all(&ctx).await.map(|fills| (fills.first().map(|f| f.order_id.clone()), format!("CLOSE {coin} ({} fill(s))", fills.len()))),
            Plan::Hold(why) => Ok((None, format!("HOLD: {why}"))),
        };
        let (order_id, summary) = executed.unwrap_or_else(|e| (None, format!("ORDER FAILED, holding: {e:#}")));

        if let Some(oid) = &order_id {
            let receipt = json!({"receipt": {"token_id": token_id, "order_id": oid, "request_id": thought.request_id, "model": thought.model,
                "request_hash": thought.request_hash, "response_hash": thought.response_hash, "signature": thought.signature, "verified": false, "kind": thought.kind}});
            self.near.call(&self.contract, "submit_receipt", receipt, 30, 0).await?;
        }
        self.report_pnl(&ctx).await?;
        self.vault.store(
            token_id,
            &thought.request_id,
            &json!({"request_id": thought.request_id, "chat_id": thought.chat_id, "order_id": order_id, "receipt_label": thought.label, "request_body": thought.request_body, "response_body": thought.response_body, "summary": summary, "reason": decision.as_ref().map(|d| d.reason.clone()),
                "content": thought.content, "reasoning": thought.reasoning, "at_ns": now.to_string()}),
        )?;
        Ok(format!("{summary} | thinking cost {cost} micro-USD"))
    }

    // Once per epoch an agent with an acquisition budget is asked whether to buy another agent. The answer is paid for and
    // receipted like a trade; a "buy" spends the agent's credits through the contract, bounded by the owner's budget.
    async fn decide_acquisition(&self, token_id: &str, a: &Value) -> Result<String> {
        let budget = num(&a["acquisition_budget_micro_usd"]).min(num(&a["credits_micro_usd"]));
        let rows = self.leaderboard().await?;
        let owner = a["owner"].as_str().unwrap_or("");
        let candidates: Vec<_> = rows.iter().filter(|r| r.token_id != token_id && r.owner != owner && r.price_usd * 1e6 <= budget as f64).collect();
        if candidates.is_empty() {
            return Ok("acquisition: no agent within budget to consider".into());
        }
        let mut prompt = format!("Your acquisition budget: ${:.2}.\nCandidates:\n", budget as f64 / 1e6);
        for r in &candidates {
            prompt += &format!(
                "#{} ({}): price ${:.2}, P&L ${:.2}, thinking spent ${:.4}, return on thought {}, trades {}\n",
                r.token_id, r.model, r.price_usd, r.pnl_usd, r.thinking_spent_usd, r.return_on_thought.map_or("n/a".into(), |x| format!("{x:.1}")), r.trades
            );
        }
        let model = a["model"].as_str().context("agent has no model")?;
        let ai = self.ai_for(model);
        let thought = ai.think(strategy::ACQUISITION_PROMPT, &prompt, self.max_tokens.unwrap_or_else(|| Ai::default_max_tokens(model))).await?;
        let cost = thought.cost_micro_usd.min(MAX_COST_PER_REQUEST);
        self.near.call(&self.contract, "report_usage", json!({"token_id": token_id, "request_ids": [thought.request_id], "cost_micro_usd": cost.to_string()}), 30, 0).await?;

        let choice = strategy::parse_acquisition(&thought.content);
        let target = choice.as_ref().and_then(|c| c.target.clone()).filter(|t| candidates.iter().any(|r| &r.token_id == t));
        let summary = match &target {
            Some(t) => {
                let row = candidates.iter().find(|r| &r.token_id == t).context("candidate")?;
                self.near.call(&self.contract, "report_equity", json!({"token_id": t, "equity_micro_usd": ((row.equity_usd * 1e6).round() as u64).to_string(), "timestamp_ns": now_ns().to_string()}), 30, 0).await?;
                match self.near.call(&self.contract, "buy_with_credits", json!({"buyer_token_id": token_id, "target_token_id": t}), 60, 0).await {
                    Ok(_) => format!("ACQUIRED agent {t} for ${:.2}", row.price_usd),
                    Err(e) => format!("ACQUISITION FAILED for agent {t}: {e:#}"),
                }
            }
            None => "PASS on acquisitions".to_owned(),
        };
        let order_id = format!("acquire-{}", a["epoch_end_ns"].as_str().unwrap_or("0"));
        let receipt = json!({"receipt": {"token_id": token_id, "order_id": order_id, "request_id": thought.request_id, "model": thought.model,
            "request_hash": thought.request_hash, "response_hash": thought.response_hash, "signature": thought.signature, "verified": false, "kind": thought.kind}});
        self.near.call(&self.contract, "submit_receipt", receipt, 30, 0).await?;
        self.vault.store(
            token_id,
            &thought.request_id,
            &json!({"request_id": thought.request_id, "chat_id": thought.chat_id, "order_id": order_id, "receipt_label": thought.label, "request_body": thought.request_body, "response_body": thought.response_body,
                "summary": summary, "reason": choice.map(|c| c.reason), "content": thought.content, "reasoning": thought.reasoning, "at_ns": now_ns().to_string()}),
        )?;
        Ok(format!("{summary} | thinking cost {cost} micro-USD"))
    }

    // Called before a sale: the contract only quotes a price from an equity report under two minutes old.
    pub async fn refresh_equity(&self, token_id: &str) -> Result<()> {
        let a = self.near.view(&self.contract, "agent", json!({"token_id": token_id})).await?;
        let address = a["hl_address"].as_str().filter(|x| !x.is_empty()).context("agent has no Hyperliquid address yet")?;
        let equity = (self.hl.perp_state(address).await?.account_value * 1e6).round() as u64;
        self.near.call(&self.contract, "report_equity", json!({"token_id": token_id, "equity_micro_usd": equity.to_string(), "timestamp_ns": now_ns().to_string()}), 30, 0).await?;
        Ok(())
    }

    async fn report_pnl(&self, ctx: &AgentCtx) -> Result<()> {
        let pnl = (self.hl.pnl(&ctx.address).await? * 1e6).round() as i64;
        self.near.call(&self.contract, "report_pnl", json!({"token_id": ctx.token_id, "pnl_micro_usd": pnl.to_string()}), 30, 0).await?;
        Ok(())
    }

    async fn market_prompt(&self, allowed: &[String], positions: &[crate::hl::Position], margin: f64, max_leverage: u32) -> Result<String> {
        let mut s = String::from("Market (price now, then last hourly closes):\n");
        for coin in allowed {
            let closes = self.hl.recent_closes(coin, 8).await.unwrap_or_default();
            s += &format!("{coin}: {:.2}, closes {:?}\n", self.hl.mid(coin).await?, closes.iter().map(|c| (c * 100.0).round() / 100.0).collect::<Vec<_>>());
        }
        let funding = self.hl.funding_rates(allowed).await.unwrap_or_default();
        s += &format!("Funding rate per hour: {}\n", funding.iter().map(|(c, r)| format!("{c} {r:.7}")).collect::<Vec<_>>().join(", "));
        s += &format!("Your margin: ${margin:.2}. Max leverage you may use: {max_leverage}x.\n");
        if positions.is_empty() {
            s += "Open positions: none.\n";
        }
        for p in positions {
            s += &format!("Open position: {} size {} entry {} unrealised PnL ${:.2}\n", p.coin, p.size, p.entry_px, p.unrealized);
        }
        Ok(s)
    }

    // Testnet only: tops an agent's Hyperliquid margin up from the funder wallet.
    pub async fn fund(&self, token_id: &str, amount_micro: u64) -> Result<String> {
        let Conversion::Simulated { funder, .. } = &self.conversion else { anyhow::bail!("fund is a testnet helper; CONVERSION must be simulated") };
        let a = self.near.view(&self.contract, "agent", json!({"token_id": token_id})).await?;
        let address = a["hl_address"].as_str().filter(|x| !x.is_empty()).context("agent has no Hyperliquid address yet")?;
        crate::conversion::funder_pay(funder, &self.hl, address, amount_micro).await?;
        let signed = self.near.call(&self.contract, "usd_class_transfer", json!({"token_id": token_id, "amount": usdc(amount_micro), "to_perp": true}), 300, 0).await?;
        self.hl.submit_admin(&signed).await?;
        Ok(format!("agent {token_id} ({address}) funded with ${} margin", usdc(amount_micro)))
    }

    pub async fn owed_summary(&self, token_id: &str) -> Result<String> {
        let a = self.near.view(&self.contract, "agent", json!({"token_id": token_id})).await?;
        Ok(format!("credits ${}, margin ${}", usdc(num(&a["credits_micro_usd"])), usdc(num(&a["margin_micro_usd"]))))
    }
}
