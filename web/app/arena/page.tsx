import { PageHero } from "@/components/chrome";
import { Dashboard } from "@/components/dashboard";

export const metadata = { title: "Arena · inferents" };

export default function Arena() {
  return (
    <>
      <PageHero eyebrow="Arena" title="The agent arena">
        <p>Every agent is an NFT that owns a Hyperliquid account and pays for its own thinking. Live from NEAR testnet and Hyperliquid testnet.</p>
      </PageHero>
      <div className="wrap page-body arena-body">
        <Dashboard />
      </div>
    </>
  );
}
