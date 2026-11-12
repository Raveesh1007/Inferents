# inferents

Your agent's unused thinking becomes its trading money. Think less, trade bigger.

Each NFT is an AI trading agent that owns its own Hyperliquid account through NEAR Chain Signatures. It pays for its own thinking from credits; whatever it does not spend becomes trading margin. Agents compete on a public leaderboard, every decision carries a signed receipt you can check in your browser, and any agent can be bought at its live price.

**Hosted demo:** _add the Vercel URL here after deploying `web/`_ · **Runner API:** _add the hosted API URL here_ · Testnet only (Hyperliquid testnet, NEAR testnet, mock USDC), with one mainnet Intents swap as evidence below.

## How it works

```
 you ──USDC──▶ NEAR contract ──────────────┐   mint: strategy hash, model, 15% credits / 85% margin
        (reserve ≥ credits + treasury)     │
                                           ▼
   ┌──────────────── runner (Rust) ────────────────┐        ┌───────────────────────────┐
   │ per agent, every cycle:                       │        │ NEAR AI Cloud (TEE)       │
   │  read Hyperliquid ─▶ ask the agent's model ───┼───────▶│ model-verified: the model │
   │  risk limits ─▶ trade-only key places order   │        │ signs its reply           │
   │  charge real cost ─▶ report_usage             │◀───────│ gateway-verified: the     │
   │  store receipt ─▶ submit_receipt (ecrecover)  │        │ gateway signs the relay   │
   │  report_equity ─▶ live price on chain         │        └───────────────────────────┘
   └───────────────┬───────────────────────────────┘
                   │ approveAgent / withdraw3 / usdClassTransfer signed by the contract via Chain Signatures
                   ▼
        Hyperliquid account owned by the NFT      epoch end: unused credits burn ─▶ Intents ─▶ margin
```

- **Mint.** Pick a model, write a strategy (only its SHA-256 goes on-chain), choose the deposit split. The contract holds the USDC as a 1:1 reserve.
- **Think and trade.** Each cycle the agent asks its model for a decision, applies the risk limits and trades with a trade-only key. The real cost of the call is deducted from its credits.
- **Receipts.** Every order stores the model's signed receipt on NEAR, verified with `ecrecover`. The browser recomputes the hashes and the signer.
- **Convert.** At epoch end unused credits are burned and the matching USDC becomes margin through NEAR Intents.
- **Own, sell, acquire.** Selling the NFT sells the agent, its money and its record, at a live price: account value + unused credits + 20% of realised profit. A seller is paid the price less a 5% royalty to the minter. An agent whose owner set an acquisition budget can buy another agent with its own credits, once per epoch.
- **Viewing keys.** The owner sells time-limited keys that unlock an agent's full reasoning.

## Try it (hosted demo)

0. **Start on the landing page.** Scroll through the 3D explainer: one particle field shows the deposit, the split into thinking credits and margin, signed decisions, the end-of-epoch conversion and ownership. The receipt panel on that page recovers a real signature in your browser.
1. **Open the Arena** (`/arena`). See 8 testnet agents ranked by P&L, each with its model, a verified badge, equity, thinking spent, **return on thought** (P&L ÷ thinking spent), trades and live price. Click a column to sort. Six run different models (DeepSeek, Qwen, GLM, Claude, GPT, Gemini) and different strategies.
2. **Open an agent.** Live positions come straight from Hyperliquid's public API, credits burn down as it thinks, and the price is read from the contract.
3. **Verify a receipt.** Open any receipt → the page recomputes the signature in your browser with `ecrecover` and compares the signer with the key registered on the contract. Open-model receipts say **model-verified**; Claude, GPT and Gemini receipts say **gateway-verified, provider not attested** (see the trust model).
4. **Unlock reasoning.** Buy a $1 viewing key (mock USDC) on an agent page, then on a receipt press *Sign and unlock*. A NEP-413 signature proves the account; you get the reasoning and the exact request bytes, and the page checks that the signed request names the model. Use a popup wallet (Meteor) for message signing.
5. **Mint your own.** *Mint an agent*: write a strategy, pick a model (cost per decision is shown), move the split slider, choose how to pay. SOL / ETH / BTC show a live 1Click dry quote; on testnet the payment completes with mock USDC and is labelled simulated.
6. **Buy an agent.** On an agent page press *Buy at live price*. The runner first reports the agent's account value on chain (the contract only quotes a price from a report under two minutes old), then your payment transfers the NFT, flags the trading key for rotation and pays the seller and the minter.

## Trust model

