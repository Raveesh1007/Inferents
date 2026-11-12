"use client";
import Link from "next/link";
import { useWallet } from "@/lib/wallet";

export function SiteHeader() {
  const { accountId, connect, disconnect } = useWallet();
  return (
    <header className="site-header">
      <nav>
        <Link href="/arena" className="arrow">Arena</Link>
        <Link href="/mint" className="arrow">Mint</Link>
      </nav>
      <Link href="/" className="wordmark" aria-label="inferents home">inferents</Link>
      <div className="right">
        {accountId ? (
          <>
            <span className="mono dim account" title={accountId}>{accountId}</span>
            <button className="btn quiet" onClick={disconnect}>Disconnect</button>
          </>
        ) : (
          <button className="btn" onClick={connect}>Connect wallet</button>
        )}
      </div>
    </header>
  );
}

export function SiteFooter() {
  return (
    <footer className="site-footer">
      <div className="wrap footer-top">
        <h2>Think less,<br />trade bigger</h2>
        <div className="row">
          <Link href="/mint" className="btn large">Mint an agent</Link>
          <Link href="/arena" className="btn quiet large">Open the arena</Link>
        </div>
      </div>
      <div className="wrap footer-base">
        <span>inferents · testnet demo on NEAR testnet and Hyperliquid testnet, paid with mock USDC</span>
        <nav>
          <Link href="/arena">Arena</Link>
          <Link href="/mint">Mint</Link>
          <Link href="/#how">How it works</Link>
          <Link href="/#proof">Receipts</Link>
        </nav>
      </div>
    </footer>
  );
}

// Inner pages open on the same horizon as the landing hero, lower and quieter.
export function PageHero({ eyebrow, title, children }: { eyebrow: string; title: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="page-hero">
      <div className="horizon" aria-hidden />
      <div className="wrap">
        <span className="arrow">{eyebrow}</span>
        <h1>{title}</h1>
        {children}
      </div>
    </div>
  );
}
