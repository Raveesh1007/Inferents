use anyhow::{Context, Result, bail};
use ethers::signers::{LocalWallet, Signer};
use hyperliquid_rust_sdk::{
    BaseUrl, ClientCancelRequest, ClientLimit, ClientOrder, ClientOrderRequest, ExchangeClient,
    ExchangeDataStatus, ExchangeResponseStatus, InfoClient,
};
use std::fs;

#[tokio::main]
async fn main() -> Result<()> {
    let token_id = std::env::args().nth(1).unwrap_or_else(|| "1".into());
    let key = fs::read_to_string(format!(".secrets/agent-{token_id}.key"))?;
    let agent: LocalWallet = key.trim().parse()?;
    println!("agent key address: {:?}", agent.address());

    let info = InfoClient::new(None, Some(BaseUrl::Testnet)).await?;
    let mid: f64 = info.all_mids().await?.get("BTC").context("no BTC mid")?.parse()?;
    // Resting bid 10% below mid so it never fills; whole-dollar price satisfies BTC's tick rules, size rounded up to 5 decimals for >= $12 notional.
    let px = (mid * 0.9).round();
    let sz = ((12.0 / px) * 1e5).ceil() / 1e5;
    println!("BTC mid {mid}, bid {px} x {sz} = ${:.2}", px * sz);

    let ex = ExchangeClient::new(None, agent, Some(BaseUrl::Testnet), None, None).await?;
    let order = ClientOrderRequest {
        asset: "BTC".into(),
        is_buy: true,
        reduce_only: false,
        limit_px: px,
        sz,
        cloid: None,
        order_type: ClientOrder::Limit(ClientLimit { tif: "Gtc".into() }),
    };
    let ExchangeResponseStatus::Ok(resp) = ex.order(order, None).await? else {
        bail!("order rejected by exchange");
    };
    let status = resp.data.context("no data")?.statuses.into_iter().next().context("no status")?;
    let oid = match status {
        ExchangeDataStatus::Resting(r) => r.oid,
        other => bail!("unexpected order status: {other:?}"),
    };
    println!("order resting, oid {oid}");

    let cancel = ex.cancel(ClientCancelRequest { asset: "BTC".into(), oid }, None).await?;
    println!("cancel: {cancel:?}");
    Ok(())
}
