use crate::near::Near;
use anyhow::{Context, Result, bail};
use ethers::signers::{LocalWallet, Signer};
use hyperliquid_rust_sdk::{BaseUrl, ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus, MarketCloseParams, MarketOrderParams};
use serde_json::{Value, json};
use std::sync::Arc;

pub const MIN_ORDER_USD: f64 = 10.0;

#[derive(Clone, Debug)]
pub struct Position {
    pub coin: String,
    pub size: f64,
    pub entry_px: f64,
    pub unrealized: f64,
}

#[derive(Debug)]
pub struct PerpState {
    pub account_value: f64,
    pub withdrawable: f64,
    pub positions: Vec<Position>,
}

pub struct AgentCtx {
    pub token_id: String,
    pub address: String,
    pub wallet: LocalWallet,
}

pub struct OrderFill {
    pub order_id: String,
    pub avg_px: f64,
    pub size: f64,
}

// Venue adapter: another exchange implements these five calls and the agent loop is unchanged.
#[allow(async_fn_in_trait)]
pub trait Venue {
    async fn positions(&self, ctx: &AgentCtx) -> Result<PerpState>;
    async fn place_order(&self, ctx: &AgentCtx, coin: &str, is_buy: bool, notional_usd: f64, leverage: u32) -> Result<OrderFill>;
    async fn close_all(&self, ctx: &AgentCtx) -> Result<Vec<OrderFill>>;
    async fn deposit(&self, ctx: &AgentCtx, amount_usdc: &str) -> Result<()>;
    async fn withdraw(&self, ctx: &AgentCtx, amount_usdc: &str, destination: &str) -> Result<()>;
}

pub struct Hyperliquid {
    http: reqwest::Client,
    base: &'static str,
    testnet: bool,
    near: Arc<Near>,
    contract: String,
}

fn f(v: &Value) -> f64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).unwrap_or(0.0)
}

impl Hyperliquid {
    pub fn new(testnet: bool, near: Arc<Near>, contract: &str) -> Self {
        let base = if testnet { "https://api.hyperliquid-testnet.xyz" } else { "https://api.hyperliquid.xyz" };
        Self { http: reqwest::Client::new(), base, testnet, near, contract: contract.into() }
    }

    pub async fn info(&self, body: Value) -> Result<Value> {
        Ok(self.http.post(format!("{}/info", self.base)).json(&body).send().await?.json().await?)
    }

    pub async fn mid(&self, coin: &str) -> Result<f64> {
        let mids = self.info(json!({"type": "allMids"})).await?;
        mids.get(coin).map(f).filter(|p| *p > 0.0).with_context(|| format!("no mid for {coin}"))
    }

    // Testnet oracles can lag the book badly; Hyperliquid rejects orders priced far from the oracle, so skip such coins.
    pub async fn tradable_coins(&self, coins: &[&str]) -> Result<Vec<String>> {
        let r = self.info(json!({"type": "metaAndAssetCtxs"})).await?;
        let universe = r[0]["universe"].as_array().context("universe")?;
        let ctxs = r[1].as_array().context("asset contexts")?;
        Ok(coins
            .iter()
            .filter(|c| {
                universe.iter().position(|u| u["name"] == **c).is_some_and(|i| {
                    let (mid, oracle) = (f(&ctxs[i]["midPx"]), f(&ctxs[i]["oraclePx"]));
                    oracle > 0.0 && mid > 0.0 && (mid - oracle).abs() / oracle < 0.03
                })
            })
            .map(|c| c.to_string())
            .collect())
    }

    pub async fn recent_closes(&self, coin: &str, hours: u64) -> Result<Vec<f64>> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64;
        let req = json!({"coin": coin, "interval": "1h", "startTime": now - (hours + 1) * 3_600_000, "endTime": now});
        let r = self.info(json!({"type": "candleSnapshot", "req": req})).await?;
        Ok(r.as_array().map_or(vec![], |a| a.iter().map(|c| f(&c["c"])).collect()))
    }

    pub async fn spot_usdc(&self, address: &str) -> Result<f64> {
        let s = self.info(json!({"type": "spotClearinghouseState", "user": address})).await?;
        Ok(s["balances"].as_array().and_then(|b| b.iter().find(|x| x["coin"] == "USDC")).map_or(0.0, |x| f(&x["total"])))
    }

    // Realised P&L from fills (net of fees) plus unrealised on open positions.
    pub async fn pnl(&self, address: &str) -> Result<f64> {
        let fills = self.info(json!({"type": "userFills", "user": address})).await?;
        let realised: f64 = fills.as_array().map_or(0.0, |a| a.iter().map(|x| f(&x["closedPnl"]) - f(&x["fee"])).sum());
        let unrealised: f64 = self.info(json!({"type": "clearinghouseState", "user": address})).await?["assetPositions"]
            .as_array()
            .map_or(0.0, |a| a.iter().map(|p| f(&p["position"]["unrealizedPnl"])).sum());
        Ok(realised + unrealised)
    }

    async fn asset_meta(&self, coin: &str) -> Result<(u32, u32)> {
        let m = self.info(json!({"type": "meta"})).await?;
        let a = m["universe"].as_array().and_then(|u| u.iter().find(|x| x["name"] == coin)).with_context(|| format!("unknown coin {coin}"))?;
        Ok((a["szDecimals"].as_u64().unwrap_or(0) as u32, a["maxLeverage"].as_u64().unwrap_or(1) as u32))
    }

    pub async fn submit_admin(&self, signed: &Value) -> Result<Value> {
        let body = json!({"action": signed["action"], "nonce": signed["nonce"], "signature": signed["signature"]});
        let r: Value = self.http.post(format!("{}/exchange", self.base)).json(&body).send().await?.json().await?;
        if r["status"] != "ok" {
            bail!("hyperliquid rejected admin action: {r}");
        }
        Ok(r)
    }

    async fn exchange(&self, wallet: &LocalWallet) -> Result<ExchangeClient> {
        let url = if self.testnet { BaseUrl::Testnet } else { BaseUrl::Mainnet };
        ExchangeClient::new(None, wallet.clone(), Some(url), None, None).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    // Approve a new trade-only agent key; the contract signs through the MPC and the unnamed approval replaces the old key.
    pub async fn rotate_agent_key(&self, token_id: &str, agent_address: &str) -> Result<()> {
        let signed = self.near.call(&self.contract, "approve_agent", json!({"token_id": token_id, "agent_address": agent_address}), 300, 0).await?;
        self.submit_admin(&signed).await.map(|_| ())
    }

    async fn contract_admin(&self, method: &str, args: Value) -> Result<()> {
        let signed = self.near.call(&self.contract, method, args, 300, 0).await?;
        self.submit_admin(&signed).await.map(|_| ())
    }
}

