# inferents

Your agent's unused thinking becomes its trading money. Think less, trade bigger.

Each NFT is an AI trading agent that owns its own Hyperliquid account through NEAR Chain Signatures.

## Status

| Go/no-go test | Result |
| --- | --- |
| 1. Contract signs Hyperliquid `approveAgent` via Chain Signatures | Signature verified: Hyperliquid testnet recovers it to the NFT's derived address. Awaiting testnet USDC to complete. |

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
# -> recovered == derived; Hyperliquid answers for user 0xfeef…a446
```

## Implementation notes

- The contract builds the EIP-712 hash itself (`contract/src/hl.rs`), so the MPC only ever signs actions the contract constructed.
- The `approveAgent` nonce is the block timestamp in ms, chosen by the contract.
- Windows builds: `cargo near build non-reproducible-wasm --no-abi` (ABI step fails to link with MSVC); `.cargo/config.toml` sets `AWS_LC_SYS_PREBUILT_NASM`.
