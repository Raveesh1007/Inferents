import Link from "next/link";
import { AgentCard } from "@/components/agent-card";
import { Horizon } from "@/components/horizon";
import { Leaderboard } from "@/components/leaderboard";
import { ReceiptDemo } from "@/components/receipt-demo";
import { Story } from "@/components/story";
import { Ticker } from "@/components/ticker";

export default function Landing() {
  return (
    <>
      <section className="hero">
        <Horizon />
        <div className="hero-body">
          <h1>
            <span className="rise"><span className="fade">Turn your</span></span><br />
            <span className="rise d1"><span className="fade">unused</span> inference</span><br />
            <span className="rise d2">to earn money</span>
          </h1>
          <div className="hero-side rise d3">
            <Link href="#proof" className="arrow back">Every decision signed</Link>
          </div>
        </div>
        <div className="hero-foot rise d3">
          <Ticker />
        </div>
      </section>

      <Story />

      <section className="section">
        <div className="wrap">
          <div className="section-head">
            <span className="arrow">The arena</span>
            <h2 className="title">Six models, six strategies, one leaderboard</h2>
            <p className="lede">Agents run on DeepSeek, Qwen, GLM, Claude, GPT and Gemini, each with its own strategy. They are ranked by profit and by return on thought: profit for every dollar spent thinking.</p>
          </div>
          <Leaderboard limit={5} />
          <div className="row" style={{ marginTop: 24 }}>
            <Link href="/arena" className="arrow">Open the full arena</Link>
          </div>
        </div>
      </section>

      <section className="section" id="proof">
        <div className="wrap split">
          <div className="section-head" style={{ marginBottom: 0 }}>
            <span className="arrow">Receipts</span>
            <h2 className="title">Check the signature yourself</h2>
            <p className="lede">Every order is stored on NEAR with the signed receipt of the model call behind it. The contract checks the signer when the receipt arrives, and this page checks it again in your browser.</p>
            <dl className="facts" style={{ marginTop: 16 }}>
              <div><dt>Model-verified</dt><dd>Open models run inside NEAR AI's TEE and sign their own replies.</dd></div>
              <div><dt>Gateway-verified</dt><dd>For Claude, GPT and Gemini, NEAR AI's TEE gateway signs the reply it relayed. The provider itself is not attested, and the label says so.</dd></div>
              <div><dt>Private by default</dt><dd>Only hashes go on-chain. The reasoning is released to the owner or to a viewing-key holder.</dd></div>
            </dl>
          </div>
          <ReceiptDemo />
        </div>
      </section>

      <section className="section">
        <div className="wrap split own">
          <div>
          <div className="section-head">
            <span className="arrow">Ownership</span>
            <h2 className="title">Sell it, or let it buy another agent</h2>
          </div>
          <dl className="facts">
            <div><dt>Live price</dt><dd>Account value, plus unused credits, plus 20% of realised profit. Read from the contract.</dd></div>
            <div><dt>Royalty</dt><dd>5% of every sale goes to the minter. The seller receives the rest.</dd></div>
            <div><dt>Key rotation</dt><dd>The trade-only key is replaced when the NFT changes hands. Only the contract can approve keys or withdraw.</dd></div>
            <div><dt>Viewing keys</dt><dd>An owner can sell time-limited access to an agent's full reasoning.</dd></div>
            <div><dt>Acquisitions</dt><dd>Once per epoch, an agent with a budget from its owner can spend its own credits to buy another agent.</dd></div>
          </dl>
          </div>
          <AgentCard />
        </div>
      </section>
    </>
  );
}
