"use client";
import Link from "next/link";
import { use, useEffect, useState } from "react";
import { PageHero } from "@/components/chrome";
import { API, CONTRACT, HL_INFO } from "@/lib/config";
import { micro, short, usd, view } from "@/lib/near";
import { Kind, modelName, pnlClass, times, type Row } from "@/lib/ui";
import type { Receipt } from "@/lib/verify";
import { useWallet } from "@/lib/wallet";

type Chain = { key_price_usdc: string; key_duration_ns: string; epoch_end_ns: string; acquired_by: string | null; max_leverage: number };
type HlPosition = { position: { coin: string; szi: string; entryPx: string; unrealizedPnl: string } };

export default function AgentPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = use(params);
  const { accountId, connect, payUsdc } = useWallet();
  const [row, setRow] = useState<Row | null>(null);
  const [chain, setChain] = useState<Chain | null>(null);
  const [receipts, setReceipts] = useState<Receipt[]>([]);
  const [live, setLive] = useState<Row["positions"]>([]);
  const [msg, setMsg] = useState("");
  const [access, setAccess] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const load = async () => {
      const r: Row = await fetch(`${API}/agents/${id}`).then((x) => (x.ok ? x.json() : Promise.reject(new Error("no such agent"))));
      setRow(r);
      setChain(await view<Chain>(CONTRACT, "agent", { token_id: id }));
      setReceipts((await fetch(`${API}/receipts/${id}`).then((x) => x.json())).reverse());
      // Straight from Hyperliquid's public API, not through the runner.
      const s = await fetch(HL_INFO, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ type: "clearinghouseState", user: r.hl_address }) }).then((x) => x.json());
      setLive(((s.assetPositions ?? []) as HlPosition[]).map((p) => ({ coin: p.position.coin, size: Number(p.position.szi), entry_px: Number(p.position.entryPx), unrealized: Number(p.position.unrealizedPnl) })).filter((p) => p.size !== 0));
    };
    load().catch((e) => setError(e instanceof Error ? e.message : String(e)));
    const t = setInterval(() => load().catch(() => {}), 30_000);
    return () => clearInterval(t);
  }, [id]);

  // A viewing key is a record on the contract, not a token in the wallet, so show it here.
  useEffect(() => {
    if (!accountId) return setAccess(false);
    view<boolean>(CONTRACT, "has_access", { token_id: id, account: accountId }).then(setAccess).catch(() => setAccess(false));
  }, [id, accountId, msg]);

  const [quote, setQuote] = useState<{ price: number; at: number } | null>(null);

  // Step one: the runner reports the account value on chain so the contract can quote a price for the next two minutes.
  async function getQuote() {
    if (!accountId) return connect();
    try {
      setMsg("Asking the runner to report this agent's account value on chain");
      const q = await fetch(`${API}/agents/${id}/refresh`, { method: "POST" }).then((x) => (x.ok ? x.json() : x.text().then((t) => Promise.reject(new Error(t)))));
      setQuote({ price: Number(q.price_micro_usd), at: Date.now() });
      setMsg("");
    } catch (e) {
      setMsg(`No quote: ${e instanceof Error ? e.message : e}`);
    }
  }

  // Step two, straight from the click so the wallet popup is allowed. A 2% buffer covers drift; any excess is refundable.
  async function buy() {
    if (!accountId || !quote) return;
    if (Date.now() - quote.at > 110_000) {
      setQuote(null);
      return setMsg("That quote expired; get a fresh one");
    }
    try {
      await payUsdc(Math.ceil(quote.price * 1.02).toString(), { action: "buy", owner: accountId, token_id: id });
      setQuote(null);
      setMsg("Bought. The trading key rotates to the runner on the agent's next pass");
    } catch (e) {
      setMsg(`Not bought: ${e instanceof Error ? e.message : e}`);
    }
  }

  async function buyKey() {
    if (!accountId || !chain) return connect();
    await payUsdc(chain.key_price_usdc, { action: "buy_key", owner: accountId, token_id: id }).catch((e) => setMsg(`Key not bought: ${e instanceof Error ? e.message : e}`));
  }

  if (!row) {
    return (
      <PageHero eyebrow={`Agent ${id}`} title={`Agent ${id}`}>
        <p>{error ? `Could not load this agent: ${error}. The runner API may be offline.` : "Reading the agent"}</p>
      </PageHero>
    );
  }

  const acquisitions = receipts.filter((r) => r.order_id.startsWith("acquire-"));
  const epochEnd = chain ? new Date(Number(BigInt(chain.epoch_end_ns) / 1_000_000n)) : null;
  const keyPrice = chain ? micro(chain.key_price_usdc) : 0;
  const mine = accountId === row.owner;

  return (
    <>
      <PageHero eyebrow={`Agent ${id}`} title={modelName(row.model)}>
        <p className="row" style={{ gap: 24 }}>
          <Kind kind={row.receipt_kind} />
          <span className="mono">owner {row.owner}</span>
          <span className="mono">strategy {short(row.strategy_hash, 6)}</span>
        </p>
      </PageHero>

      <div className="wrap page-body">
        <div className="stats">
          <div className="stat"><span className="k">Account value</span><span className="v">{usd(row.equity_usd)}</span><span className={`s ${pnlClass(row.pnl_usd)}`}>{usd(row.pnl_usd)} P&amp;L since mint</span></div>
          <div className="stat"><span className="k">Credits left</span><span className="v">{usd(row.credits_usd)}</span><span className="s">{usd(row.thinking_spent_usd, 4)} spent thinking</span></div>
          <div className="stat"><span className="k">Return on thought</span><span className="v">{times(row.return_on_thought)}</span><span className="s">{row.trades} receipts</span></div>
          <div className="stat"><span className="k">Epoch ends</span><span className="v" style={{ fontSize: 20, lineHeight: 1.4 }}>{epochEnd?.toLocaleDateString(undefined, { day: "numeric", month: "short" })}</span><span className="s">unused credits become margin</span></div>
        </div>

        <div className="split">
          <div className="block">
            <h2>Open positions</h2>
            <div className="panel flush scroll">
              <table>
                <thead><tr><th>Coin</th><th>Side</th><th className="num">Size</th><th className="num">Entry</th><th className="num">Unrealised</th></tr></thead>
                <tbody>
                  {live.length === 0 && <tr><td colSpan={5} className="faint">Flat</td></tr>}
                  {live.map((p) => (
                    <tr key={p.coin}><td>{p.coin}</td><td>{p.size > 0 ? "Long" : "Short"}</td><td className="num">{Math.abs(p.size)}</td><td className="num">{usd(p.entry_px)}</td><td className={`num ${pnlClass(p.unrealized)}`}>{usd(p.unrealized)}</td></tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="note">Read live from Hyperliquid's public API for <span className="mono break">{row.hl_address}</span>, the account this NFT owns.</p>
          </div>

          <div className="block">
            <h2>Own this agent</h2>
            {/* The NFT's own image, as a wallet shows it: drawn live by the runner from this agent's current state. */}
            <img className="nft-art" src={`${API}/nft/${id}.svg?t=${Math.floor(Date.now() / 30_000)}`} alt={`Live NFT card for agent ${id}`} width={1000} height={1000} />
            <div className="panel" style={{ display: "grid", gap: 20 }}>
              <div className="row between">
                <div className="stat" style={{ padding: 0, border: 0 }}><span className="k">Live price</span><span className="v">{usd(row.price_usd)}</span></div>
                {mine ? (
                  <span className="mono dim">You own this agent</span>
                ) : quote ? (
                  <button className="btn large" onClick={buy}>Pay {usd(micro(quote.price))} and buy</button>
                ) : (
                  <button className="btn large" onClick={getQuote}>{accountId ? "Get a live quote" : "Connect wallet to buy"}</button>
                )}
              </div>
              <p className="note" style={{ margin: 0 }}>Account value, plus unused credits, plus 20% of realised profit. The seller receives the price less a 5% royalty to the minter, and the trading key rotates on transfer. Testnet payment uses mock USDC.</p>
              <div className="row between" style={{ borderTop: "1px solid var(--line)", paddingTop: 20 }}>
                {access && !mine ? (
                  <>
                    <span className="pos">You hold a viewing key for this agent. Open any receipt below and choose Sign and unlock to read its reasoning</span>
                    {receipts[0] && <Link href={`/receipt/${id}/${receipts[0].index}`} className="btn quiet">Read latest</Link>}
                  </>
                ) : keyPrice > 0 ? (
                  <>
                    <span className="dim">Viewing key: {usd(keyPrice)} for {Number(chain!.key_duration_ns) / 3.6e12} hours of this agent's reasoning</span>
                    <button className="btn quiet" onClick={buyKey}>Buy viewing key</button>
                  </>
                ) : (
                  <span className="faint">The owner has not priced viewing keys for this agent</span>
                )}
              </div>
              {msg && <p className="warn" style={{ margin: 0 }}>{msg}</p>}
            </div>
          </div>
        </div>

        <div className="block">
          <h2>Receipts</h2>
          <div className="panel flush scroll">
            <table>
              <thead><tr><th>No.</th><th>Kind</th><th>Order</th><th>Request</th><th /></tr></thead>
              <tbody>
                {receipts.length === 0 && <tr><td colSpan={5} className="faint">No receipts yet</td></tr>}
                {receipts.map((r) => (
                  <tr key={r.index}>
                    <td className="mono faint">{r.index}</td>
                    <td><Kind kind={r.label} /></td>
                    <td className="mono">{r.order_id.startsWith("acquire-") ? "acquisition decision" : r.order_id}</td>
                    <td className="mono faint">{short(r.request_id, 8)}</td>
                    <td className="num"><Link href={`/receipt/${id}/${r.index}`} className="arrow">Verify</Link></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        {(chain?.acquired_by || acquisitions.length > 0) && (
          <div className="block">
            <h2>Acquisitions</h2>
            <dl className="facts">
              {chain?.acquired_by && <div><dt>Acquired by</dt><dd><Link href={`/agent/${chain.acquired_by}`} className="arrow">Agent {chain.acquired_by}</Link>, using its own credits</dd></div>}
              {acquisitions.map((r) => (<div key={r.index}><dt>Decision</dt><dd><Link href={`/receipt/${id}/${r.index}`} className="arrow">Receipt {r.index}</Link></dd></div>))}
            </dl>
          </div>
        )}
      </div>
    </>
  );
}
