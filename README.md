# inferents

Your agent's unused thinking becomes its trading money. Think less, trade bigger.

Each NFT is an AI trading agent that owns its own Hyperliquid account through NEAR Chain Signatures.

## Status

| Go/no-go test | Result |
| --- | --- |
| 1. Contract signs Hyperliquid `approveAgent` via Chain Signatures | Pass. Hyperliquid testnet accepted it for NFT #1's derived address. |
| 2. Approved agent key places and cancels a ≥$10 order | Pass. $12.10 BTC bid rested, then cancelled. |
| 3. Contract signs `withdraw3` | Pass. Accepted; NFT balance dropped by the withdrawn amount. |
| 4. NEAR AI receipt and cost, verified on NEAR | Not started |
| 5. Credit-to-margin path (mainnet, tiny amount) | Not started |
| 6. NEAR AI terms check | Not started |

The contract also signs `usdClassTransfer` (spot to perps), needed because transfers into a fresh Hyperliquid account land in spot.

Testnet contract: `inferents-11111630.testnet` (MPC signer `v1.signer-prod.testnet`).

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
```

## Implementation notes

- The contract builds the EIP-712 hash itself (`contract/src/hl.rs`), so the MPC only ever signs actions the contract constructed.
- The `approveAgent` nonce is the block timestamp in ms, chosen by the contract.
- Windows builds: `cargo near build non-reproducible-wasm --no-abi` (ABI step fails to link with MSVC); `.cargo/config.toml` sets `AWS_LC_SYS_PREBUILT_NASM`.
