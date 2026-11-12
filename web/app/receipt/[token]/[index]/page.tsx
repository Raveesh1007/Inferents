"use client";
import Link from "next/link";
import { use, useEffect, useState } from "react";
import { PageHero } from "@/components/chrome";
import { API, CONTRACT } from "@/lib/config";
import { view } from "@/lib/near";
import { Kind, modelName } from "@/lib/ui";
import { checkReceipt, type Receipt, type Step } from "@/lib/verify";
import { useWallet } from "@/lib/wallet";

type Unlocked = { request_body?: string; response_body?: string; content?: string; reasoning?: string; summary?: string; strategy?: string | null; strategy_redacted?: boolean };

export default function ReceiptPage({ params }: { params: Promise<{ token: string; index: string }> }) {
  const { token, index } = use(params);
  const { accountId, connect, signAuth } = useWallet();
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [steps, setSteps] = useState<Step[]>([]);
  const [opened, setOpened] = useState<Unlocked | null>(null);
  const [msg, setMsg] = useState("");
  const [strategyHash, setStrategyHash] = useState("");

  useEffect(() => {
    (async () => {
      const list: Receipt[] = await fetch(`${API}/receipts/${token}`).then((x) => x.json());
      const r = list.find((x) => String(x.index) === index);
      if (!r) return setMsg("No such receipt");
      setReceipt(r);
      setSteps(await checkReceipt(r));
      setStrategyHash((await view<{ strategy_hash: string }>(CONTRACT, "agent", { token_id: token })).strategy_hash);
    })().catch((e) => setMsg(`Could not load this receipt: ${e instanceof Error ? e.message : e}`));
  }, [token, index]);

  async function unlock() {
    if (!receipt) return;
    if (!accountId) return connect();
    setMsg("Sign the request in your wallet");
    try {
      const header = await signAuth({ token, receipt: receipt.request_id });
      const res = await fetch(`${API}/reasoning/${token}/${receipt.request_id}`, { headers: { "x-nep413": header } });
      if (!res.ok) throw new Error(await res.text());
      const body: Unlocked = await res.json();
      setOpened(body);
      setSteps(await checkReceipt(receipt, { ...body, strategy_hash: strategyHash }));
      setMsg("");
    } catch (e) {
      setMsg(`Not unlocked: ${e instanceof Error ? e.message : e}`);
    }
  }

  if (!receipt) {
    return <PageHero eyebrow={`Agent ${token}`} title={`Receipt ${index}`}><p>{msg || "Reading the receipt"}</p></PageHero>;
  }

  return (
    <>
      <PageHero eyebrow={`Agent ${token}`} title={`Receipt ${receipt.index}`}>
        <p className="row" style={{ gap: 24 }}>
          <Kind kind={receipt.label} />
          <span className="mono">{modelName(receipt.model)}</span>
          <span className="mono">{receipt.order_id.startsWith("acquire-") ? "acquisition decision" : `order ${receipt.order_id}`}</span>
        </p>
      </PageHero>

      <div className="wrap page-body">
        <div className="split">
          <div className="block">
            <h2>Checks run in your browser</h2>
            <div className="ticket">
              {steps.map((s, i) => (
                <div className={`check ${s.ok === true ? "ok" : s.ok === false ? "bad" : ""}`} key={i}>
                  <span className="mark">{s.ok === true ? "✓" : s.ok === false ? "✗" : "·"}</span>
                  <div><div className="t">{s.title}</div><div className="d">{s.detail}</div></div>
                </div>
              ))}
            </div>
            <p className="note">
              The hashes are recomputed with SHA-256 and the signer is recovered with ecrecover, then compared with the key registered on the contract.
              {receipt.kind === "Gateway" && " A gateway signature covers the request and response hashes but not the model name, so the model is confirmed from the signed request bytes."}
            </p>
            <p style={{ marginTop: 24 }}><Link href={`/agent/${token}`} className="arrow">Back to agent {token}</Link></p>
          </div>

          <div className="block">
            <h2>Reasoning</h2>
            {!opened ? (
              <div className="panel" style={{ display: "grid", gap: 20, justifyItems: "start" }}>
                <p className="dim" style={{ margin: 0 }}>The full reasoning is stored encrypted by the runner. It is released to the agent's owner or to a viewing-key holder, who proves the account with a signed message.</p>
                <button className="btn" onClick={unlock}>{accountId ? "Sign and unlock" : "Connect wallet to unlock"}</button>
                {msg && <p className="warn" style={{ margin: 0 }}>{msg}</p>}
              </div>
            ) : (
              <div className="panel" style={{ display: "grid", gap: 20 }}>
                <div><span className="mono faint">Decision</span><div>{opened.summary}</div></div>
                <div><span className="mono faint">Model reply</span><pre>{opened.content}</pre></div>
                {opened.reasoning && <div><span className="mono faint">Hidden reasoning</span><pre>{opened.reasoning}</pre></div>}
                <div><span className="mono faint">Request bytes{opened.strategy_redacted ? ", with the owner's strategy redacted" : ""}</span><pre>{opened.request_body}</pre></div>
                {opened.strategy && <div><span className="mono faint">Strategy, shown to the owner only</span><pre>{opened.strategy}</pre></div>}
              </div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}
