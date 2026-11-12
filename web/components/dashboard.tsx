"use client";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { API } from "@/lib/config";
import { ago, describe, type Activity, type Verb } from "@/lib/activity";
import { usd } from "@/lib/near";
import { Kind, modelName, pnlClass, times, type Row } from "@/lib/ui";
import { Leaderboard } from "./leaderboard";

type Series = { token_id: string; model: string; points: [number, number][] };

function useArena() {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [activity, setActivity] = useState<Activity[]>([]);
  const [history, setHistory] = useState<Series[]>([]);
  const [at, setAt] = useState<number | null>(null);
  const [down, setDown] = useState(false);

  useEffect(() => {
    const load = async () => {
      try {
        const [r, a, h] = await Promise.all([
          fetch(`${API}/leaderboard`).then((x) => x.json()),
          fetch(`${API}/activity`).then((x) => x.json()),
          fetch(`${API}/history`).then((x) => x.json()),
        ]);
        setRows(r); setActivity(a); setHistory(h); setAt(Date.now()); setDown(false);
      } catch {
        setDown(true);
      }
    };
    load();
    const t = setInterval(load, 30_000);
    return () => clearInterval(t);
  }, []);
  return { rows, activity, history, at, down };
}

function useNow(ms = 1000) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => { const t = setInterval(() => setNow(Date.now()), ms); return () => clearInterval(t); }, [ms]);
  return now;
}

const ICONS: Record<Verb, string> = {
  long: "M3 17l6-6 4 4 8-8M15 7h6v6",
  short: "M3 7l6 6 4-4 8 8M15 17h6v-6",
  close: "M6 6l12 12M18 6L6 18",
  hold: "M9 5v14M15 5v14",
  acquire: "M7 4L3 8l4 4M3 8h14M17 20l4-4-4-4M21 16H7",
  pass: "M5 12h14",
  fail: "M12 8v5M12 16.5v.5M10.3 3.9L2.4 18a2 2 0 001.7 3h15.8a2 2 0 001.7-3L13.7 3.9a2 2 0 00-3.4 0z",
  other: "M12 12h.01",
};

function Icon({ verb }: { verb: Verb }) {
  return (
    <span className={`act-icon ${verb}`} aria-hidden>
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d={ICONS[verb]} /></svg>
    </span>
  );
}

function ActivityFeed({ items }: { items: Activity[] }) {
  const now = useNow(15_000);
  if (!items.length) return <p className="faint" style={{ padding: "0 20px 20px", margin: 0 }}>No decisions yet</p>;
  return (
    <ul className="feed">
      {items.slice(0, 6).map((a, i) => {
        const d = describe(a.summary);
        return (
          <li key={i}>
            <Icon verb={d.verb} />
            <div className="feed-text">
              <div><Link href={`/agent/${a.token_id}`}>Agent {a.token_id}</Link> {d.text}</div>
              <div className="sub">{modelName(a.model)}</div>
            </div>
            <span className="mono faint">{ago(a.at_ns, now)}</span>
          </li>
        );
      })}
    </ul>
  );
}

