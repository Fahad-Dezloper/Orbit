import Link from "next/link";
import {
  type AssetReport,
  type Cap,
  type Lenders,
  type Result,
  type Verdict,
  getCap,
  getLatest,
  getLenders,
} from "@/lib/api";

/** Plain-English names for the tokens Orbit tracks. */
const NAMES: Record<string, string> = {
  USTRY: "US Treasury bonds",
  CETES: "Mexican government bills",
  TESOURO: "Brazilian government bonds",
  USDY: "Ondo US dollar yield",
  MEX: "Mexican bond token",
  CETESZ: "Mexican bill token",
  EUROB: "Euro government bonds",
  GILTS: "UK government bonds",
  KTB: "Korean government bonds",
  MEXe: "Mexican bond token",
};

const STATUS: Record<Verdict, { label: string; tone: string }> = {
  healthy: { label: "Deep market", tone: "good" },
  shallow: { label: "Thin market", tone: "warn" },
  critical: { label: "Very thin", tone: "bad" },
  dead: { label: "No buyers", tone: "none" },
};

/** Liquidation bonus a typical lender pays (5%). */
const BONUS_BPS = 500;

function money(v: string | number | null | undefined): string {
  const n = Number(v ?? 0);
  if (!Number.isFinite(n)) return "-";
  if (n >= 1_000_000) return `$${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 10_000) return `$${Math.round(n / 1000)}K`;
  if (n >= 1_000) return `$${(n / 1000).toFixed(1)}K`;
  return n.toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
}

function price(v: string): string {
  const n = Number(v);
  return n >= 1 ? `$${n.toFixed(2)}` : `$${n.toFixed(4)}`;
}

function when(ts: number): string {
  const d = new Date(ts * 1000);
  return (
    d.toLocaleString("en-GB", {
      day: "numeric",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
      timeZone: "UTC",
    }) + " UTC"
  );
}

function exitAt(r: AssetReport, band: number) {
  return r.depths.find((d) => d.band === band);
}

/** Split the 10% exit into order book vs AMM pools. */
function split(r: AssetReport) {
  let book = 0;
  let pools = 0;
  for (const v of exitAt(r, 1000)?.venues ?? []) {
    if (v.venue.kind === "sdex") book += Number(v.usd);
    else pools += Number(v.usd);
  }
  return { book, pools, total: book + pools };
}

function TokenCard({ r, cap }: { r: AssetReport; cap?: Cap }) {
  const d10 = exitAt(r, 1000);
  const st = STATUS[d10?.verdict ?? "dead"];
  const { book, pools, total } = split(r);
  const bookPct = total > 0 ? Math.round((book / total) * 100) : 0;
  const steps = [
    { drop: "2%", v: exitAt(r, 200)?.total_usd },
    { drop: "10%", v: d10?.total_usd },
    { drop: "25%", v: exitAt(r, 2500)?.total_usd },
  ];
  const maxStep = Math.max(...steps.map((s) => Number(s.v ?? 0)), 1);

  let limit: React.ReactNode;
  if (cap?.recommended_cap_usd) {
    limit = (
      <>
        <span className="limit-label">Safe for lenders to accept</span>
        <span className="limit-value">up to {money(cap.recommended_cap_usd)}</span>
      </>
    );
  } else {
    const have =
      cap && cap.window.status === "insufficient"
        ? Number(/have (\d+)/.exec(cap.window.reason)?.[1] ?? 0)
        : 0;
    limit = (
      <>
        <span className="limit-label">Safe limit for lenders</span>
        <span className="limit-wait">
          {have > 0
            ? `Shown after a full day of readings (${Math.min(have, 12)} of 12 so far)`
            : "Shown after a full day of steady readings"}
        </span>
      </>
    );
  }

  return (
    <article className="token">
      <header className="token-head">
        <div>
          <h3>{r.code}</h3>
          <p className="token-name">{NAMES[r.code] ?? r.code}</p>
        </div>
        <span className={`badge ${st.tone}`}>{st.label}</span>
      </header>

      <div className="headline">
        <div className="big">{money(d10?.total_usd)}</div>
        <p>can be sold before the price falls 10%</p>
      </div>

      <div className="split">
        <div className="split-bar" aria-hidden>
          <span className="seg book" style={{ width: `${bookPct}%` }} />
          <span className="seg pools" style={{ width: `${100 - bookPct}%` }} />
        </div>
        <div className="split-legend">
          <span>
            <i className="dot book" /> Order book <b>{money(book)}</b>
          </span>
          <span>
            <i className="dot pools" /> AMM pools <b>{money(pools)}</b>
          </span>
        </div>
      </div>

      <div className="steps">
        <p className="steps-title">Sell more and the price falls more</p>
        {steps.map((s) => (
          <div className="step" key={s.drop}>
            <span className="step-drop">Price falls {s.drop}</span>
            <span className="step-bar">
              <span style={{ width: `${(Number(s.v ?? 0) / maxStep) * 100}%` }} />
            </span>
            <span className="step-v">{money(s.v)}</span>
          </div>
        ))}
      </div>

      <footer className="token-foot">
        <div className="limit">{limit}</div>
        <p className="token-meta">Current price {r.reference ? price(r.reference.price_usd) : "unknown"}</p>
      </footer>
    </article>
  );
}

/** Blend pool states in plain words. */
function poolStatus(s: string): string {
  if (s.includes("frozen")) return "frozen";
  if (s.includes("on_ice")) return "paused";
  if (s === "setup") return "not open yet";
  return "open";
}

function LenderTable({ l }: { l: Lenders }) {
  if (l.exposures.length === 0) return null;
  return (
    <section className="section">
      <h2>What lenders hold today</h2>
      <p className="section-sub">
        How much of each token Blend lending pools hold, next to how much of it could actually be
        sold.
      </p>
      <div className="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Lending pool</th>
              <th>Token</th>
              <th className="r">Holds</th>
              <th className="r">Could be sold</th>
              <th>Verdict</th>
            </tr>
          </thead>
          <tbody>
            {l.exposures.map((e) => (
              <tr key={`${e.contract}-${e.code}`}>
                <td>
                  {e.lender}
                  <span className="muted"> · {poolStatus(e.status)}</span>
                </td>
                <td>{e.code}</td>
                <td className="r">{money(e.supplied_usd)}</td>
                <td className="r">{e.exit_usd_at_bonus ? money(e.exit_usd_at_bonus) : "-"}</td>
                <td>
                  {e.exceeds_exit ? (
                    <span className="badge bad">Too much to sell</span>
                  ) : (
                    <span className="badge good">Can be sold</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function Top({ measured }: { measured?: number }) {
  return (
    <header className="topbar">
      <Link href="/" className="brand">
        Orbit
      </Link>
      <nav>
        <Link href="/" className="on">
          Dashboard
        </Link>
        <Link href="/methodology">How it works</Link>
        <a href="https://github.com/Fahad-Dezloper/Orbit">GitHub</a>
      </nav>
      {measured && (
        <span className="updated">
          <i className="live" /> Updated {when(measured)}
        </span>
      )}
    </header>
  );
}

export default async function Page() {
  const latest = await getLatest();
  if (!latest.ok) {
    return (
      <main className="wrap">
        <Top />
        <div className="empty">
          <h2>No fresh reading right now</h2>
          <p>Orbit measures every 30 minutes. The next reading will appear here shortly.</p>
        </div>
      </main>
    );
  }
  const run = latest.data;
  const lenders: Result<Lenders> = await getLenders(BONUS_BPS);

  const verdict = (r: AssetReport) => exitAt(r, 1000)?.verdict ?? "dead";
  const withMarket = run.reports
    .filter((r) => verdict(r) !== "dead")
    .sort((a, b) => Number(exitAt(b, 1000)?.total_usd ?? 0) - Number(exitAt(a, 1000)?.total_usd ?? 0));
  const noMarket = run.reports.filter((r) => verdict(r) === "dead");
  const total = withMarket.reduce((s, r) => s + Number(exitAt(r, 1000)?.total_usd ?? 0), 0);

  const caps = new Map<string, Cap>();
  await Promise.all(
    withMarket.map(async (r) => {
      const c = await getCap(r.code);
      if (c.ok) caps.set(r.code, c.data);
    }),
  );

  return (
    <main className="wrap">
      <Top measured={run.measured_at} />

      <section className="hero">
        <h1>How much of each Stellar RWA can really be sold?</h1>
        <p>
          A price tells you what a token is worth. It doesn’t tell you if anyone will buy it. Orbit
          checks every market on Stellar every 30 minutes and shows how many dollars a lender
          could raise by selling, before the price falls too far.
        </p>
      </section>

      <section className="summary">
        <div className="stat">
          <div className="stat-v">{money(total)}</div>
          <p>could be sold across all tokens before prices fall 10%</p>
        </div>
        <div className="stat">
          <div className="stat-v good">{withMarket.length}</div>
          <p>tokens have a real market</p>
        </div>
        <div className="stat">
          <div className="stat-v bad">{noMarket.length}</div>
          <p>tokens have no buyers at all</p>
        </div>
      </section>

      <section className="section">
        <h2>Tokens with a market</h2>
        <p className="section-sub">
          Largest first. Each card shows where the buyers are and how much they would take.
        </p>
        <div className="grid">
          {withMarket.map((r) => (
            <TokenCard key={r.code} r={r} cap={caps.get(r.code)} />
          ))}
        </div>
      </section>

      {noMarket.length > 0 && (
        <section className="section">
          <h2>Tokens with no buyers</h2>
          <p className="section-sub">
            No market on Stellar holds even $100 of buyers for these. A lender could not sell them
            at all.
          </p>
          <div className="chips">
            {noMarket.map((r) => (
              <span className="chip" key={r.code}>
                <b>{r.code}</b>
                {NAMES[r.code] ?? ""}
              </span>
            ))}
          </div>
        </section>
      )}

      {lenders.ok && <LenderTable l={lenders.data} />}

      <footer className="foot">
        <p>
          Markets read: SDEX order books, Aquarius pools and Soroswap pools. Every number can be
          checked: <a href="v1/latest.json">raw data</a> · <Link href="/methodology">how it works</Link>.
        </p>
      </footer>
    </main>
  );
}
