"use client";
import { useEffect, useState } from "react";
import { API } from "@/lib/config";
import { ago, describe, type Activity } from "@/lib/activity";
import { modelName } from "@/lib/ui";

const FALLBACK = [
  "Each NFT is an AI trading agent that owns its own Hyperliquid account",
  "Accounts controlled through NEAR Chain Signatures",
  "Thinking billed by NEAR AI Cloud",
  "Unused credits converted through NEAR Intents",
];

// A slow tape of what the agents just did, read from the runner. Hover to pause.
export function Ticker() {
  const [items, setItems] = useState<string[] | null>(null);

  useEffect(() => {
    const load = () =>
      fetch(`${API}/activity`)
        .then((r) => r.json())
        .then((list: Activity[]) => {
          const now = Date.now();
          const lines = list
            .map((a) => ({ a, d: describe(a.summary) }))
            .filter(({ d }) => d.verb !== "fail")
            .slice(0, 16)
            .map(({ a, d }) => `Agent ${a.token_id} · ${modelName(a.model)} ${d.text} · ${ago(a.at_ns, now)} ago`);
          setItems(lines.length ? lines : FALLBACK);
        })
        .catch(() => setItems(FALLBACK));
    load();
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  }, []);

  const list = items ?? FALLBACK;
  const live = items !== null && items !== FALLBACK;
  return (
    <div className="ticker" aria-label="Recent agent activity">
      {live && <span className="ticker-live">Live</span>}
      <div className="ticker-window">
        <div className="ticker-track" style={{ animationDuration: `${Math.max(40, list.length * 7)}s` }}>
          {[0, 1].map((copy) => (
            <div className="ticker-set" key={copy} aria-hidden={copy === 1}>
              {list.map((line, i) => <span key={i}>{line}</span>)}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
