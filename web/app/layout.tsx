import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Orbit · How much of each Stellar RWA can really be sold",
  description:
    "Live market depth for Stellar RWAs, so lenders know what’s safe to accept as collateral.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Space+Grotesk:wght@500;600;700&display=swap"
        />
      </head>
      <body>{children}</body>
    </html>
  );
}
