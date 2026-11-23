import Link from "next/link";

export const metadata = {
  title: "Methodology · Orbit",
  description: "How Orbit measures exit liquidity on Stellar, step by step.",
};

const STEPS: { n: string; title: string; body: React.ReactNode }[] = [
  {
    n: "1",
    title: "The question",
    body: (
      <>
        How many US dollars could be raised selling this asset on Stellar right now before the{" "}
        <b>average</b> sale price falls more than a band below the reference price? Published at
        2%, 10% and 25%; the liquidation curve samples 12 bands from 0.5% to 50%.
      </>
    ),
  },
  {
    n: "2",
    title: "Assets",
    body: (
      <>
        An asset is code + issuer. It is measured only if the issuer&apos;s own{" "}
        <code>stellar.toml</code> (SEP-1) lists that exact pair, so impostor tokens with the same
        code are never counted.
      </>
    ),
  },
  {
    n: "3",
    title: "Venues",
    body: (
      <>
        SDEX order books against USDC and XLM (Horizon, exact rational prices), constant-product
        pools (Aquarius, Soroswap) from their reserves, and Aquarius concentrated and stable pools
        from their own <code>estimate_swap</code> quotes. Every pool&apos;s tokens must equal Stellar
        Asset Contract ids derived locally from the network id. Everything is read-only.
      </>
    ),
  },
  {
    n: "4",
    title: "One reference price",
    body: (
      <>
        Each venue reports its own price. Venues with under $100 of depth are ignored; the
        reference is the upper median of the rest. No venue is its own reference, a thin venue
        cannot move it, and with no qualifying venue the asset reads <b>zero</b>.
      </>
    ),
  },
  {
    n: "5",
    title: "Exit on each venue",
    body: (
      <>
        Order books: walk bids from the best, counting a bid&apos;s amount in the counter asset
        (Horizon&apos;s convention). Constant-product pools: the closed form{" "}
        <code>x + dx′ ≤ (1 − f)·y / floor</code>. Quoted pools: the largest sampled sale inside the
        band, never interpolated. Venues are summed in USD.
      </>
    ),
  },
  {
    n: "6",
    title: "Time",
    body: (
      <>
        Measured every 30 minutes. The published figure is the <b>minimum over 24 hours</b>, and
        only with at least 12 samples and no gap over 2 hours. Liquidity parked for one
        measurement can only lower it.
      </>
    ),
  },
  {
    n: "7",
    title: "Collateral cap",
    body: (
      <>
        A liquidator paid a bonus <i>b</i> acts only if it can sell at most <i>b</i> below the
        price, so the exit at band <i>b</i> is the most collateral that can be liquidated
        on-chain. The recommended cap is the 24h minimum of that, less a safety margin.
      </>
    ),
  },
  {
    n: "8",
    title: "Arithmetic and publication",
    body: (
      <>
        7-decimal fixed point, no floats; every rounding lowers depth. The publisher writes the
        windowed minimum with its timestamp and sample count to the depth oracle, which rejects
        replays, stale or future data, and any signer but the publisher.
      </>
    ),
  },
];

const LIMITS = [
  "Venues that are not pinned, and pools against counters other than USDC and XLM, are not counted (this understates depth).",
  "Quoted pools are sampled, so exit between samples is not credited.",
  "Exit on other chains and redemption with the issuer are not counted as exit on Stellar.",
  "The reference comes from Stellar venues only; the 24h minimum limits, but cannot detect, a market that is wrong everywhere at once.",
  "The contract and publisher have not had an external audit yet.",
];

export default function Methodology() {
  return (
    <main className="wrap">
      <header className="top">
        <div>
          <h1>Methodology</h1>
          <p>How every figure on the dashboard is produced. Full version: docs/METHODOLOGY.md.</p>
        </div>
        <nav className="nav">
          <Link href="/">Dashboard</Link>
          <span aria-current="page">Methodology</span>
        </nav>
      </header>
      <section className="steps">
        {STEPS.map((s) => (
          <article key={s.n} className="step">
            <div className="step-n">{s.n}</div>
            <div>
              <h2>{s.title}</h2>
              <p>{s.body}</p>
            </div>
          </article>
        ))}
      </section>
      <section className="panel limits">
        <h2>Known limits</h2>
        <ul>
          {LIMITS.map((l) => (
            <li key={l}>{l}</li>
          ))}
        </ul>
      </section>
    </main>
  );
}
