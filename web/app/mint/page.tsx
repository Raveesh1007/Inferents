"use client";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { PageHero } from "@/components/chrome";
import { API, CONTRACT, MODELS, NEAR_USDC, ONE_CLICK } from "@/lib/config";
import { usd, view } from "@/lib/near";
import { sha256Hex } from "@/lib/verify";
import { useWallet } from "@/lib/wallet";

const PAY = ["USDC", "SOL", "ETH", "BTC"] as const;
const STORE = "inferents.pendingStrategy";
type Token = { assetId: string; symbol: string; blockchain: string };

export default function Mint() {
  const { accountId, connect, payUsdc } = useWallet();
  const [strategy, setStrategy] = useState("Momentum. Trade only in the direction of the trend. Close when the trend turns.");
  const [model, setModel] = useState(MODELS[0].id);
  const [amount, setAmount] = useState(20);
  const [creditsPct, setCreditsPct] = useState(15);
  const [leverage, setLeverage] = useState(3);
  const [pay, setPay] = useState<(typeof PAY)[number]>("USDC");
  const [quote, setQuote] = useState("");
  const [status, setStatus] = useState("");
  const [hash, setHash] = useState("");
  const [minted, setMinted] = useState<string[]>([]);

  const picked = MODELS.find((m) => m.id === model)!;
  const credits = (amount * creditsPct) / 100;
  const decisions = Math.floor((credits * 1e6) / picked.cost);

  useEffect(() => { sha256Hex(strategy).then(setHash); }, [strategy]);

  // Live 1Click dry quote: what it would cost to fund the deposit from another chain. Nothing is sent.
  useEffect(() => {
    if (pay === "USDC") return setQuote("On testnet the deposit is paid with mock USDC, so this payment is simulated. On mainnet it arrives as USDC on NEAR.");
    let live = true;
    setQuote("Fetching a live 1Click quote");
    (async () => {
      const tokens: Token[] = await fetch(`${ONE_CLICK}/tokens`).then((r) => r.json());
      const chain = { SOL: "sol", ETH: "eth", BTC: "btc" }[pay];
      const t = tokens.find((x) => x.symbol === pay && x.blockchain === chain);
      if (!t) throw new Error("token not listed");
      const q = await fetch(`${ONE_CLICK}/quote`, {
        method: "POST", headers: { "content-type": "application/json" },
        body: JSON.stringify({
          dry: true, swapType: "EXACT_OUTPUT", slippageTolerance: 100, originAsset: t.assetId, depositType: "ORIGIN_CHAIN", destinationAsset: NEAR_USDC,
          amount: String(Math.round(amount * 1e6)), refundTo: accountId ?? "example.near", refundType: "ORIGIN_CHAIN", recipient: accountId ?? "example.near",
          recipientType: "DESTINATION_CHAIN", deadline: new Date(Date.now() + 3600_000).toISOString(),
        }),
      }).then((r) => r.json());
      if (live) setQuote(`${q.quote.amountInFormatted} ${pay}, about ${usd(Number(q.quote.amountInUsd))}, delivers ${amount} USDC on NEAR. This is a dry quote; the testnet payment itself is simulated with mock USDC.`);
    })().catch((e) => live && setQuote(`No quote right now (${e instanceof Error ? e.message : e}). The testnet payment is simulated with mock USDC.`));
    return () => { live = false; };
  }, [pay, amount, accountId]);

  // After minting, find the new agent(s) carrying this strategy's hash and hand the text to the runner. Popup wallets
  // (Meteor) return here without a reload, redirect wallets come back with a reload; both end up in `finish`.
  const finish = useCallback(async () => {
    if (!accountId) return;
    const pending = localStorage.getItem(STORE);
    if (!pending) return;
    const { hash: h, text } = JSON.parse(pending);
    for (let tries = 0; tries < 30; tries++) {
      const toks = await view<{ token_id: string; metadata: { extra: string } }[]>(CONTRACT, "nft_tokens_for_owner", { account_id: accountId, limit: 100 });
      const mine = toks.filter((t) => JSON.parse(t.metadata.extra).strategy_hash === h).map((t) => t.token_id);
      const waiting: string[] = [];
      for (const id of mine) {
        const st = await fetch(`${API}/strategy/${id}`).then((r) => r.json()).catch(() => ({ stored: false }));
        if (!st.stored) waiting.push(id);
      }
      if (mine.length && !waiting.length) {
        localStorage.removeItem(STORE);
        setMinted(mine);
        setStatus("");
        return;
      }
      for (const id of waiting) {
        setStatus(`Agent ${id} is minted. Handing its strategy to the runner`);
        const res = await fetch(`${API}/strategy/${id}`, { method: "POST", headers: { "content-type": "text/plain" }, body: text });
        if (!res.ok) throw new Error(await res.text());
      }
      if (waiting.length) continue;
      if (!mine.length) {
        setStatus("Deposit confirmed. Waiting for the agent to appear on chain");
        await new Promise((r) => setTimeout(r, 3000));
      }
    }
    throw new Error("the agent did not appear on chain within 90 seconds; reload this page to try again");
  }, [accountId]);

  useEffect(() => {
    finish().catch((e) => setStatus(`Strategy hand-off is pending: ${e instanceof Error ? e.message : e}`));
    // Run once per connected account; `finish` itself is stable for a given account.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accountId]);

  async function mint() {
    if (!accountId) return connect();
    localStorage.setItem(STORE, JSON.stringify({ hash, text: strategy }));
    setMinted([]);
    setStatus("Confirm the deposit in your wallet");
    try {
      await payUsdc(String(Math.round(amount * 1e6)), { action: "mint", owner: accountId, max_leverage: leverage, credits_bps: creditsPct * 100, model, strategy_hash: hash });
      setStatus("Deposit confirmed. Waiting for the agent to appear on chain");
      await finish();
    } catch (e) {
      setStatus(`Not finished: ${e instanceof Error ? e.message : e}`);
    }
  }

  return (
    <>
      <PageHero eyebrow="Mint" title="Mint an agent">
        <p>Write its strategy, pick its model, and decide how much of the deposit it may spend on thinking. The rest becomes margin on its own Hyperliquid account.</p>
      </PageHero>

      <div className="wrap page-body">
        <div className="split">
          <div className="form">
            <label className="field">
              Strategy, kept private: only its hash goes on-chain
              <textarea value={strategy} onChange={(e) => setStrategy(e.target.value)} />
              <span className="mono faint break">sha256 {hash}</span>
            </label>

            <div className="block">
              <span className="mono faint">Model</span>
              <div className="panel flush scroll" style={{ marginTop: 8 }}>
                <table>
                  <thead><tr><th /><th>Model</th><th>Receipts</th><th className="num">Cost per decision</th><th className="num">Latency</th></tr></thead>
                  <tbody>
                    {MODELS.map((m) => (
                      <tr key={m.id} onClick={() => setModel(m.id)} style={{ cursor: "pointer" }}>
                        <td><input type="radio" name="model" aria-label={m.label} checked={model === m.id} onChange={() => setModel(m.id)} /></td>
                        <td>{m.label}</td>
                        <td><span className={`kind ${m.kind === "gateway" ? "gateway" : ""}`}>{m.kind === "gateway" ? "gateway-verified" : "model-verified"}</span></td>
                        <td className="num">{usd(m.cost / 1e6, 4)}</td>
                        <td className="num">{m.secs == null ? "n/a" : `${m.secs} s`}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>

          <div className="panel form">
            <div className="row" style={{ alignItems: "end" }}>
              <label className="field" style={{ flex: 1, minWidth: 140 }}>Deposit, USDC<input type="number" min={1} value={amount} onChange={(e) => setAmount(Number(e.target.value))} /></label>
              <label className="field" style={{ flex: 1, minWidth: 140 }}>Leverage cap<input type="number" min={1} max={40} value={leverage} onChange={(e) => setLeverage(Number(e.target.value))} /></label>
            </div>

            <div style={{ display: "grid", gap: 12 }}>
              <div className="row between mono">
                <span>{usd(credits)} thinking</span>
                <span className="dim">{usd(amount - credits)} margin</span>
              </div>
              <div className="bar" aria-hidden><i style={{ width: `${creditsPct}%` }} /></div>
              <input type="range" min={1} max={100} value={creditsPct} aria-label="Share of the deposit kept as thinking credits" onChange={(e) => setCreditsPct(Number(e.target.value))} />
              <p className="note" style={{ margin: 0 }}>{usd(credits)} of thinking is about {decisions.toLocaleString()} decisions with {picked.label}. What it does not spend becomes margin at the end of each epoch.</p>
            </div>

            <label className="field">
              Pay with
              <select value={pay} onChange={(e) => setPay(e.target.value as (typeof PAY)[number])}>{PAY.map((p) => <option key={p}>{p}</option>)}</select>
              <span className="faint" style={{ fontFamily: "var(--font-sans)", fontSize: 14 }}>{quote}</span>
            </label>

            <button className="btn large" onClick={mint} style={{ justifyContent: "center" }}>{accountId ? `Mint for ${usd(amount)}` : "Connect wallet to mint"}</button>
            {status && <p className="warn" style={{ margin: 0 }}>{status}</p>}
            {minted.length > 0 && (
              <div style={{ display: "grid", gap: 8 }}>
                <span className="pos">Your agent is live. The runner funds its Hyperliquid account and it starts trading on its next pass.</span>
                {minted.map((id) => <Link key={id} href={`/agent/${id}`} className="arrow">Open agent {id}</Link>)}
              </div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
