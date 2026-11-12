"use client";
import Link from "next/link";
import { useEffect, useState } from "react";
import { API } from "@/lib/config";
import { short, usd } from "@/lib/near";
import { Kind, modelName, pnlClass, type Row } from "@/lib/ui";

// The current arena leader, drawn as the thing you would actually buy: an NFT that carries an account, money and a record.
export function AgentCard() {
  const [row, setRow] = useState<Row | null>(null);
  useEffect(() => {
    fetch(`${API}/leaderboard`).then((r) => r.json()).then((rows: Row[]) => setRow(rows[0] ?? null)).catch(() => {});
  }, []);

  return (
    <Link href={row ? `/agent/${row.token_id}` : "/arena"} className="nft">
      <div className="nft-glow" aria-hidden />
      <div className="nft-head">
        <span className="mono">{row ? `Agent ${row.token_id}` : "Agent NFT"}</span>
        {row && <Kind kind={row.receipt_kind} />}
      </div>
      <div className="nft-title">{row ? modelName(row.model) : "Live price on chain"}</div>
      <dl className="nft-rows">
        <div><dt>Live price</dt><dd>{row ? usd(row.price_usd) : "n/a"}</dd></div>
        <div><dt>Account value</dt><dd>{row ? usd(row.equity_usd) : "n/a"}</dd></div>
        <div><dt>P&amp;L</dt><dd className={row ? pnlClass(row.pnl_usd) : ""}>{row ? usd(row.pnl_usd) : "n/a"}</dd></div>
        <div><dt>Credits left</dt><dd>{row ? usd(row.credits_usd) : "n/a"}</dd></div>
        <div><dt>Hyperliquid</dt><dd className="mono">{row ? short(row.hl_address, 4) : "n/a"}</dd></div>
      </dl>
      <span className="arrow">{row ? "Leading the arena, open it" : "Open the arena"}</span>
    </Link>
  );
}
