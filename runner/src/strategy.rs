use crate::hl::{MIN_ORDER_USD, Position};
use serde_json::Value;

pub const COINS: [&str; 3] = ["BTC", "ETH", "SOL"];

pub const SYSTEM_PROMPT: &str = "You are an autonomous perpetual-futures trading agent on Hyperliquid. \
Decide one action from the market data. Reply with a single JSON object and nothing else: \
{\"action\":\"open_long\"|\"open_short\"|\"close\"|\"hold\",\"coin\":\"BTC\"|\"ETH\"|\"SOL\",\"size_pct\":1-100,\"reason\":\"one sentence\"}. \
size_pct is the share of your allowed position size to use. Trade actively: with no open position, open one in the direction of the recent hourly trend; with an open position, close it if the trend has turned against it; hold only when closes are essentially flat.";

#[derive(Debug, PartialEq)]
pub enum Action {
    OpenLong,
    OpenShort,
    Close,
    Hold,
}

#[derive(Debug)]
pub struct Decision {
    pub action: Action,
    pub coin: String,
    pub size_pct: f64,
    pub reason: String,
}

#[derive(Debug, PartialEq)]
pub enum Plan {
    Open { coin: String, is_buy: bool, notional: f64, leverage: u32 },
    Close { coin: String },
    Hold(String),
}

// Reasoning models wrap the JSON in prose, so take the first balanced {...} that parses.
pub fn parse_decision(content: &str) -> Option<Decision> {
    let bytes = content.as_bytes();
    for (start, _) in content.match_indices('{') {
        let mut depth = 0;
        for (i, b) in bytes[start..].iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(v) = serde_json::from_str::<Value>(&content[start..=start + i]) {
                            if let Some(d) = from_json(&v) {
                                return Some(d);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn from_json(v: &Value) -> Option<Decision> {
    let action = match v["action"].as_str()? {
        "open_long" => Action::OpenLong,
        "open_short" => Action::OpenShort,
        "close" => Action::Close,
        "hold" => Action::Hold,
        _ => return None,
    };
    Some(Decision {
        action,
        coin: v["coin"].as_str().unwrap_or("").to_uppercase(),
        size_pct: v["size_pct"].as_f64().or_else(|| v["size_pct"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0),
        reason: v["reason"].as_str().unwrap_or("").to_owned(),
    })
}

// The risk budget: position size is bounded by the margin the agent actually has and the leverage cap its owner set.
pub fn plan(d: &Decision, margin_usd: f64, max_leverage: u32, positions: &[Position], allowed: &[String]) -> Plan {
    if d.action == Action::Hold {
        return Plan::Hold("model chose to hold".into());
    }
    if !allowed.iter().any(|c| *c == d.coin) {
        return Plan::Hold(format!("coin {} is not tradable", d.coin));
    }
    let has_position = positions.iter().any(|p| p.coin == d.coin);
    match d.action {
        Action::Close if has_position => Plan::Close { coin: d.coin.clone() },
        Action::Close => Plan::Hold(format!("no {} position to close", d.coin)),
        _ if !positions.is_empty() => Plan::Hold("a position is already open".into()),
        _ => {
            let capacity = margin_usd * max_leverage as f64 * 0.9;
            if capacity < MIN_ORDER_USD * 1.05 {
                return Plan::Hold(format!("margin ${margin_usd:.2} at {max_leverage}x cannot reach the $10 minimum order"));
            }
            let notional = (capacity * d.size_pct.clamp(1.0, 100.0) / 100.0).max(MIN_ORDER_USD * 1.05);
            Plan::Open { coin: d.coin.clone(), is_buy: d.action == Action::OpenLong, notional, leverage: max_leverage }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(coin: &str) -> Position {
        Position { coin: coin.into(), size: 0.1, entry_px: 1.0, unrealized: 0.0 }
    }

    fn all() -> Vec<String> {
        COINS.iter().map(|c| c.to_string()).collect()
    }

    fn d(action: Action, coin: &str, pct: f64) -> Decision {
        Decision { action, coin: coin.into(), size_pct: pct, reason: String::new() }
    }

    #[test]
    fn parses_json_buried_in_reasoning_prose() {
        let text = "Let me think {not json} about it.\n```json\n{\"action\":\"open_long\",\"coin\":\"btc\",\"size_pct\":\"50\",\"reason\":\"trend\"}\n```";
        let dec = parse_decision(text).unwrap();
        assert_eq!((dec.action, dec.coin.as_str(), dec.size_pct), (Action::OpenLong, "BTC", 50.0));
        assert!(parse_decision("no decision here").is_none());
        assert!(parse_decision("{\"action\":\"yolo\"}").is_none());
    }

    #[test]
    fn open_size_scales_with_margin_and_leverage_cap() {
        let Plan::Open { notional, leverage, is_buy, .. } = plan(&d(Action::OpenShort, "ETH", 100.0), 20.0, 3, &[], &all()) else { panic!() };
        assert_eq!((leverage, is_buy), (3, false));
        assert!((notional - 54.0).abs() < 1e-9);
        let Plan::Open { notional: half, .. } = plan(&d(Action::OpenLong, "ETH", 50.0), 20.0, 3, &[], &all()) else { panic!() };
        assert!((half - 27.0).abs() < 1e-9);
    }

    #[test]
    fn waits_until_margin_covers_the_minimum_order() {
        assert!(matches!(plan(&d(Action::OpenLong, "BTC", 100.0), 3.0, 3, &[], &all()), Plan::Hold(_)));
        assert!(matches!(plan(&d(Action::OpenLong, "BTC", 100.0), 4.0, 3, &[], &all()), Plan::Open { .. }));
    }

    #[test]
    fn coins_with_a_stale_oracle_are_not_tradable() {
        let only_btc = vec!["BTC".to_string()];
        assert!(matches!(plan(&d(Action::OpenLong, "SOL", 50.0), 100.0, 3, &[], &only_btc), Plan::Hold(_)));
        assert!(matches!(plan(&d(Action::OpenLong, "BTC", 50.0), 100.0, 3, &[], &only_btc), Plan::Open { .. }));
    }

    #[test]
    fn refuses_untradable_coins_pyramiding_and_phantom_closes() {
        assert!(matches!(plan(&d(Action::OpenLong, "DOGE", 10.0), 100.0, 3, &[], &all()), Plan::Hold(_)));
        assert!(matches!(plan(&d(Action::OpenLong, "BTC", 10.0), 100.0, 3, &[pos("ETH")], &all()), Plan::Hold(_)));
        assert!(matches!(plan(&d(Action::Close, "BTC", 0.0), 100.0, 3, &[], &all()), Plan::Hold(_)));
        assert_eq!(plan(&d(Action::Close, "ETH", 0.0), 100.0, 3, &[pos("ETH")], &all()), Plan::Close { coin: "ETH".into() });
    }
}
