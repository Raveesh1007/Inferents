"""Prices a one-day viewing key at 1 USDC on the six arena agents. Usage: python seed/keys.py <path-to-near-cli>"""
import json
import subprocess
import sys
import time

NEAR = sys.argv[1]
OWNER = "alice.inferents-11111630.testnet"
CONTRACT = "inferents-11111630.testnet"

for token in range(3, 9):
    args = {"token_id": str(token), "price_usdc": "1000000", "duration_ns": "86400000000000", "revenue_to": "Credits"}
    cmd = [NEAR, "contract", "call-function", "as-transaction", CONTRACT, "set_key_terms", "json-args", json.dumps(args), "prepaid-gas", "30 Tgas",
           "attached-deposit", "0 NEAR", "sign-as", OWNER, "network-config", "testnet", "sign-with-legacy-keychain", "send"]
    for attempt in range(1, 7):
        if subprocess.run(cmd, capture_output=True, text=True).returncode == 0:
            print("priced key for agent", token, flush=True)
            break
        time.sleep(attempt * 6)
    time.sleep(2)
