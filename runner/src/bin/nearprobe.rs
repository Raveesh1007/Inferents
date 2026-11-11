use anyhow::Result;
use inferents_runner::near::Near;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))?;
    let n = Near::from_credentials_file("https://rpc.testnet.near.org", "inferents-11111630.testnet", &format!("{home}/.near-credentials/testnet/inferents-11111630.testnet.json"))?;
    let c = "inferents-11111630.testnet";
    println!("before: {}", n.view(c, "agent", json!({"token_id": "1"})).await?["pnl_micro_usd"]);
    let out = n.call(c, "report_pnl", json!({"token_id": "1", "pnl_micro_usd": "-12345"}), 30, 0).await?;
    println!("report_pnl returned {out}");
    println!("after:  {}", n.view(c, "agent", json!({"token_id": "1"})).await?["pnl_micro_usd"]);
    Ok(())
}
