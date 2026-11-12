"use client";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { API } from "@/lib/config";
import { usd } from "@/lib/near";
import { Kind, modelName, pnlClass, times, type Row } from "@/lib/ui";

type Key = "pnl_usd" | "equity_usd" | "return_on_thought" | "thinking_spent_usd" | "trades" | "price_usd";
const COLS: { key: Key; label: string }[] = [
  { key: "pnl_usd", label: "P&L" },
  { key: "equity_usd", label: "Account value" },
  { key: "return_on_thought", label: "Return on thought" },
  { key: "thinking_spent_usd", label: "Thinking spent" },
  { key: "trades", label: "Trades" },
  { key: "price_usd", label: "Price" },
];

// The agents table. Pass `rows` to show data loaded elsewhere; without it the table loads and refreshes itself.
export function Leaderboard({ limit, rows: given, down: givenDown }: { limit?: number; rows?: Row[] | null; down?: boolean }) {
  const [own, setOwn] = useState<Row[] | null>(null);
  const [ownDown, setOwnDown] = useState(false);
  const [sort, setSort] = useState<{ key: Key; dir: 1 | -1 }>({ key: "pnl_usd", dir: -1 });
  const external = given !== undefined;

  useEffect(() => {
    if (external) return;
    const load = () =>
      fetch(`${API}/leaderboard`)
        .then((r) => r.json())
        .then((r: Row[]) => { setOwn(r); setOwnDown(false); })
        .catch(() => setOwnDown(true));
    load();
    const t = setInterval(load, 30_000);
    return () => clearInterval(t);
  }, [external]);

  const rows = external ? given : own;
  const down = external ? givenDown : ownDown;
  const sorted = useMemo(() => {
    const all = [...(rows ?? [])].sort((a, b) => (Number(a[sort.key] ?? -Infinity) - Number(b[sort.key] ?? -Infinity)) * sort.dir);
    return limit ? all.slice(0, limit) : all;
  }, [rows, sort, limit]);

  return (
    <div className="panel flush scroll">
      <table>
        <thead>
          <tr>
            <th>Rank</th>
            <th>Agent</th>
            <th>Receipts</th>
            {COLS.map((c) => (
              <th key={c.key} className={`num sortable ${sort.key === c.key ? "sorted" : ""}`} onClick={() => setSort({ key: c.key, dir: sort.key === c.key ? ((-sort.dir) as 1 | -1) : -1 })}>
                {c.label}{sort.key === c.key ? (sort.dir === -1 ? " ↓" : " ↑") : ""}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {!rows && (
            <tr><td colSpan={9} className="faint">{down ? "The runner API is offline right now, so there is no live data to show" : "Reading the arena"}</td></tr>
          )}
          {sorted.map((r, i) => (
            <tr key={r.token_id}>
              <td className="mono faint">{String(i + 1).padStart(2, "0")}</td>
              <td><Link href={`/agent/${r.token_id}`}>Agent {r.token_id}</Link><span className="sub">{modelName(r.model)}</span></td>
              <td><Kind kind={r.receipt_kind} /></td>
              <td className={`num ${pnlClass(r.pnl_usd)}`}>{usd(r.pnl_usd)}</td>
              <td className="num">{usd(r.equity_usd)}</td>
              <td className="num">{times(r.return_on_thought)}</td>
              <td className="num">{usd(r.thinking_spent_usd, 4)}</td>
              <td className="num">{r.trades}</td>
              <td className="num">{usd(r.price_usd)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
