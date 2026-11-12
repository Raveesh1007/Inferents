export const API = process.env.NEXT_PUBLIC_API ?? "http://localhost:8787";
export const CONTRACT = process.env.NEXT_PUBLIC_CONTRACT ?? "inferents-11111630.testnet";
export const USDC = process.env.NEXT_PUBLIC_USDC ?? "usdc.inferents-11111630.testnet";
export const NETWORK = "testnet";
export const RPC = process.env.NEXT_PUBLIC_RPC ?? "https://test.rpc.fastnear.com";
export const HL_INFO = "https://api.hyperliquid-testnet.xyz/info";
export const ONE_CLICK = "https://1click.chaindefuser.com/v0";
export const NEAR_USDC = "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

// Cost per decision (micro-USD) and latency measured by `modelcompare` on one market snapshot; GLM's cost is the average
// of its live arena decisions, and its latency was not measured.
export const MODELS: { id: string; label: string; kind: "model" | "gateway"; cost: number; secs: number | null }[] = [
  { id: "deepseek-ai/DeepSeek-V4.1-Flash-1017", label: "DeepSeek V4.1 Flash", kind: "model", cost: 369, secs: 8.3 },
  { id: "Qwen/Qwen3.8-27B", label: "Qwen 3.8 27B", kind: "model", cost: 916, secs: 17.8 },
  { id: "z-ai/glm-5.3-flash", label: "GLM 5.3 Flash", kind: "model", cost: 790, secs: null },
  { id: "anthropic/claude-sonnet-5-5", label: "Claude Sonnet 5.5", kind: "gateway", cost: 2736, secs: 11.4 },
  { id: "openai/gpt-5.5", label: "GPT 5.5", kind: "gateway", cost: 6845, secs: 5.4 },
  { id: "google/gemini-3.5-flash", label: "Gemini 3.5 Flash", kind: "gateway", cost: 7029, secs: 6.8 },
];
