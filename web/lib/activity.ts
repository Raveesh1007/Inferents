export type Activity = { token_id: string; model: string; summary: string; label: string | null; at_ns: string; receipt: boolean };
export type Verb = "long" | "short" | "close" | "hold" | "acquire" | "pass" | "fail" | "other";

// The runner logs each decision as a terse line ("SHORT SOL $76.50 at 5x, oid …"); people read a sentence.
export function describe(summary: string): { verb: Verb; text: string } {
  let m = summary.match(/^(LONG|SHORT) (\w+) \$([\d.]+) at (\d+)x/);
  if (m) return { verb: m[1] === "LONG" ? "long" : "short", text: `opened a $${m[3]} ${m[1].toLowerCase()} on ${m[2]} at ${m[4]}x` };
  if ((m = summary.match(/^CLOSE (\w+)/))) return { verb: "close", text: `closed its ${m[1]} position` };
  if ((m = summary.match(/^ACQUIRED agent (\d+) for \$([\d.]+)/))) return { verb: "acquire", text: `bought agent ${m[1]} for $${m[2]}` };
  if (summary.startsWith("PASS")) return { verb: "pass", text: "passed on buying another agent" };
  if (summary.startsWith("flattened")) return { verb: "close", text: "closed out before the epoch ended" };
  if (summary.startsWith("HOLD: margin")) return { verb: "hold", text: "waited: margin below the $10 order minimum" };
  if (summary.startsWith("HOLD: a position")) return { verb: "hold", text: "kept its open position" };
  if (summary.startsWith("HOLD")) return { verb: "hold", text: "held after reading the market" };
  if (summary.startsWith("ORDER FAILED") || summary.startsWith("ACQUISITION FAILED")) return { verb: "fail", text: "tried to trade; the exchange rejected the order" };
  return { verb: "other", text: summary };
}

export function ago(at_ns: string, now = Date.now()) {
  const s = Math.max(0, Math.round((now - Number(BigInt(at_ns) / 1_000_000n)) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${Math.round(s / 3600)}h`;
  return `${Math.round(s / 86400)}d`;
}
