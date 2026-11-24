import Link from "next/link";

export const metadata = {
  title: "How it works · Orbit",
  description: "How Orbit measures how much of each Stellar RWA can really be sold.",
};

const STEPS: { title: string; body: React.ReactNode }[] = [
  {
    title: "The question",
    body: (
      <>
        If a lender had to sell a token right now, how many dollars could it raise before the
        price falls too far? Orbit answers this for a 2%, 10% and 25% price drop, and for 12 steps
        in between, from 0.5% up to 50%.
      </>
    ),
  },
  {
    title: "Only real tokens",
    body: (
      <>
        Every token issuer lists its official tokens on its own website. Orbit only counts tokens
        that appear on that list, so look-alike tokens with the same name are ignored.
      </>
    ),
  },
  {
    title: "Every market on Stellar",
    body: (
      <>
        Orbit reads the SDEX order book (Stellar’s built-in exchange) and the AMM pools on
        Aquarius, Soroswap and Sushi. For each pool, it checks that the pool really holds the tokens it
        claims to hold before counting it.
      </>
    ),
  },
  {
    title: "One fair price",
    body: (
      <>
        Each market shows a slightly different price. Orbit takes the middle price of the markets
        that hold real money, so one small or fake market can’t set the price for everyone.
      </>
    ),
  },
  {
    title: "Adding up the buyers",
    body: (
      <>
        Starting from that price, Orbit works out how much each market would buy before the
        average sale price drops 2%, 10% or 25%, then adds the markets together in US dollars.
      </>
    ),
  },
  {
    title: "The worst of the day",
    body: (
      <>
        Orbit measures about every 30 minutes, at random times. The number it publishes is the
        lowest one from the last 24 hours. Buyers who appear for a moment and then leave can’t
        make a token look safer than it is.
      </>
    ),
  },
  {
    title: "A safe limit for lenders",
    body: (
      <>
        When a loan goes bad, the lender gives a small discount (often 5%) to whoever sells the
        collateral. Orbit shows how much can be sold within that discount, then halves it as a
        safety margin. That is the most a lender should accept as collateral.
      </>
    ),
  },
  {
    title: "On the blockchain",
    body: (
      <>
        The safe limits are written to a contract on Stellar (testnet today), so a lending app can
        check them automatically before every loan. If data is missing or old, Orbit publishes
        nothing rather than a wrong number.
      </>
    ),
  },
];

const LIMITS = [
  "Some newer markets (Curve, Comet and Stellar Broker) are not counted yet. Leaving a market out can only make the number lower, never higher.",
  "Buying back from the issuer, or selling on other blockchains, is not counted. Only Stellar’s own markets are.",
  "Someone with a lot of money could keep fake buyers in place for a whole day. Planned fixes make that slow and expensive.",
  "The contract has an internal security review, but no outside audit yet.",
];

export default function Methodology() {
  return (
    <main className="wrap">
      <header className="topbar">
        <Link href="/" className="brand">
          Orbit
        </Link>
        <nav>
          <Link href="/">Dashboard</Link>
          <Link href="/methodology" className="on">
            How it works
          </Link>
          <a href="https://fahad-dezloper.github.io/Orbit/">Docs</a>
          <a href="https://github.com/Fahad-Dezloper/Orbit">GitHub</a>
        </nav>
      </header>

      <section className="doc">
        <h1>How Orbit works</h1>
        <p className="lede">
          Orbit answers one question: how much of each Stellar RWA token can really be sold? Here
          is how each number on the dashboard is made, step by step.
        </p>

        {STEPS.map((s, i) => (
          <article key={s.title} className="doc-step">
            <div className="doc-n">{i + 1}</div>
            <div>
              <h3>{s.title}</h3>
              <p>{s.body}</p>
            </div>
          </article>
        ))}

        <div className="note-box">
          <h3>What Orbit doesn’t cover yet</h3>
          <ul>
            {LIMITS.map((l) => (
              <li key={l}>{l}</li>
            ))}
          </ul>
        </div>

        <p className="foot">
          The full technical method is in{" "}
          <a href="https://github.com/Fahad-Dezloper/Orbit/blob/main/docs/METHODOLOGY.md">
            docs/METHODOLOGY.md
          </a>
          .
        </p>
      </section>
    </main>
  );
}