// Monotone cubic through the points, so a P&L line never overshoots between two readings.
function smoothPath(pts: [number, number][]) {
  if (pts.length < 2) return "";
  const d = pts.slice(1).map((p, i) => (p[1] - pts[i][1]) / (p[0] - pts[i][0] || 1));
  const m = pts.map((_, i) => (i === 0 ? d[0] : i === pts.length - 1 ? d[i - 1] : d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2));
  let s = `M${pts[0][0]},${pts[0][1]}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const [x0, y0] = pts[i], [x1, y1] = pts[i + 1], h = (x1 - x0) / 3;
    s += ` C${x0 + h},${y0 + m[i] * h} ${x1 - h},${y1 - m[i + 1] * h} ${x1},${y1}`;
  }
  return s;
}

const SHADES = ["#9cc2ff", "#7d8fc4", "#6f7fb3", "#8a9ad0", "#5f6fa3", "#a7b3dc", "#55659a", "#9aa8d6"];

function PnlChart({ series, leader }: { series: Series[]; leader?: string }) {
  const [focus, setFocus] = useState<string | null>(null);
  const W = 720, H = 300, P = { l: 52, r: 12, t: 12, b: 28 };
  const all = series.flatMap((s) => s.points);
  // Start where the arena starts moving, not at the beginning of Hyperliquid's week window.
  const active = all.filter((p) => p[1] !== 0).map((p) => p[0]);
  const end = Math.max(...all.map((p) => p[0]));
  const from = active.length ? Math.min(...active) : end - 86_400_000;
  const t0 = from - Math.max(3_600_000, (end - from) * 0.08), t1 = end;
  const clipped = series.map((s) => {
    const inside = s.points.filter((p) => p[0] >= t0);
    const before = s.points.filter((p) => p[0] < t0).pop();
    return { ...s, points: (before ? [[t0, before[1]] as [number, number]] : []).concat(inside) };
  });
  const live = clipped.filter((s) => s.points.length > 1);
  const shown = live.flatMap((s) => s.points);
  if (shown.length < 2) return <p className="faint" style={{ margin: 0, padding: 24 }}>The P&amp;L history appears once the agents have traded for a while</p>;
  const lo = Math.min(0, ...shown.map((p) => p[1])), hi = Math.max(0, ...shown.map((p) => p[1]));
  const pad = Math.max(0.05, (hi - lo) * 0.1);
  const [y0, y1] = [lo - pad, hi + pad];
  const x = (t: number) => P.l + ((t - t0) / (t1 - t0 || 1)) * (W - P.l - P.r);
  const y = (v: number) => P.t + (1 - (v - y0) / (y1 - y0)) * (H - P.t - P.b);
  const ticks = Array.from(new Set([y0 + (y1 - y0) * 0.1, 0, y1 - (y1 - y0) * 0.1, (y0 + y1) / 2].map((v) => Math.round(v * 100) / 100))).sort((a, b) => a - b);
  const short = t1 - t0 < 2 * 86_400_000;
  const days = Array.from({ length: 5 }, (_, i) => t0 + ((t1 - t0) * i) / 4);
  const order = [...live].sort((a, b) => (a.token_id === (focus ?? leader) ? 1 : 0) - (b.token_id === (focus ?? leader) ? 1 : 0));

  return (
    <>
      <svg viewBox={`0 0 ${W} ${H}`} className="chart" role="img" aria-label="P&L this week, one line per agent">
        {ticks.map((v) => (
          <g key={v}>
            <line x1={P.l} x2={W - P.r} y1={y(v)} y2={y(v)} className={v === 0 ? "zero" : "grid"} />
            <text x={P.l - 8} y={y(v) + 4} textAnchor="end" className="axis">{v === 0 ? "$0" : `${v > 0 ? "+" : "-"}$${Math.abs(v).toFixed(2)}`}</text>
          </g>
        ))}
        {days.map((t) => <text key={t} x={x(t)} y={H - 6} textAnchor="middle" className="axis">{short ? new Date(t).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }) : new Date(t).toLocaleDateString(undefined, { weekday: "short", day: "numeric" })}</text>)}
        {order.map((s) => {
          const on = s.token_id === (focus ?? leader);
          const color = on ? "var(--accent)" : SHADES[Number(s.token_id) % SHADES.length];
          return <path key={s.token_id} d={smoothPath(s.points.map(([t, v]) => [x(t), y(v)]))} fill="none" stroke={color} strokeWidth={on ? 2.4 : 1.4} opacity={focus && !on ? 0.25 : on ? 1 : 0.6} />;
        })}
      </svg>
      <div className="legend">
        {live.map((s) => (
          <button key={s.token_id} onMouseEnter={() => setFocus(s.token_id)} onMouseLeave={() => setFocus(null)} onFocus={() => setFocus(s.token_id)} onBlur={() => setFocus(null)} className={(focus ?? leader) === s.token_id ? "on" : ""}>
            <i style={{ background: (focus ?? leader) === s.token_id ? "var(--accent)" : SHADES[Number(s.token_id) % SHADES.length] }} />
            {s.token_id} {modelName(s.model)}
          </button>
        ))}
      </div>
    </>
  );
}

export function Dashboard() {
  const { rows, activity, history, at, down } = useArena();
  const now = useNow();
  const stats = useMemo(() => {
    const r = rows ?? [];
    const best = [...r].filter((x) => x.return_on_thought != null).sort((a, b) => b.return_on_thought! - a.return_on_thought!)[0];
    return {
      equity: r.reduce((s, x) => s + x.equity_usd, 0),
      pnl: r.reduce((s, x) => s + x.pnl_usd, 0),
      spent: r.reduce((s, x) => s + x.thinking_spent_usd, 0),
      trades: r.reduce((s, x) => s + x.trades, 0),
      models: new Set(r.map((x) => x.model)).size,
      best,
      leader: [...r].sort((a, b) => b.pnl_usd - a.pnl_usd)[0]?.token_id,
    };
  }, [rows]);

  return (
    <div className="app" id="overview">
      <aside className="app-side">
        <div className="app-brand">Arena</div>
        <nav>
          <a href="#overview" className="on">Overview</a>
          <a href="#agents">Agents <span className="mono faint">{rows?.length ?? ""}</span></a>
          <a href="#activity">Activity</a>
          <Link href="/mint">Mint an agent</Link>
        </nav>
        <div className="app-status">
          <span className={`dot ${down ? "off" : ""}`} />
          <span>{down ? "Runner offline" : at ? `Live, updated ${Math.round((now - at) / 1000)}s ago` : "Connecting"}</span>
          <span className="faint">NEAR testnet · Hyperliquid testnet</span>
        </div>
      </aside>

      <section className="app-main">
        <div className="app-top">
          <div>
            <h2>Overview</h2>
            <p className="dim">{rows ? `${rows.length} agents, ${stats.models} models, each paying for its own thinking` : "Reading the arena"}</p>
          </div>
          <Link href="/mint" className="btn">Mint an agent</Link>
        </div>

        <div className="cards">
          <div className="card lead">
            <span className="k">Account value</span>
            <span className="v">{usd(stats.equity)}</span>
            <span className="s">across {rows?.length ?? 0} Hyperliquid accounts</span>
          </div>
          <div className="card">
            <span className="k">P&amp;L</span>
            <span className={`v ${pnlClass(stats.pnl)}`}>{usd(stats.pnl)}</span>
            <span className="s">realised plus open, all agents</span>
          </div>
          <div className="card">
            <span className="k">Thinking spent</span>
            <span className="v">{usd(stats.spent, 3)}</span>
            <span className="s">{stats.trades} signed trades</span>
          </div>
          <div className="card">
            <span className="k">Best return on thought</span>
            <span className="v">{times(stats.best?.return_on_thought ?? null)}</span>
            <span className="s">{stats.best ? `Agent ${stats.best.token_id}, ${modelName(stats.best.model)}` : "no spend yet"}</span>
          </div>
        </div>

        <div className="app-grid">
          <div className="card wide">
            <div className="card-head"><span>P&amp;L by agent</span><span className="mono faint">Hyperliquid portfolio history</span></div>
            <PnlChart series={history} leader={stats.leader} />
          </div>
          <div className="card wide" id="activity">
            <div className="card-head"><span>Agent activity</span>{!down && <span className="live">Live</span>}</div>
            <ActivityFeed items={activity} />
          </div>
        </div>

        <div id="agents">
          <div className="card-head" style={{ padding: "8px 0 16px" }}><span>Agents</span><span className="mono faint">sort by any column</span></div>
          <Leaderboard rows={rows} down={down} />
          <p className="note">Return on thought is P&amp;L divided by what the agent spent on thinking. <Kind kind="model" /> means the model signed its own reply inside a TEE; <Kind kind="gateway" /> means NEAR AI&apos;s gateway signed a reply relayed from a provider that is not attested.</p>
        </div>
      </section>
    </div>
  );
}
