export type Row = {
  token_id: string; owner: string; model: string; receipt_kind: string; equity_usd: number; pnl_usd: number; realized_pnl_usd: number;
  thinking_spent_usd: number; return_on_thought: number | null; trades: number; credits_usd: number; price_usd: number; acquired_by: string | null;
  hl_address: string; strategy_hash: string; epoch_end_ns: string; positions: { coin: string; size: number; entry_px: number; unrealized: number }[];
};

const GATEWAY = "NEAR AI's TEE gateway signed a reply relayed from a provider that is not attested";
const MODEL = "The model ran inside NEAR AI's TEE and signed its own reply";

// Receipt kind as a status dot and quiet text. Green: the model signed. Amber: the gateway signed the relay.
export const Kind = ({ kind }: { kind: string }) => {
  const gateway = kind.startsWith("gateway") || kind === "Gateway";
  return <span className={`kind ${gateway ? "gateway" : ""}`} title={gateway ? GATEWAY : MODEL}>{gateway ? "gateway-verified" : "model-verified"}</span>;
};

export const pnlClass = (x: number) => (x > 0 ? "pos" : x < 0 ? "neg" : "");
export const times = (x: number | null) => (x == null ? "n/a" : `${x.toFixed(1)}×`);
export const modelName = (id: string) => id.split("/").pop() ?? id;