- **The runner is trusted for usage and account-value reports.** It reports what each model call cost (capped per request on-chain, replays rejected) and each agent's Hyperliquid account value. A dishonest runner could misstate either; the request ids and receipts are on-chain so reports can be audited, and the account value can be checked against Hyperliquid's public API.
- **The runner holds the vault key and can read every strategy and every reasoning trace.** Strategies and reasoning are encrypted at rest and served only to the NFT owner (strategy) or to the owner and viewing-key holders (reasoning) after a NEP-413 signature, but that is access control by the runner, not cryptography against it. This is the same trust as the usage and account-value reports.
- **The runner holds trade-only agent keys.** They can trade and set leverage; they cannot withdraw, transfer or approve agents. Only the contract, through the MPC, can approve keys or withdraw, and the NFT owner alone can trigger a withdrawal. The runner chooses the Intents deposit address when converting credits.
- **Gateway receipts do not attest the provider.** For Claude, GPT and Gemini, NEAR AI's TEE gateway signs `request_hash:response_hash` for a reply relayed from a provider that is not attested (`x-serving-provider: non-attested`). The signature does not contain the model name; the model is confirmed from the signed request bytes, which hash to `request_hash`. We did **not** verify the gateway's TEE quote, only that its signing key matches the key in the gateway attestation.
- **A receipt is stored once and only for a charged request.** The contract rejects a receipt whose request id was never billed through `report_usage`, and rejects a second receipt for the same request.
- **Model-verified receipts** come from models NEAR AI serves inside its own TEE with an ECDSA key; the contract checks them with `ecrecover` against the registered key.
- **Hyperliquid positions are public.** Anyone can link an agent NFT to its Hyperliquid address. What stays private is the strategy text and the full reasoning.
- **Viewing-key holders do not see the owner's strategy text.** The strategy is redacted from the request bytes they receive, so only the owner can recompute the full request hash.
- **Intents is simulated on testnet.** NEAR Intents is mainnet-only. On testnet a funder wallet pays the agent's Hyperliquid account where Intents would, and the runner logs `SIMULATED`. The real route was verified once on mainnet (below).
- **Credits are not a general AI API.** They live inside the agent NFT and cannot be sent elsewhere; the protocol is the NEAR AI customer.

## Evidence

| Go/no-go test | Result |
| --- | --- |
| 1. Contract signs Hyperliquid `approveAgent` via Chain Signatures | Pass. Accepted for NFT #1's derived address. |
| 2. Approved agent key places and cancels a ≥$10 order | Pass. $12.10 BTC bid rested, then cancelled. |
| 3. Contract signs `withdraw3` | Pass. NFT balance dropped by the withdrawn amount. |
| 4. NEAR AI receipt and cost, verified on NEAR | Pass. A `provider_tee` signature recovers to the TEE-attested key on-chain; cost read from the billing API. |
| 5. Credit-to-margin path (mainnet, tiny amount) | Pass for the Intents leg: 2 USDC swapped through 1Click to HyperCore USDC and landed on Hyperliquid mainnet in about a minute (2.0 in, 1.6847 out). |
| Gateway receipts | Pass. A Claude Sonnet 5.5 reply's gateway signature recovers to the gateway attestation key and verifies in the contract. |

Live end-to-end checks on testnet: `node scripts/e2e.mjs <path to near CLI> [sale|key|acquire|withdraw]`. With the runner API running, it has a second account buy an agent at its live price, buy a viewing key and unlock reasoning with a real NEP-413 signature (and be refused for an agent it has no key for), has one agent buy another from its own credits, and withdraws the held balances, asserting the result on chain after each step. All checks passed on 12 Nov 2026.

Contract tests: `cargo test -p inferents-contract` (reserve invariant, receipts of both kinds, buying, acquisition, held payments, migration).

## Tracks

NFTs That Do Something (lead: the NFT is an agent that owns money, a record and a live price) · Unstoppable DeFi (Hyperliquid perps, Chain Signatures, Intents) · Agent Economy (agents pay for their own compute and can buy each other) · Privacy & Private AI (TEE receipts; reasoning and strategy kept private).

## Reference

## Status

| Go/no-go test | Result |
| --- | --- |
| 1. Contract signs Hyperliquid `approveAgent` via Chain Signatures | Pass. Hyperliquid testnet accepted it for NFT #1's derived address. |
| 2. Approved agent key places and cancels a ≥$10 order | Pass. $12.10 BTC bid rested, then cancelled. |
| 3. Contract signs `withdraw3` | Pass. Accepted; NFT balance dropped by the withdrawn amount. |
| 4. NEAR AI receipt and cost, verified on NEAR | Pass. A `provider_tee` signature over the exact request/response bytes recovers to the TEE-attested key on-chain; cost read from the billing API (60,300 nano-USD). |
| 5. Credit-to-margin path (mainnet, tiny amount) | Pass for the Intents leg. 2 USDC (Arbitrum) swapped through 1Click to HyperCore USDC and landed on Hyperliquid mainnet in about a minute: 2.0 in, 1.6847 out, ~$0.32 fixed spread, minimum swap about $1.31. Origin was Arbitrum, not a NEAR contract (no NEAR funds available); the contract-side `ft_transfer` is covered by the e2e with a simulated deposit. |
| 6. NEAR AI terms check | Not started |

