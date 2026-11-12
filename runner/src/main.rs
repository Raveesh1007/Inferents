use anyhow::{Context, Result, bail};
use ethers::signers::{LocalWallet, Signer};
use inferents_runner::agent::Runner;
use inferents_runner::conversion::Conversion;
use inferents_runner::hl::Hyperliquid;
use inferents_runner::near::Near;
use inferents_runner::vault::Vault;
use sha2::Digest;
use std::sync::Arc;
use std::time::Duration;

fn env(k: &str, default: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| default.to_owned())
}

fn funder_wallet(path: &str) -> Result<LocalWallet> {
    if let Ok(h) = std::fs::read_to_string(path) {
        return Ok(h.trim().parse()?);
    }
    let w = LocalWallet::new(&mut ethers::core::rand::thread_rng());
    std::fs::create_dir_all(".secrets")?;
    std::fs::write(path, hex::encode(w.signer().to_bytes()))?;
    Ok(w)
}

fn build() -> Result<Runner> {
    dotenvy::dotenv().ok();
    let account = env("NEAR_ACCOUNT_ID", "inferents-11111630.testnet");
    let contract = env("CONTRACT_ID", &account);
    let rpc = env("NEAR_RPC", "https://test.rpc.fastnear.com");
    let near = Arc::new(match std::env::var("NEAR_PRIVATE_KEY") {
        Ok(k) => Near::new(&rpc, &account, &k)?,
        Err(_) => {
            let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME"))?;
            Near::from_credentials_file(&rpc, &account, &env("NEAR_CREDENTIALS_FILE", &format!("{home}/.near-credentials/testnet/{account}.json")))?
        }
    });
    let testnet = env("HL_NETWORK", "testnet") == "testnet";
    let conversion = match env("CONVERSION", "simulated").as_str() {
        "simulated" => Conversion::Simulated { deposit_account: env("SIM_DEPOSIT_ACCOUNT", &format!("funder.{contract}")), funder: funder_wallet(".secrets/funder.key")? },
        "live" => Conversion::Live { refund_to: std::env::var("NEAR_REFUND_ACCOUNT").context("NEAR_REFUND_ACCOUNT")?, jwt: std::env::var("ONECLICK_JWT").ok() },
        other => bail!("CONVERSION must be simulated or live, got {other}"),
    };
    Ok(Runner {
        hl: Hyperliquid::new(testnet, near.clone(), &contract),
        near,
        contract,
        ai_key: std::env::var("NEAR_AI_API_KEY").context("NEAR_AI_API_KEY")?,
        ais: Default::default(),
        vault: Vault::open(&env("VAULT_DIR", ".secrets/vault"), ".secrets/vault.key")?,
        conversion,
        keys_dir: ".secrets".into(),
        flat_window_ns: env("FLAT_WINDOW_SECS", "15").parse::<u64>()? * 1_000_000_000,
        max_tokens: std::env::var("MAX_TOKENS").ok().map(|v| v.parse()).transpose()?,
        hidden: env("HIDDEN_AGENTS", "").split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("funder") => {
            let w = funder_wallet(".secrets/funder.key")?;
            println!("funder Hyperliquid address: {:?}", w.address());
        }
        Some("fund") => {
            let r = build()?;
            let id = args.get(2).context("usage: fund <token_id> <usdc>")?;
            let amount: f64 = args.get(3).context("usage: fund <token_id> <usdc>")?.parse()?;
            println!("{}", r.fund(id, (amount * 1e6).round() as u64).await?);
        }
        Some("show") => {
            let v = Vault::open(&env("VAULT_DIR", ".secrets/vault"), ".secrets/vault.key")?;
            let id = args.get(2).context("usage: show <token_id> [n]")?;
            let ids = v.list(id)?;
            let n: usize = args.get(3).map_or(1, |x| x.parse().unwrap_or(1));
            for rid in ids.iter().rev().take(n) {
                let e = v.load(id, rid)?;
                println!("request {rid}
  summary: {}
  reason:  {}
  reply:   {}", e["summary"], e["reason"], e["content"].as_str().unwrap_or("").chars().take(400).collect::<String>());
            }
        }
        Some("strategy-hash") => {
            let text = std::fs::read_to_string(args.get(2).context("usage: strategy-hash <file>")?)?;
            println!("{}", hex::encode(sha2::Sha256::digest(text.as_bytes())));
        }
        Some("set-strategy") => {
            let v = Vault::open(&env("VAULT_DIR", ".secrets/vault"), ".secrets/vault.key")?;
            let (id, file) = (args.get(2).context("usage: set-strategy <token_id> <file>")?, args.get(3).context("usage: set-strategy <token_id> <file>")?);
            let text = std::fs::read_to_string(file)?;
            v.store_strategy(id, &text)?;
            println!("stored; strategy_hash {}", hex::encode(sha2::Sha256::digest(text.as_bytes())));
        }
        Some("tick") => {
            let r = build()?;
            println!("conversion mode: {}", r.conversion.label());
            let id = args.get(2).context("usage: tick <token_id>")?;
            println!("agent {id}: {}", r.tick(id).await?);
        }
        Some("api") => {
            // The read API on its own, with no agent loop: nothing thinks, nothing is charged.
            let r = Arc::new(build()?);
            inferents_runner::server::serve(r, env("API_PORT", "8787").parse()?).await?;
        }
        Some("serve") => {
            let r = Arc::new(build()?);
            let port: u16 = env("API_PORT", "8787").parse()?;
            tokio::spawn({
                let r = r.clone();
                async move {
                    if let Err(e) = inferents_runner::server::serve(r, port).await {
                        eprintln!("API stopped: {e:#}");
                    }
                }
            });
            // Each pass can pay for a model call, so the default is slow. Agents keep their own schedule: one that
            // passed recently is not due again after a restart, and a failed pass is retried after five minutes.
            let every = Duration::from_secs(env("LOOP_SECS", "1800").parse()?);
            println!("runner started, conversion mode: {}, each agent every {every:?}", r.conversion.label());
            let mut retry: std::collections::HashMap<String, std::time::Instant> = Default::default();
            loop {
                for id in r.agent_ids().await.unwrap_or_default() {
                    let rested = r.pass_age_secs(&id).is_none_or(|age| age >= every.as_secs());
                    if !rested || retry.get(&id).is_some_and(|at| *at > std::time::Instant::now()) {
                        continue;
                    }
                    match r.tick(&id).await {
                        Ok(s) => {
                            r.mark_pass(&id);
                            println!("agent {id}: {s}");
                        }
                        Err(e) => {
                            retry.insert(id.clone(), std::time::Instant::now() + every.min(Duration::from_secs(300)));
                            eprintln!("agent {id}: error: {e:#}");
                        }
                    }
                }
                tokio::time::sleep(every.min(Duration::from_secs(60))).await;
            }
        }
        _ => bail!("usage: inferents-runner serve | api | tick <token_id> | show <token_id> [n] | set-strategy <token_id> <file> | strategy-hash <file> | fund <token_id> <usdc> | funder"),
    }
    Ok(())
}
