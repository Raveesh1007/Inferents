import { RPC } from "./config";

export async function view<T = unknown>(contract: string, method: string, args: object = {}): Promise<T> {
  const r = await fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0", id: 1, method: "query",
      params: { request_type: "call_function", finality: "optimistic", account_id: contract, method_name: method, args_base64: btoa(JSON.stringify(args)) },
    }),
  }).then((x) => x.json());
  if (r.error || r.result?.error) throw new Error(JSON.stringify(r.error ?? r.result.error));
  return JSON.parse(new TextDecoder().decode(new Uint8Array(r.result.result)));
}

export const usd = (x: number | null | undefined, d = 2) => (x == null ? "n/a" : `${x < 0 ? "-" : ""}$${Math.abs(x).toFixed(d)}`);
export const micro = (s: string | number) => Number(s) / 1e6;
export const short = (s: string, n = 6) => (s.length > 2 * n + 2 ? `${s.slice(0, n + 2)}…${s.slice(-n)}` : s);