The contract also signs `usdClassTransfer` (spot to perps), needed because transfers into a fresh Hyperliquid account land in spot.

## Testnet deployment

| What | Account |
| --- | --- |
| Agent contract | `inferents-11111630.testnet` |
| Mock USDC (6 decimals, anyone can `mint`; stands in for Circle USDC) | `usdc.inferents-11111630.testnet` |
| MPC signer | `v1.signer-prod.testnet` |

Verified on testnet: mint an agent with `ft_transfer_call` (`{"action":"mint","owner":"alice.testnet","max_leverage":3,"model":"deepseek-ai/DeepSeek-V4.1-Flash-1017","strategy_hash":"<sha256 hex>"}`; every message names its `owner`, and a payment that cannot be applied is kept as a refundable balance for that owner, withdrawn with `withdraw_balance`), the contract derives the agent's Hyperliquid address from the MPC key, runner usage reports debit credits (replays rejected), and `release_margin` after the epoch sends the unused credits out and keeps reserve >= credits + treasury.

Deposit split: a mint deposit is divided at once into thinking credits and trading margin. The default is 10% credits, 90% margin; set it with `credits_bps` in the mint message (the seeded arena agents use 1500).  The runner's next pass calls `fund_margin`, which sends the margin share to the agent's Hyperliquid account without waiting for the epoch to end. Credits are spent on thinking, and what is left of them still becomes margin at epoch end.

Defaults chosen during the build (change with owner calls): resale royalty 5% to the minter, viewing-key revenue to the agent's credits, epoch 7 days (60 s in the testnet demo), minimum mint $20 (1 USDC on testnet).

## Try it

Requires Rust (pinned by `rust-toolchain.toml`), `cargo-near` and `near` CLI.

```sh
cargo test -p inferents-contract                      # EIP-712 digest built on-chain matches alloy
cp .env.example .env
cargo run -p inferents-runner --bin gonogo_approve -- prepare 1
# -> {"hl_address": "0xfeef…a446", "agent_address": "0x…"}   (NFT #1's Hyperliquid account)
near contract call-function as-transaction inferents-11111630.testnet approve_agent \
  json-args '{"token_id":"1","agent_address":"<agent_address>"}' prepaid-gas '300 Tgas' \
  attached-deposit '0 NEAR' sign-as <owner> network-config testnet sign-with-legacy-keychain send
# save the returned JSON to signed.json
cargo run -p inferents-runner --bin gonogo_approve -- submit 1 signed.json
# -> recovered == derived; Hyperliquid answers {"status":"ok"}
cargo run -p inferents-runner --bin gonogo_order -- 1
# -> order resting, then cancelled, signed by the approved agent key
cargo run -p inferents-runner --bin gonogo_receipt   # needs NEAR_AI_API_KEY in .env
# -> recovered signer == attested signing_address, cost in nano-USD
```

On-chain receipts: `near contract call-function as-read-only inferents-11111630.testnet receipts json-args '{"from_index":0,"limit":10}' network-config testnet now`

## Implementation notes

- The contract builds the EIP-712 hash itself (`contract/src/hl.rs`), so the MPC only ever signs actions the contract constructed.
- The `approveAgent` nonce is the block timestamp in ms, chosen by the contract.
- Windows builds: `cargo near build non-reproducible-wasm --no-abi` (ABI step fails to link with MSVC); `.cargo/config.toml` sets `AWS_LC_SYS_PREBUILT_NASM`.

## Runner

Rust service (`runner/`) that runs the agents. Config comes from `.env` (see `.env.example`).

```sh
cargo run -p inferents-runner --bin inferents-runner -- funder          # testnet funder wallet address
cargo run -p inferents-runner --bin inferents-runner -- fund 2 50      # give agent 2 $50 testnet margin
cargo run -p inferents-runner --bin inferents-runner -- tick 2         # one think/trade/report pass
cargo run -p inferents-runner --bin inferents-runner -- show 2         # latest decision from the encrypted vault
cargo run -p inferents-runner --bin inferents-runner -- serve          # agent loop plus the HTTP API (API_PORT, default 8787)
cargo run -p inferents-runner --bin inferents-runner -- api            # the HTTP API only: nothing thinks, nothing is charged
cargo run -p inferents-runner --bin inferents-runner -- set-strategy 3 seed/momentum.txt   # store a strategy; prints its hash
```

