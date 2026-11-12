use crate::arena::Row;

// The NFT's image: a card drawn fresh from the agent's live state each time a wallet or marketplace asks for it.
// Plain SVG with system fonts, so it renders the same in a wallet, an <img> tag or a browser tab.

const SANS: &str = "Helvetica Neue, Helvetica, Arial, sans-serif";
const MONO: &str = "SFMono-Regular, Menlo, Consolas, monospace";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn money(x: f64) -> String {
    format!("{}${:.2}", if x < 0.0 { "-" } else { "" }, x.abs())
}

// Each model family gets its own light, so two agents read apart at a glance; all stay inside the site's night palette.
fn hue(model: &str) -> u32 {
    let m = model.to_lowercase();
    if m.contains("claude") { 28 } else if m.contains("gpt") || m.contains("openai") { 158 } else if m.contains("gemini") { 200 } else if m.contains("qwen") { 255 } else if m.contains("glm") { 182 } else { 222 }
}

pub fn render(r: &Row, updated_utc: &str) -> String {
    let h = hue(&r.model);
    let name = r.model.rsplit('/').next().unwrap_or(&r.model);
    let title_size = (980.0 / name.len().max(8) as f64).clamp(44.0, 88.0);
    let gateway = r.receipt_kind.starts_with("gateway");
    let position = r.positions.first().map_or("Flat".to_owned(), |p| {
        let size = p["size"].as_f64().unwrap_or(0.0);
        format!("{} {} {}", if size > 0.0 { "Long" } else { "Short" }, p["coin"].as_str().unwrap_or("?"), size.abs())
    });
    let pnl_color = if r.pnl_usd > 0.0 { "#7ad6ad" } else if r.pnl_usd < 0.0 { "#f09393" } else { "#f3f5ff" };
    let rows = [
        ("Account value", money(r.equity_usd), "#f3f5ff"),
        ("P&amp;L", money(r.pnl_usd), pnl_color),
        ("Credits left", money(r.credits_usd), "#f3f5ff"),
        ("Position", esc(&position), "#f3f5ff"),
    ];
    let list: String = rows
        .iter()
        .enumerate()
        .map(|(i, (k, v, c))| {
            let y = 668 + i as i32 * 64;
            format!(
                r##"<line x1="560" x2="936" y1="{l}" y2="{l}" stroke="#bccdff" stroke-opacity="0.12"/><text x="560" y="{y}" font-family="{MONO}" font-size="20" fill="#7682ad">{k}</text><text x="936" y="{y}" text-anchor="end" font-family="{SANS}" font-size="26" fill="{c}">{v}</text>"##,
                l = y - 38
            )
        })
        .collect();

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000" viewBox="0 0 1000 1000">
<defs>
<radialGradient id="limb" gradientUnits="userSpaceOnUse" cx="500" cy="-140" r="760" gradientTransform="translate(500 -140) scale(1.3 1) translate(-500 140)">
<stop offset="0" stop-color="#070a24"/>
<stop offset="0.52" stop-color="hsl({h},70%,16%)"/>
<stop offset="0.72" stop-color="hsl({h},72%,45%)"/>
<stop offset="0.84" stop-color="hsl({h},80%,70%)"/>
<stop offset="0.9" stop-color="hsl({h},60%,92%)"/>
<stop offset="0.95" stop-color="hsl({h},25%,40%)"/>
<stop offset="1" stop-color="#070a24"/>
</radialGradient>
</defs>
<rect width="1000" height="1000" fill="url(#limb)"/>
<rect x="0.5" y="0.5" width="999" height="999" fill="none" stroke="#bccdff" stroke-opacity="0.18"/>
<text x="64" y="84" font-family="{SANS}" font-size="28" letter-spacing="1" fill="#ffffff">inferents</text>
<text x="936" y="84" text-anchor="end" font-family="{MONO}" font-size="22" fill="#ffffff">Agent #{id}</text>
<text x="500" y="392" text-anchor="middle" font-family="{SANS}" font-weight="300" font-size="{title_size:.0}" letter-spacing="-1.5" fill="#ffffff">{name}</text>
<text x="500" y="454" text-anchor="middle" font-family="{MONO}" font-size="20" fill="#e8edff"><tspan fill="{dot}">●</tspan>  {label}</text>
<text x="64" y="636" font-family="{MONO}" font-size="20" fill="#7682ad">Live price</text>
<text x="60" y="736" font-family="{SANS}" font-weight="300" font-size="104" letter-spacing="-3" fill="#ffffff">{price}</text>
<text x="64" y="790" font-family="{MONO}" font-size="18" fill="#7682ad">account value + unused credits</text>
<text x="64" y="816" font-family="{MONO}" font-size="18" fill="#7682ad">+ 20% of realised profit</text>
{list}
<line x1="64" x2="936" y1="900" y2="900" stroke="#bccdff" stroke-opacity="0.12"/>
<circle cx="70" cy="940" r="5" fill="#7ad6ad"/>
<text x="86" y="947" font-family="{MONO}" font-size="18" fill="#aab4d8">Live, updated {updated}</text>
<text x="936" y="947" text-anchor="end" font-family="{MONO}" font-size="18" fill="#aab4d8">Hyperliquid {addr}</text>
</svg>"##,
        id = esc(&r.token_id),
        name = esc(name),
        dot = if gateway { "#e8c27a" } else { "#7ad6ad" },
        label = if gateway { "gateway-verified receipts" } else { "model-verified receipts" },
        price = money(r.price_usd),
        updated = esc(updated_utc),
        addr = esc(&format!("{}…{}", &r.hl_address.get(..6).unwrap_or(""), &r.hl_address.get(r.hl_address.len().saturating_sub(4)..).unwrap_or(""))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(model: &str) -> Row {
        Row {
            token_id: "6".into(), owner: "alice.testnet".into(), model: model.into(), receipt_kind: "model-verified", strategy_hash: String::new(),
            hl_address: "0x5e87abbb3acb27a0c4d4594e90f3ab154b76b11e".into(), equity_usd: 17.21, realized_pnl_usd: 0.1, pnl_usd: -0.42,
            thinking_spent_usd: 0.02, return_on_thought: None, trades: 3, credits_usd: 2.98, price_usd: 19.96, acquired_by: None,
            epoch_end_ns: "0".into(), positions: vec![json!({"coin": "BTC", "size": -0.00015})],
        }
    }

    #[test]
    fn card_shows_live_figures_and_is_well_formed() {
        let svg = render(&row("deepseek-ai/DeepSeek-V4.1-Flash-1017"), "12 Nov 14:02 UTC");
        for want in ["$19.96", "$17.21", "-$0.42", "$2.98", "Short BTC 0.00015", "DeepSeek-V4.1-Flash-1017", "Agent #6", "0x5e87…b11e", "model-verified"] {
            assert!(svg.contains(want), "missing {want}");
        }
        assert_eq!(svg.matches("<text").count(), svg.matches("</text>").count());
    }

    #[test]
    fn model_names_are_escaped() {
        let svg = render(&row("evil/<script>&"), "now");
        assert!(svg.contains("&lt;script&gt;&amp;") && !svg.contains("<script>"));
    }
}
