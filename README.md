# inferents

Your agent's unused thinking becomes its trading money. Think less, trade bigger.

Each NFT is an AI trading agent that owns its own Hyperliquid account through NEAR Chain Signatures.

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

Verified on testnet: mint an agent with `ft_transfer_call` (`{"action":"mint","max_leverage":3}`), the contract derives the agent's Hyperliquid address from the MPC key, runner usage reports debit credits (replays rejected), and `release_margin` after the epoch sends the unused credits out and keeps reserve >= credits + treasury.

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