Per pass the runner: rotates the trade-only key if the NFT changed hands, flattens before the epoch ends, converts unused credits to margin after it ends, otherwise reads the market, asks the TEE-verified model, applies the risk limits, trades, charges the thinking cost on NEAR and stores the signed receipt.

Verified on testnet:
- Agent 2 (10 USDC of credits) opened a real BTC short sized by its margin and leverage cap; usage was charged, the receipt verified on-chain against the attested model key, and credits + treasury stayed equal to the reserve.
- Agent 1's epoch ended: its unused credits were burned and paid into its Hyperliquid account, then moved to perps (perps balance 44 to 49).

Choices made during the build:
- **Testnet conversion is simulated.** NEAR Intents is mainnet-only, so on testnet a funder wallet pays the agent's Hyperliquid account where Intents would. The runner logs `SIMULATED`. `CONVERSION=live` uses the real 1Click flow (verified separately on mainnet with a 2 USDC swap).
- **Coins with a stale oracle are skipped.** Hyperliquid rejects orders priced far from the oracle, and some testnet oracles lag the book.
- **Thinking is charged before trading,** because the model call is paid for either way. A rejected order becomes a hold.
- **The prompt asks the model to trade actively** (follow the hourly trend, hold only when flat); the risk limits, not the prompt, bound the downside.

`serve` gives each agent its own schedule (`LOOP_SECS`, default 1800). The time of an agent's last pass is kept in the vault, so restarting the runner does not make every agent think, and pay, again; a failed pass is retried after five minutes.

**NFT image.** Each token's `media` is `{media_base}/nft/{id}.svg`, a card the runner draws on request from the agent's live state: model, live price, account value, P&L, credits left and open position, lit in a colour per model family. The owner sets the base with `set_media_base` (currently `http://localhost:8787`; switch it to the hosted runner URL so wallets can load it). The live price shown everywhere uses the contract's formula on the live Hyperliquid account value, which is what a buyer is quoted once the runner reports it on chain.

HTTP API: `GET /nft/{id}.svg`, `GET /activity`, `GET /history`, `GET /agents`, `GET /agents/{id}`, `GET /leaderboard`, `GET /receipts/{id}`, `GET /reasoning/{token}/{request_id}` (NEP-413 signature in `x-nep413`), `POST /agents/{id}/refresh` (reports account value on chain before a buy, at most once per 30 s per agent), `POST /strategy/{token}` (owner uploads the strategy text; accepted only if it hashes to `strategy_hash`).

## Frontend

`web/` is a Next.js app: a landing page (`/`), the arena (`/arena`), agent pages, mint and a receipt verifier. `cd web && npm install && npm run dev`, with `NEXT_PUBLIC_API` pointing at the runner API (default `http://localhost:8787`). The hero and the explainer are Three.js; the footer painting is one of the free backgrounds published by @uixhassan.

## Models

Each agent thinks with the model fixed at mint (`model` in the mint message). A model can be minted only if the contract has a signing key for it or it is registered as a gateway model. For a provider-TEE model NEAR AI serves it inside its own TEE with an ECDSA `signing_address`, because the contract verifies each receipt by `ecrecover` against that key. Five such chat models are registered on the contract: DeepSeek-V4.1-Flash, Qwen3.6-35B-A3B, Qwen3.8-27B, Qwen3-VL-30B-A3B and GLM-5.3-flash. DeepSeek-V3.2, Kimi K2.6/K3 and GLM-5.3 run on third-party TEE providers whose attestation has no such key, so their receipts cannot be verified this way. The two abliterated variants of DeepSeek and Qwen also expose keys and are registered.

Two receipt kinds, shown as labels:
- **model-verified** (`ProviderTee`): the model ran inside NEAR AI's TEE and signed `model:request_hash:response_hash`.
- **gateway-verified, provider not attested** (`Gateway`): Claude, GPT and Gemini. NEAR AI's TEE gateway signed `request_hash:response_hash` for a reply relayed from the provider (`x-serving-provider: non-attested`). The signature does not contain the model name, so the runner keeps the exact request bytes in the vault: they hash to `request_hash` and their `"model"` field names the model. The gateway key is set with `set_gateway_signing_address`, separate from the per-model keys.

Closed models get a larger default token budget (`MAX_TOKENS` overrides it). `cargo run -p inferents-runner --bin gatewayprobe -- survey` lists which models expose a signing key; `gatewayprobe <model>` tests one.

`cargo run -p inferents-runner --bin modelcompare [model ...]` gives every model the same market snapshot and prints verification, latency, cost and decision. In one run all five verified; they differ in cost per decision (84 to 1,639 micro-USD) and speed (3.5 to 28 s). Reasoning-heavy models can spend the whole token budget on hidden reasoning and return an empty reply (seen once with Qwen3.6 at 1,500 tokens), so DeepSeek stays the default and `MAX_TOKENS` should be raised when switching.
