import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Pocky · Exit liquidity on Stellar",
  description:
    "How much of each real-world asset on Stellar can actually be sold before the price breaks.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
