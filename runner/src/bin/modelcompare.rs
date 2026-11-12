use anyhow::{Context, Result};
use inferents_runner::ai::Ai;
use inferents_runner::strategy::{self, COINS, Plan};
use serde_json::{Value, json};
use std::time::Instant;

const MODELS: [&str; 11] = [
    "deepseek-ai/DeepSeek-V4.1-Flash-1017",
    "deepseek-ai/DeepSeek-V4.1-Flash-1017-abliterated",
    "Qwen/Qwen3.8-27B-abliterated",
    "deepseek/deepseek-v3.2",
    "moonshotai/kimi-k2.6",
    "moonshotai/kimi-k3",
    "Qwen/Qwen3.6-35B-A3B-FP8",
    "Qwen/Qwen3.8-27B",
    "Qwen/Qwen3-VL-30B-A3B-Instruct",
    "z-ai/glm-5.3",
    "z-ai/glm-5.3-flash",
];

async fn info(http: &reqwest::Client, body: Value) -> Result<Value> {
    Ok(http.post("https://api.hyperliquid-testnet.xyz/info").json(&body).send().await?.json().await?)
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let key = std::env::var("NEAR_AI_API_KEY").context("NEAR_AI_API_KEY")?;
    let http = reqwest::Client::new();

    let mids = info(&http, json!({"type": "allMids"})).await?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64;
    let mut prompt = String::from("Market (price now, then last hourly closes):\n");
    for coin in ["BTC", "ETH"] {
        let req = json!({"coin": coin, "interval": "1h", "startTime": now - 9 * 3_600_000, "endTime": now});
        let c = info(&http, json!({"type": "candleSnapshot", "req": req})).await?;
        let closes: Vec<f64> = c.as_array().map_or(vec![], |a| a.iter().filter_map(|x| x["c"].as_str()?.parse::<f64>().ok()).map(|v| (v * 100.0).round() / 100.0).collect());
        prompt += &format!("{coin}: {}, closes {closes:?}\n", mids[coin].as_str().unwrap_or("?"));
    }
    prompt += "Your margin: $50.00. Max leverage you may use: 3x.\nOpen positions: none.\n";
    println!("same prompt for every model:\n{prompt}");

    let allowed: Vec<String> = ["BTC", "ETH"].iter().map(|s| s.to_string()).collect();
    println!("{:<40} {:>7} {:>9}  {}", "model", "secs", "µUSD", "result");
    let picked: Vec<String> = std::env::args().skip(1).collect();
    let models: Vec<String> = if picked.is_empty() { MODELS.iter().map(|m| m.to_string()).collect() } else { picked };
    for model in models.iter().map(String::as_str) {
        let ai = Ai::new(key.clone(), model.to_owned());
        let t = Instant::now();
        let r = ai.think(strategy::SYSTEM_PROMPT, &prompt, Ai::default_max_tokens(model)).await;
        let secs = t.elapsed().as_secs_f32();
        match r {
            Ok(th) => {
                let verdict = match strategy::parse_decision(&th.content) {
                    None => format!("INVALID reply: {:?}", th.content.chars().take(60).collect::<String>()),
                    Some(d) => match strategy::plan(&d, 50.0, 3, &[], &allowed) {
                        Plan::Open { coin, is_buy, .. } => format!("{} {coin} ({})", if is_buy { "LONG" } else { "SHORT" }, d.reason.chars().take(70).collect::<String>()),
                        Plan::Close { coin } => format!("CLOSE {coin}"),
                        Plan::Hold(w) => format!("HOLD ({w}; {})", d.reason.chars().take(60).collect::<String>()),
                    },
                };
                println!("{model:<40} {secs:>7.1} {:>9}  {} | {verdict}", th.cost_micro_usd, th.label);
            }
            Err(e) => println!("{model:<40} {secs:>7.1} {:>9}  FAILED: {}", "-", format!("{e:#}").chars().take(110).collect::<String>()),
        }
    }
    let _ = COINS;
    Ok(())
}
