use crate::hl::{AgentCtx, Hyperliquid, Venue};
use crate::near::Near;
use anyhow::{Context, Result, bail};
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{BaseUrl, ExchangeClient, ExchangeResponseStatus};
use serde_json::{Value, json};
use std::time::Duration;

const ONE_CLICK: &str = "https://1click.chaindefuser.com/v0";
const NEAR_USDC: &str = "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
const HYPERCORE_USDC: &str = "1cs_v1:hypercore:hip1:0x6d1e7cde53ba9467b783cb7c530ce054";
// 1Click rejects swaps under about $1.31; below this the credits stay put and roll into the next epoch.
pub const MIN_SWAP_MICRO_USD: u64 = 1_400_000;

pub enum Conversion {
    // Testnet: Intents is mainnet-only, so a funded testnet wallet pays the agent's Hyperliquid account instead.
    Simulated { deposit_account: String, funder: LocalWallet },
    Live { refund_to: String, jwt: Option<String> },
}

pub fn usdc(amount_micro: u64) -> String {
    let s = format!("{:.6}", amount_micro as f64 / 1e6);
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl Conversion {
    pub fn label(&self) -> &'static str {
        match self {
            Conversion::Simulated { .. } => "SIMULATED (testnet funder stands in for the Intents payout)",
            Conversion::Live { .. } => "LIVE (NEAR Intents)",
        }
    }

    // method is "release_margin" (epoch end: credits + margin) or "fund_margin" (deposit split: margin only, any time).
    pub async fn payout(&self, near: &Near, contract: &str, method: &str, hl: &Hyperliquid, ctx: &AgentCtx, amount_micro: u64) -> Result<String> {
        let http = reqwest::Client::new();
        match self {
            Conversion::Simulated { deposit_account, funder } => {
                near.call(contract, method, json!({"token_id": ctx.token_id, "deposit_address": deposit_account}), 100, 0).await?;
                funder_pay(funder, hl, &ctx.address, amount_micro).await?;
                hl.deposit(ctx, &usdc(amount_micro)).await?;
                Ok(format!("released {} credits; funder paid the agent's Hyperliquid account (simulated); moved to perps", usdc(amount_micro)))
            }
            Conversion::Live { refund_to, jwt } => {
                if amount_micro < MIN_SWAP_MICRO_USD {
                    return Ok(format!("{} is below the Intents minimum, credits carry over", usdc(amount_micro)));
                }
                let deadline = (std::time::SystemTime::now() + Duration::from_secs(3600)).duration_since(std::time::UNIX_EPOCH)?.as_secs();
                let body = json!({"dry": false, "swapType": "EXACT_INPUT", "slippageTolerance": 100, "originAsset": NEAR_USDC, "depositType": "ORIGIN_CHAIN",
                    "destinationAsset": HYPERCORE_USDC, "amount": amount_micro.to_string(), "refundTo": refund_to, "refundType": "ORIGIN_CHAIN",
                    "recipient": ctx.address, "recipientType": "DESTINATION_CHAIN", "deadline": iso(deadline)});
                let mut req = http.post(format!("{ONE_CLICK}/quote")).json(&body);
                if let Some(j) = jwt {
                    req = req.bearer_auth(j);
                }
                let q: Value = req.send().await?.json().await?;
                let deposit = q["quote"]["depositAddress"].as_str().with_context(|| format!("no quote: {q}"))?.to_owned();
                near.call(contract, method, json!({"token_id": ctx.token_id, "deposit_address": deposit}), 100, 0).await?;
                for _ in 0..60 {
                    let s: Value = http.get(format!("{ONE_CLICK}/status")).query(&[("depositAddress", deposit.as_str())]).send().await?.json().await?;
                    match s["status"].as_str() {
                        Some("SUCCESS") => return Ok(format!("Intents swap done, {} out", q["quote"]["amountOutFormatted"])),
                        Some("REFUNDED") | Some("FAILED") => bail!("Intents swap ended as {}", s["status"]),
                        _ => tokio::time::sleep(Duration::from_secs(10)).await,
                    }
                }
                bail!("Intents swap still pending after 10 minutes")
            }
        }
    }
}

// Testnet stand-in for the Intents payout: the funder sends USDC to the agent's Hyperliquid spot balance.
pub async fn funder_pay(funder: &LocalWallet, hl: &Hyperliquid, address: &str, amount_micro: u64) -> Result<()> {
    let meta = hl.info(json!({"type": "spotMeta"})).await?;
    let tok = meta["tokens"].as_array().and_then(|t| t.iter().find(|x| x["name"] == "USDC")).context("USDC spot token")?;
    let token = format!("USDC:{}", tok["tokenId"].as_str().context("tokenId")?);
    let ex = ExchangeClient::new(None, funder.clone(), Some(BaseUrl::Testnet), None, None).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let r = ex.spot_transfer(&usdc(amount_micro), address, &token, None).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    if let ExchangeResponseStatus::Err(e) = r {
        bail!("funder transfer failed: {e}");
    }
    wait_spot(hl, address, amount_micro).await
}

async fn wait_spot(hl: &Hyperliquid, address: &str, at_least_micro: u64) -> Result<()> {
    for _ in 0..30 {
        if hl.spot_usdc(address).await? * 1e6 + 1.0 >= at_least_micro as f64 {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    bail!("payout never appeared in the agent's spot balance")
}

fn iso(unix: u64) -> String {
    let days = (unix / 86400) as i64;
    let (y, m, d) = civil(days);
    let s = unix % 86400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", s / 3600, s % 3600 / 60, s % 60)
}

// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usdc_amounts_are_trimmed_decimals() {
        assert_eq!(usdc(4_939_700), "4.9397");
        assert_eq!(usdc(5_000_000), "5");
        assert_eq!(usdc(1_400_000), "1.4");
    }

    #[test]
    fn iso_formats_unix_time() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_794_400_000), "2026-11-11T12:26:40.000Z");
    }
}
