// End-to-end checks of the paths unit tests cannot prove, run for real on testnet.
// usage: node scripts/e2e.mjs <path to near CLI> [sale|key|acquire|withdraw ...]   (default: all, in that order)
//
// Accounts: alice owns the seeded agents; funder plays a second, independent owner. Both sign with their
// legacy-keychain keys in ~/.near-credentials/testnet. Payments use mock USDC, minted in the same transaction.
import { execFileSync } from "node:child_process";
import { createHash, createPrivateKey, randomBytes, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";

const NEAR = process.argv[2];
const steps = process.argv.slice(3).length ? process.argv.slice(3) : ["sale", "key", "acquire", "withdraw"];
const CONTRACT = "inferents-11111630.testnet";
const USDC = "usdc.inferents-11111630.testnet";
const ALICE = "alice.inferents-11111630.testnet";
const FUNDER = "funder.inferents-11111630.testnet";
const RPC = "https://test.rpc.fastnear.com";
const API = "http://localhost:8787";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let failures = 0;
const check = (ok, what, detail = "") => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${what}${detail ? `  (${detail})` : ""}`);
  if (!ok) failures++;
};

async function view(method, args = {}, account = CONTRACT) {
  const body = { jsonrpc: "2.0", id: 1, method: "query", params: { request_type: "call_function", finality: "final", account_id: account, method_name: method, args_base64: Buffer.from(JSON.stringify(args)).toString("base64") } };
  for (let i = 0; i < 5; i++) {
    const r = await fetch(RPC, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) }).then((x) => x.json());
    if (r.result?.result) return JSON.parse(Buffer.from(r.result.result).toString());
    await sleep(1500);
  }
  throw new Error(`view ${method} failed`);
}

// One transaction with several function calls to the same receiver, retried on RPC rate limits.
function tx(signer, receiver, calls) {
  const actions = calls.flatMap((c) => ["add-action", "function-call", c.method, "json-args", JSON.stringify(c.args), "prepaid-gas", c.gas ?? "30 Tgas", "attached-deposit", c.deposit ?? "0 NEAR"]);
  const single = calls.length === 1;
  const argv = single
    ? ["contract", "call-function", "as-transaction", receiver, calls[0].method, "json-args", JSON.stringify(calls[0].args), "prepaid-gas", calls[0].gas ?? "30 Tgas", "attached-deposit", calls[0].deposit ?? "0 NEAR", "sign-as", signer, "network-config", "testnet", "sign-with-legacy-keychain", "send"]
    : ["transaction", "construct-transaction", signer, "receiver-id", receiver, ...actions, "skip", "network-config", "testnet", "sign-with-legacy-keychain", "send"];
  for (let i = 1; i <= 6; i++) {
    try {
      return execFileSync(NEAR, argv, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
    } catch (e) {
      const out = `${e.stdout ?? ""}${e.stderr ?? ""}`;
      if (!/rate limit|nonce|timeout|Timeout|429/i.test(out) || i === 6) throw new Error(`${calls.map((c) => c.method).join("+")} failed: ${out.replace(/\x1b\[[0-9;]*m/g, "").slice(-600)}`);
    }
    execFileSync(process.execPath, ["-e", `setTimeout(()=>{}, ${i * 5000})`]);
  }
}

const pay = (signer, amount, msg) =>
  tx(signer, USDC, [
    { method: "mint", args: { account_id: signer, amount } },
    { method: "ft_transfer_call", args: { receiver_id: CONTRACT, amount, msg: JSON.stringify(msg) }, deposit: "1 yoctoNEAR", gas: "250 Tgas" },
  ]);

// NEP-413: sha256(borsh(u32 2^31+413) ++ borsh({message, nonce, recipient, callbackUrl: None})), signed with ed25519.
const B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const b58decode = (s) => {
  let n = 0n;
  for (const ch of s) n = n * 58n + BigInt(B58.indexOf(ch));
  const hex = n.toString(16).padStart(2, "0");
  const bytes = Buffer.from(hex.length % 2 ? `0${hex}` : hex, "hex");
  const zeros = s.match(/^1*/)[0].length;
  return Buffer.concat([Buffer.alloc(zeros), bytes]);
};
const borshString = (s) => { const b = Buffer.from(s); const len = Buffer.alloc(4); len.writeUInt32LE(b.length); return Buffer.concat([len, b]); };

function nep413(account, message) {
  const cred = JSON.parse(readFileSync(`${homedir()}/.near-credentials/testnet/${account}.json`, "utf8"));
  const seed = b58decode(cred.private_key.replace("ed25519:", "")).subarray(0, 32);
  const key = createPrivateKey({ key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]), format: "der", type: "pkcs8" });
  const nonce = randomBytes(32);
  const tag = Buffer.alloc(4); tag.writeUInt32LE(2 ** 31 + 413);
  const payload = Buffer.concat([tag, borshString(message), nonce, borshString("inferents-reasoning"), Buffer.from([0])]);
  const signature = sign(null, createHash("sha256").update(payload).digest(), key);
  return Buffer.from(JSON.stringify({ accountId: account, publicKey: cred.public_key, signature: signature.toString("base64"), message, nonce: nonce.toString("base64") })).toString("base64");
}

async function reasoning(account, token, requestId) {
  const header = nep413(account, JSON.stringify({ token, receipt: requestId, ts: Date.now() }));
  const res = await fetch(`${API}/reasoning/${token}/${requestId}`, { headers: { "x-nep413": header } });
  return { status: res.status, body: res.ok ? await res.json() : await res.text() };
}

async function invariant(label) {
  const a = await view("accounting");
  const ok = BigInt(a.reserve_usdc) >= BigInt(a.total_credits) + BigInt(a.treasury_usdc) + BigInt(a.refundable_usdc);
  check(ok, `reserve covers credits + treasury + refundable ${label}`, `${a.reserve_usdc} >= ${a.total_credits} + ${a.treasury_usdc} + ${a.refundable_usdc}`);
}

async function sale() {
  console.log("\n== Agent sale: funder buys agent 3 from alice at the live price");
  const before = await view("agent", { token_id: "3" });
  const aliceBefore = BigInt(await view("balance_of", { account: ALICE }));
  const funderBefore = BigInt(await view("balance_of", { account: FUNDER }));
  const q = await fetch(`${API}/agents/3/refresh`, { method: "POST" }).then((r) => r.json());
  const price = BigInt(q.price_micro_usd);
  const amount = (price * 102n) / 100n;
  console.log(`quoted ${Number(price) / 1e6} USDC; paying ${Number(amount) / 1e6} (2% buffer)`);
  pay(FUNDER, amount.toString(), { action: "buy", owner: FUNDER, token_id: "3" });
  const after = await view("agent", { token_id: "3" });
  check(before.owner === ALICE && after.owner === FUNDER, "NFT moved from alice to funder", `${before.owner} -> ${after.owner}`);
  check(after.needs_rotation === true, "trading key flagged for rotation");
  check(after.equity_ts_ns === "0", "equity report consumed, so a second buy needs a fresh quote");
  const royalty = (price * 500n) / 10_000n;
  const aliceGot = BigInt(await view("balance_of", { account: ALICE })) - aliceBefore;
  check(aliceGot === price, "alice (seller and minter) received price - 5% royalty + the royalty", `${aliceGot} = ${price - royalty} + ${royalty}`);
  const funderHeld = BigInt(await view("balance_of", { account: FUNDER })) - funderBefore;
  check(funderHeld === amount - price, "funder's overpayment is held as a refundable balance", `${funderHeld}`);
  await invariant("after the sale");
}

async function key() {
  console.log("\n== Viewing key: funder buys a key for agent 4 and unlocks its reasoning");
  const terms = await view("agent", { token_id: "4" });
  pay(FUNDER, terms.key_price_usdc, { action: "buy_key", owner: FUNDER, token_id: "4" });
  check((await view("has_access", { token_id: "4", account: FUNDER })) === true, "funder has access to agent 4", `${Number(terms.key_price_usdc) / 1e6} USDC for ${Number(terms.key_duration_ns) / 3.6e12} h`);

  const receipts = await fetch(`${API}/receipts/4`).then((r) => r.json());
  const latest = receipts[receipts.length - 1];
  const got = await reasoning(FUNDER, "4", latest.request_id);
  check(got.status === 200 && typeof got.body.content === "string", "key holder gets the reasoning", `HTTP ${got.status}`);
  check(got.body.strategy_redacted === true && !got.body.strategy && !String(got.body.request_body).includes("Funding fade"), "owner's strategy is redacted for the key holder");
  check(JSON.parse(got.body.request_body).model === latest.model, "request bytes name the receipt's model", latest.model);

  const r5 = await fetch(`${API}/receipts/5`).then((r) => r.json());
  const denied = await reasoning(FUNDER, "5", r5[r5.length - 1].request_id);
  check(denied.status === 403, "no key for agent 5, so access is refused", `HTTP ${denied.status}`);
  const wrong = await reasoning(FUNDER, "4", r5[r5.length - 1].request_id);
  check(wrong.status !== 200, "a signature for one receipt cannot open another agent's", `HTTP ${wrong.status}`);

  const owner = await reasoning(ALICE, "4", latest.request_id);
  const hash = createHash("sha256").update(owner.body.strategy ?? "").digest("hex");
  check(owner.status === 200 && hash === terms.strategy_hash, "the owner also gets the strategy, and it hashes to the on-chain commitment");
  check(createHash("sha256").update(owner.body.request_body).digest("hex") === latest.request_hash, "the owner's request bytes hash to the receipt's request_hash");
}

async function acquire() {
  console.log("\n== Acquisition: alice's agent 5 buys funder's agent 3 with its own credits");
  const target = await view("agent", { token_id: "3" });
  check(target.owner === FUNDER, "agent 3 is owned by funder (run the sale first)", target.owner);
  pay(ALICE, "30000000", { action: "load", owner: ALICE, token_id: "5" });
  tx(ALICE, CONTRACT, [{ method: "set_acquisition_budget", args: { token_id: "5", budget_micro_usd: "30000000" } }]);
  const buyer = await view("agent", { token_id: "5" });
  check(buyer.acquisition_budget_micro_usd === "30000000", "alice set a 30 USDC acquisition budget for agent 5", `credits ${Number(buyer.credits_micro_usd) / 1e6}`);
  const funderBefore = BigInt(await view("balance_of", { account: FUNDER }));
  const aliceBefore = BigInt(await view("balance_of", { account: ALICE }));

  console.log("asking agent 5's model whether to buy (one runner pass)");
  const out = execFileSync("target/release/inferents-runner.exe", ["tick", "5"], { encoding: "utf8" });
  console.log(out.trim().split("\n").pop());
  let now = await view("agent", { token_id: "3" });
  if (now.owner === FUNDER) {
    console.log("the model passed; buying through the same contract call the runner makes, to prove that path");
    await fetch(`${API}/agents/3/refresh`, { method: "POST" });
    tx(CONTRACT, CONTRACT, [{ method: "buy_with_credits", args: { buyer_token_id: "5", target_token_id: "3" }, gas: "60 Tgas" }]);
    now = await view("agent", { token_id: "3" });
  }
  check(now.owner === ALICE && now.acquired_by === "5", "agent 3 now belongs to agent 5's owner, acquired_by = 5", `${now.owner}, ${now.acquired_by}`);
  const after = await view("agent", { token_id: "5" });
  const spent = BigInt(buyer.credits_micro_usd) - BigInt(after.credits_micro_usd);
  const budgetUsed = 30_000_000n - BigInt(after.acquisition_budget_micro_usd);
  check(budgetUsed > 0n && spent >= budgetUsed, "the price came out of agent 5's credits and its budget", `budget used ${Number(budgetUsed) / 1e6} USDC, credits down ${Number(spent) / 1e6} (price plus thinking)`);
  const funderGot = BigInt(await view("balance_of", { account: FUNDER })) - funderBefore;
  const aliceGot = BigInt(await view("balance_of", { account: ALICE })) - aliceBefore;
  check(funderGot > 0n && aliceGot > 0n, "seller (funder) paid, minter (alice) paid the royalty", `funder ${Number(funderGot) / 1e6}, alice ${Number(aliceGot) / 1e6}`);
  await invariant("after the acquisition");
}

async function withdraw() {
  console.log("\n== Refunds: both accounts withdraw their held balances");
  for (const who of [ALICE, FUNDER]) {
    const held = BigInt(await view("balance_of", { account: who }));
    const walletBefore = BigInt(await view("ft_balance_of", { account_id: who }, USDC));
    if (held === 0n) { check(false, `${who} has a balance to withdraw`); continue; }
    tx(who, CONTRACT, [{ method: "withdraw_balance", args: {}, gas: "60 Tgas" }]);
    const walletAfter = BigInt(await view("ft_balance_of", { account_id: who }, USDC));
    check(walletAfter - walletBefore === held && BigInt(await view("balance_of", { account: who })) === 0n, `${who.split(".")[0]} withdrew ${Number(held) / 1e6} USDC to its wallet`);
  }
  await invariant("after withdrawals");
}

for (const s of steps) await { sale, key, acquire, withdraw }[s]();
console.log(`\n${failures === 0 ? "all checks passed" : `${failures} check(s) failed`}`);
process.exit(failures ? 1 : 0);
