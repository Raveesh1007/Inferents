"use client";
import { useEffect, useState } from "react";
import { recoverMessageAddress } from "viem";
import { short } from "@/lib/near";
import { Kind } from "@/lib/ui";

// Two real receipts from NEAR AI, captured while building this (the same ones the contract tests verify).
// The check below is not a picture of a check: your browser recovers the signer from the signature right now.
const RECEIPTS = {
  model: {
    kind: "model-verified",
    model: "deepseek-ai/DeepSeek-V4.1-Flash-1017",
    request_hash: "cc5d9658257aa1f871c69f787262d5781482af70cac006379bd4f693caac3053",
    response_hash: "5d37fb4f38040df6cc6f27da1f2815f992d1f5def6fa5babfc42b9da46283983",
    signature: "0xd472a8d834fd1b0d3b9f47de2df563e380c02a8dcf10ac139c714be22693a73071107eaf5d4afd848150c5d401f4cbe4f55edf25ae8dd310bb1d518e19b3792a1b",
    registered: "0xfe0071895500a9755c18044948ab4f4e1a7f337e",
    note: "The model ran inside NEAR AI's TEE and signed its own reply, so the signed text includes the model name.",
  },
  gateway: {
    kind: "gateway-verified",
    model: "anthropic/claude-sonnet-5-5",
    request_hash: "20f3fafffe5c970b7feb927a831d1a4ad1e9837fffe8157990ff1c9f14ad465c",
    response_hash: "ab1436150ae3df1427aad173a52ac01bcd0ac98ee1748f37cff3c8d6fb56d4bf",
    signature: "0x5fa602ca08253bf3ef5bfac718297f748c82abdfac6160b7ebe21852ce8d27fd54a7878942952e4ebcb9558cf4a809f29ff8354a194122af1f53ed9afd1dc6fa1b",
    registered: "0xb98a789cdc4817275f5155593ba73ef2a957e779",
    note: "NEAR AI's TEE gateway signed a reply relayed from the provider, which is not attested. The signature covers the two hashes only, so the model is confirmed from the signed request bytes.",
  },
} as const;
type Which = keyof typeof RECEIPTS;

export function ReceiptDemo() {
  const [which, setWhich] = useState<Which>("model");
  const [recovered, setRecovered] = useState<string | null>(null);
  const r = RECEIPTS[which];
  const text = which === "gateway" ? `${r.request_hash}:${r.response_hash}` : `${r.model}:${r.request_hash}:${r.response_hash}`;

  useEffect(() => {
    let live = true;
    setRecovered(null);
    recoverMessageAddress({ message: text, signature: r.signature }).then((a) => live && setRecovered(a.toLowerCase())).catch(() => live && setRecovered(""));
    return () => { live = false; };
  }, [text, r.signature]);

  const match = recovered === r.registered;
  return (
    <div className="ticket">
      <div className="ticket-head">
        <Kind kind={r.kind} />
        <div className="tabs" role="tablist">
          <button role="tab" aria-selected={which === "model"} className={which === "model" ? "on" : ""} onClick={() => setWhich("model")}>Open model</button>
          <button role="tab" aria-selected={which === "gateway"} className={which === "gateway" ? "on" : ""} onClick={() => setWhich("gateway")}>Claude, via gateway</button>
        </div>
      </div>
      <div className="check ok"><span className="mark">·</span><div><div className="t">Model</div><div className="d">{r.model}</div></div></div>
      <div className="check ok"><span className="mark">·</span><div><div className="t">Signed text</div><div className="d">{which === "gateway" ? "" : `${r.model}:`}{short(r.request_hash, 10)}:{short(r.response_hash, 10)}</div></div></div>
      <div className={`check ${recovered ? "ok" : recovered === "" ? "bad" : ""}`}>
        <span className="mark">{recovered ? "✓" : recovered === "" ? "✗" : "·"}</span>
        <div><div className="t">Signature recovers in your browser</div><div className="d">{recovered ?? "recovering"}</div></div>
      </div>
      <div className={`check ${recovered === null ? "" : match ? "ok" : "bad"}`}>
        <span className="mark">{recovered === null ? "·" : match ? "✓" : "✗"}</span>
        <div><div className="t">Signer is the key registered on the contract</div><div className="d">{r.registered}</div></div>
      </div>
      <div className="check"><span className="mark">·</span><div><div className="t dim">{r.note}</div></div></div>
    </div>
  );
}
