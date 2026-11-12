"use client";
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { setupWalletSelector, type WalletSelector } from "@near-wallet-selector/core";
import { setupModal, type WalletSelectorModal } from "@near-wallet-selector/modal-ui";
import { setupMyNearWallet } from "@near-wallet-selector/my-near-wallet";
import { setupMeteorWallet } from "@near-wallet-selector/meteor-wallet";
import { actionCreators } from "@near-js/transactions";
import { KeyPair, type KeyPairString } from "@near-js/crypto";
import "@near-wallet-selector/modal-ui/styles.css";
import { CONTRACT, NETWORK, USDC } from "./config";

export type Call = { method: string; args: object; deposit?: string; gas?: string };

type Ctx = {
  accountId: string | null;
  connect: () => void;
  disconnect: () => Promise<void>;
  send: (receiver: string, calls: Call[]) => Promise<void>;
  payUsdc: (amount: string, msg: object) => Promise<void>;
  signAuth: (message: object) => Promise<string>;
};
const none = async () => {};
const W = createContext<Ctx>({ accountId: null, connect() {}, disconnect: none, send: none, payUsdc: none, signAuth: async () => "" });
export const useWallet = () => useContext(W);

const RECIPIENT = "inferents-reasoning";
const b64 = (u: Uint8Array) => btoa(String.fromCharCode(...u));

// NEP-413 digest: sha256(borsh(u32 2^31+413) ++ borsh({ message, nonce, recipient, callbackUrl: None })).
async function nep413Digest(message: string, nonce: Uint8Array, recipient: string) {
  const enc = new TextEncoder();
  const u32 = (n: number) => { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, n, true); return b; };
  const str = (s: string) => { const b = enc.encode(s); return [u32(b.length), b]; };
  const parts = [u32(2 ** 31 + 413), ...str(message), nonce, ...str(recipient), new Uint8Array([0])];
  const all = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  parts.reduce((at, p) => (all.set(p, at), at + p.length), 0);
  return new Uint8Array(await crypto.subtle.digest("SHA-256", all));
}

// The key a wallet saved in this browser when the user connected to this app, if any.
function appKey(accountId: string): KeyPair | null {
  for (const prefix of ["_meteor_wallet", "near-api-js:keystore:"]) {
    const stored = localStorage.getItem(`${prefix}${accountId}:${NETWORK}`);
    if (stored?.startsWith("ed25519:")) return KeyPair.fromString(stored as KeyPairString);
  }
  return null;
}

export function WalletProvider({ children }: { children: ReactNode }) {
  const sel = useRef<WalletSelector | null>(null);
  const modal = useRef<WalletSelectorModal | null>(null);
  const [accountId, setAccountId] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      const s = await setupWalletSelector({ network: NETWORK, modules: [setupMeteorWallet(), setupMyNearWallet()] });
      sel.current = s;
      modal.current = setupModal(s, { contractId: CONTRACT });
      const sync = () => setAccountId(s.store.getState().accounts.find((a) => a.active)?.accountId ?? null);
      sync();
      s.store.observable.subscribe(sync);
    })();
  }, []);

  const wallet = async () => {
    if (!sel.current) throw new Error("wallet not ready");
    return sel.current.wallet();
  };

  // Several calls to one contract go out as one transaction, so they succeed or fail together.
  const send: Ctx["send"] = async (receiver, calls) => {
    const w = await wallet();
    await w.signAndSendTransaction({
      receiverId: receiver,
      actions: calls.map((c) => actionCreators.functionCall(c.method, c.args, BigInt(c.gas ?? "30000000000000"), BigInt(c.deposit ?? "0"))),
    });
  };

  const value: Ctx = {
    accountId,
    connect: () => modal.current?.show(),
    disconnect: async () => {
      await (await wallet()).signOut();
      setAccountId(null);
    },
    send,
    // Testnet payment: mint mock USDC and pay the contract in one transaction. `msg` names the action and its owner.
    payUsdc: async (amount, msg) => {
      if (!accountId) throw new Error("connect a wallet first");
      await send(USDC, [
        { method: "mint", args: { account_id: accountId, amount } },
        { method: "ft_transfer_call", args: { receiver_id: CONTRACT, amount, msg: JSON.stringify(msg) }, deposit: "1", gas: "250000000000000" },
      ]);
    },
    // NEP-413: the runner API accepts this signature as proof the caller controls the account.
    signAuth: async (message) => {
      const nonce = crypto.getRandomValues(new Uint8Array(32));
      const text = JSON.stringify({ ...message, ts: Date.now() });
      const local = accountId ? appKey(accountId) : null;
      if (local && accountId) {
        const { signature } = local.sign(await nep413Digest(text, nonce, RECIPIENT));
        return btoa(JSON.stringify({ accountId, publicKey: local.getPublicKey().toString(), signature: b64(signature), message: text, nonce: b64(nonce) }));
      }
      const w = await wallet();
      const signed = await w.signMessage?.({ message: text, recipient: RECIPIENT, nonce: Buffer.from(nonce) });
      if (!signed) throw new Error("this wallet cannot sign messages; use Meteor");
      return btoa(JSON.stringify({ accountId: signed.accountId, publicKey: signed.publicKey, signature: signed.signature, message: text, nonce: b64(nonce) }));
    },
  };
  return <W.Provider value={value}>{children}</W.Provider>;
}
