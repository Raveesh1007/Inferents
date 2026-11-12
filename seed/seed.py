"""Mints the six arena agents on testnet. Usage: python seed/seed.py <path-to-near-cli>

Each agent gets 20 mock USDC, 15% as thinking credits and 85% as trading margin. The runner funds the margin on its first pass.
"""
import hashlib
import json
import pathlib
import subprocess
import sys
import time

NEAR = sys.argv[1]
CONTRACT = "inferents-11111630.testnet"
USDC = "usdc.inferents-11111630.testnet"
OWNER = "alice.inferents-11111630.testnet"
DEPOSIT = 20_000_000
HERE = pathlib.Path(__file__).parent

AGENTS = [
    ("momentum", "deepseek-ai/DeepSeek-V4.1-Flash-1017", 3),
    ("funding_fade", "Qwen/Qwen3.8-27B", 3),
    ("mean_reversion", "z-ai/glm-5.3-flash", 3),
    ("cautious", "anthropic/claude-sonnet-5-5", 3),
    ("aggressive", "openai/gpt-5.5", 5),
    ("copies_the_leader", "google/gemini-3.5-flash", 3),
]


def call(method, args, signer, receiver, deposit="0 NEAR", gas="30 Tgas"):
    cmd = [NEAR, "contract", "call-function", "as-transaction", receiver, method, "json-args", json.dumps(args), "prepaid-gas", gas,
           "attached-deposit", deposit, "sign-as", signer, "network-config", "testnet", "sign-with-legacy-keychain", "send"]
    for attempt in range(1, 7):
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode == 0:
            time.sleep(2)
            return
        time.sleep(attempt * 6)
    raise SystemExit(f"{method} failed: {r.stdout[-400:]} {r.stderr[-400:]}")


for name, model, leverage in AGENTS:
    text = (HERE / f"{name}.txt").read_text()
    msg = {"action": "mint", "owner": OWNER, "max_leverage": leverage, "credits_bps": 1500, "model": model,
           "strategy_hash": hashlib.sha256(text.encode()).hexdigest()}
    call("mint", {"account_id": OWNER, "amount": str(DEPOSIT)}, OWNER, USDC)
    call("ft_transfer_call", {"receiver_id": CONTRACT, "amount": str(DEPOSIT), "msg": json.dumps(msg)}, OWNER, USDC, deposit="1 yoctoNEAR", gas="300 Tgas")
    print("minted", name, model, msg["strategy_hash"][:12], flush=True)