fn fill(r: ExchangeResponseStatus) -> Result<OrderFill> {
    let ExchangeResponseStatus::Ok(resp) = r else { bail!("exchange rejected the order") };
    match resp.data.context("no order data")?.statuses.into_iter().next().context("no order status")? {
        ExchangeDataStatus::Filled(o) => Ok(OrderFill { order_id: o.oid.to_string(), avg_px: o.avg_px.parse()?, size: o.total_sz.parse()? }),
        ExchangeDataStatus::Resting(o) => Ok(OrderFill { order_id: o.oid.to_string(), avg_px: 0.0, size: 0.0 }),
        other => bail!("order not filled: {other:?}"),
    }
}

impl Venue for Hyperliquid {
    async fn positions(&self, ctx: &AgentCtx) -> Result<PerpState> {
        let s = self.info(json!({"type": "clearinghouseState", "user": ctx.address})).await?;
        let positions = s["assetPositions"].as_array().map_or(vec![], |a| {
            a.iter()
                .map(|p| &p["position"])
                .filter(|p| f(&p["szi"]) != 0.0)
                .map(|p| Position { coin: p["coin"].as_str().unwrap_or("").into(), size: f(&p["szi"]), entry_px: f(&p["entryPx"]), unrealized: f(&p["unrealizedPnl"]) })
                .collect()
        });
        Ok(PerpState { account_value: f(&s["marginSummary"]["accountValue"]), withdrawable: f(&s["withdrawable"]), positions })
    }

    async fn place_order(&self, ctx: &AgentCtx, coin: &str, is_buy: bool, notional_usd: f64, leverage: u32) -> Result<OrderFill> {
        let (sz_decimals, max_lev) = self.asset_meta(coin).await?;
        let px = self.mid(coin).await?;
        let scale = 10f64.powi(sz_decimals as i32);
        let size = (notional_usd / px * scale).ceil() / scale;
        if size * px < MIN_ORDER_USD {
            bail!("order value ${:.2} is below the $10 minimum", size * px);
        }
        let ex = self.exchange(&ctx.wallet).await?;
        ex.update_leverage(leverage.min(max_lev).max(1), coin, true, None).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        let r = ex
            .market_open(MarketOrderParams { asset: coin, is_buy, sz: size, px: None, slippage: Some(0.05), cloid: None, wallet: None })
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        fill(r)
    }

    async fn close_all(&self, ctx: &AgentCtx) -> Result<Vec<OrderFill>> {
        let state = self.positions(ctx).await?;
        let ex = self.exchange(&ctx.wallet).await?;
        let mut out = vec![];
        for p in state.positions {
            let r = ex
                .market_close(MarketCloseParams { asset: &p.coin, sz: None, px: None, slippage: Some(0.05), cloid: None, wallet: None })
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            out.push(fill(r)?);
        }
        Ok(out)
    }

    // Moves USDC that arrived in the account's spot balance (where Intents lands it) into perps margin.
    async fn deposit(&self, ctx: &AgentCtx, amount_usdc: &str) -> Result<()> {
        self.contract_admin("usd_class_transfer", json!({"token_id": ctx.token_id, "amount": amount_usdc, "to_perp": true})).await
    }

    async fn withdraw(&self, ctx: &AgentCtx, amount_usdc: &str, destination: &str) -> Result<()> {
        self.contract_admin("withdraw", json!({"token_id": ctx.token_id, "amount": amount_usdc, "destination": destination})).await
    }
}

pub fn wallet_from_hex(hex: &str) -> Result<LocalWallet> {
    Ok(hex.trim().parse()?)
}

pub fn address_of(w: &LocalWallet) -> String {
    format!("{:?}", w.address())
}
