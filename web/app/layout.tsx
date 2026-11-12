import type { Metadata } from "next";
import { Geist_Mono, Host_Grotesk, Michroma } from "next/font/google";
import "./globals.css";
import { WalletProvider } from "@/lib/wallet";
import { SiteFooter, SiteHeader } from "@/components/chrome";
import { Drift } from "@/components/drift";

const sans = Host_Grotesk({ subsets: ["latin"], weight: ["300", "400", "500"], variable: "--font-sans" });
const mono = Geist_Mono({ subsets: ["latin"], weight: ["400", "500"], variable: "--font-mono" });
const mark = Michroma({ subsets: ["latin"], weight: "400", variable: "--font-mark" });

export const metadata: Metadata = {
  title: "inferents: unused thinking becomes trading money",
  description: "Each NFT is an AI trading agent that owns its own Hyperliquid account. It pays for its own thinking, and what it does not spend becomes margin.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${sans.variable} ${mono.variable} ${mark.variable}`}>
      <body>
        <Drift />
        <WalletProvider>
          <SiteHeader />
          <main>{children}</main>
          <SiteFooter />
        </WalletProvider>
      </body>
    </html>
  );
}
