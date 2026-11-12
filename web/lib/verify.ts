import { recoverMessageAddress } from "viem";
import { CONTRACT } from "./config";
import { view } from "./near";

export type Receipt = {
  index: number; token_id: string; order_id: string; request_id: string; model: string;
  request_hash: string; response_hash: string; signature: string; verified: boolean; kind?: "ProviderTee" | "Gateway"; label: string;
};
export type Step = { ok: boolean | null; title: string; detail: string };

export async function sha256Hex(s: string) {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// A model signs `model:req:resp`; the NEAR AI gateway signs `req:resp`.
export const signedText = (r: Receipt) => (r.kind === "Gateway" ? `${r.request_hash}:${r.response_hash}` : `${r.model}:${r.request_hash}:${r.response_hash}`);

// Everything checkable from chain data alone, then (once the bytes are unlocked) the checks that tie the hashes to real content.
export async function checkReceipt(
  r: Receipt,
  bytes?: { request_body?: string; response_body?: string; strategy?: string | null; strategy_hash?: string; strategy_redacted?: boolean },
): Promise<Step[]> {
  const steps: Step[] = [];
  const text = signedText(r);
  let recovered = "";
  try {
    recovered = await recoverMessageAddress({ message: text, signature: r.signature as `0x${string}` });
    steps.push({ ok: true, title: "Signature recovers (ecrecover)", detail: recovered });
  } catch (e) {
    steps.push({ ok: false, title: "Signature recovers (ecrecover)", detail: String(e) });
  }
  const expected =
    r.kind === "Gateway"
      ? await view<string | null>(CONTRACT, "gateway_signing_address")
      : await view<string | null>(CONTRACT, "signing_address", { model: r.model });
  steps.push({
    ok: !!recovered && !!expected && recovered.toLowerCase() === expected.toLowerCase(),
    title: r.kind === "Gateway" ? "Signer is the registered NEAR AI gateway key" : "Signer is the registered key for this model",
    detail: `registered ${expected ?? "none"}`,
  });
  steps.push({ ok: r.verified, title: "The contract verified it when it was submitted", detail: r.label });
  if (bytes?.request_body && bytes.response_body) {
    const redacted = !!bytes.strategy_redacted;
    steps.push(
      redacted
        ? { ok: null, title: "Request bytes hash to request_hash", detail: "The owner's strategy text is redacted for viewing-key holders, so only the owner can recompute this hash." }
        : { ok: (await sha256Hex(bytes.request_body)) === r.request_hash, title: "Request bytes hash to request_hash", detail: r.request_hash },
    );
    steps.push({ ok: (await sha256Hex(bytes.response_body)) === r.response_hash, title: "Response bytes hash to response_hash", detail: r.response_hash });
    let named = "";
    try { named = JSON.parse(bytes.request_body).model; } catch {}
    steps.push({ ok: redacted ? null : named === r.model, title: "The signed request names this model", detail: `request "model": ${named || "unreadable"}${redacted ? " (not tied to the hash while redacted)" : ""}` });
    if (bytes.strategy && bytes.strategy_hash) {
      const inRequest = bytes.request_body.includes(JSON.stringify(bytes.strategy).slice(1, -1));
      steps.push({ ok: (await sha256Hex(bytes.strategy)) === bytes.strategy_hash && inRequest, title: "Strategy matches the on-chain commitment and is in the request", detail: bytes.strategy_hash });
    }
  } else {
    steps.push({ ok: null, title: "Request and response bytes", detail: "Locked. A viewing key, or owning the agent, unlocks the bytes and the reasoning." });
  }
  return steps;
}
