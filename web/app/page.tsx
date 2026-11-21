import {
  type AssetDetail,
  type AssetReport,
  type Result,
  type Venue,
  type Verdict,
  type Windowed,
  type History,
  getAsset,
  getHistory,
  getLatest,
  liquidatableAt,
  usd,
  utc,
  venueName,
} from "@/lib/api";

export const dynamic = "force-dynamic";

const BANDS = [200, 1000, 2500] as const;
/** Liquidation bonus used for the lender column. Blend-style pools pay 5% to 10%. */
const BONUS_BPS = 500;

/**
 * Exit at 10% over the last 24 hours, one point per measurement. The dashed line is the
 * lowest value in the window: the figure that gets published.
 */
function HistoryChart({ h }: { h: History | undefined }) {
  const pts = h?.points ?? [];
  if (pts.length < 2) {
    return (
      <figure className="hist">
        <div className="hist-empty">{pts.length} sample{pts.length === 1 ? "" : "s"} · history building</div>
        <figcaption><span>24h exit @10%</span></figcaption>
      </figure>
    );
  }
  const W = 200;
  const H = 54;
  const vals = pts.map((p) => Number(p.exit_usd));
  const max = Math.max(...vals);
  const min = Math.min(...vals);
  const t0 = pts[0].ts;
  const t1 = pts[pts.length - 1].ts;
  const span = Math.max(t1 - t0, 1);
  const x = (ts: number) => 4 + ((ts - t0) / span) * (W - 8);
  const y = (v: number) => (max > 0 ? H - 4 - (v / max) * (H - 10) : H - 4);
  const line = pts
    .map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)},${y(Number(p.exit_usd)).toFixed(1)}`)
    .join(" ");
  return (
    <figure className="hist">
      <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} role="img" aria-label="24 hour exit history">
        <line x1={4} x2={W - 4} y1={y(min)} y2={y(min)} className="hist-min" />
        <path d={line} className="hist-line" />
      </svg>
      <figcaption>
        <span>24h exit @10%</span>
        <span>min {usd(String(min))} · {pts.length} samples</span>
      </figcaption>
    </figure>
  );
}

/** Liquidation curve: total exit (y) against slippage band (x, evenly spaced samples). */
function Curve({ r }: { r: AssetReport }) {
  const pts = r.curve ?? [];
  const max = Math.max(...pts.map((p) => Number(p.exit_usd)), 0);
  if (pts.length < 2 || max < 1) return null;
  const W = 260;
  const H = 54;
  const x = (i: number) => 4 + (i / (pts.length - 1)) * (W - 8);
  const y = (v: number) => H - 4 - (v / max) * (H - 10);
  const line = pts.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(Number(p.exit_usd)).toFixed(1)}`).join(" ");
  const bi = pts.findIndex((p) => p.band === BONUS_BPS);
  return (
    <figure className="curve">
      <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${r.code} liquidation curve`}>
        <path d={`${line} L${x(pts.length - 1)},${H - 4} L${x(0)},${H - 4} Z`} className="curve-fill" />
        <path d={line} className="curve-line" />
        {bi >= 0 && (
          <circle cx={x(bi)} cy={y(Number(pts[bi].exit_usd))} r={3} className="curve-dot" />
        )}
      </svg>
      <figcaption>
        <span>{pts[0].band / 100}%</span>
        <span>slippage → sellable</span>
        <span>{pts[pts.length - 1].band / 100}%</span>
      </figcaption>
    </figure>
  );
}

function venueColor(v: Venue): string {
  if (v.kind === "sdex") return "var(--sdex)";
  return v.protocol === "aquarius" ? "var(--aquarius)" : "var(--soroswap)";
}

function Chip({ v }: { v: Verdict }) {
  return <span className={`chip v-${v}`}>{v}</span>;
}

function exitAt(r: AssetReport, band: number) {
  return r.depths.find((d) => d.band === band);
}

function WindowCell({ w }: { w: Windowed | undefined }) {
  if (!w) return <span className="sub">-</span>;
  if (w.status === "insufficient") {
    return (
      <span className="sub" title={w.reason}>
        building window
      </span>
    );
  }
  return (
    <>
      <div>{usd(w.min_exit_usd)}</div>
      <div className="sub">{w.samples} samples</div>
    </>
  );
}

/** Where the 10% exit comes from, as a stacked bar plus a legend. */
function Venues({ r }: { r: AssetReport }) {
  const d = exitAt(r, 1000);
  if (!d) return null;
  const parts = d.venues.filter((v) => Number(v.usd) >= 1);
  const total = Number(d.total_usd);
  if (total < 1 || parts.length === 0) return null;
  return (
    <>
      <div className="bar" aria-hidden>
        {parts.map((v, i) => (
          <div
            key={i}
            style={{ width: `${(Number(v.usd) / total) * 100}%`, background: venueColor(v.venue) }}
          />
        ))}
      </div>
      <div className="legend">
        {parts.map((v, i) => (
          <span key={i}>
            <i className="swatch" style={{ background: venueColor(v.venue) }} />
            {venueName(v.venue)}
            <b>{usd(v.usd)}</b>
          </span>
        ))}
      </div>
    </>
  );
}

function Notes({ r }: { r: AssetReport }) {
  // Price gaps between venues are the signal; one-sided-book notes are context.
  const gaps = r.notes.filter((n) => n.includes("the reference"));
  if (gaps.length === 0) return null;
  return (
    <>
      {gaps.map((n, i) => (
        <div key={i} className={n.includes("too thin") ? "flag muted" : "flag"}>
          ▲ {n}
        </div>
      ))}
    </>
  );
}

export default async function Page() {
  const latest = await getLatest();
  if (!latest.ok) {
    return (
      <main className="wrap">
        <Header />
        <div className="panel empty">
          No current measurement: {latest.error}.
          <br />
          Start the service with <code>pocky-api</code> (or seed it with{" "}
          <code>pocky-api --once</code>).
        </div>
      </main>
    );
  }
  const run = latest.data;
  const details = new Map<string, Result<AssetDetail>>(
    await Promise.all(
      run.reports.map(async (r) => [r.code, await getAsset(r.code)] as const),
    ),
  );
  const histories = new Map<string, History | undefined>(
    await Promise.all(
      run.reports.map(async (r) => {
        const h = await getHistory(r.code);
        return [r.code, h.ok ? h.data : undefined] as const;
      }),
    ),
  );

  const at10 = run.reports.map((r) => exitAt(r, 1000));
  const total10 = at10.reduce((s, d) => s + Number(d?.total_usd ?? 0), 0);
  const dead = at10.filter((d) => d?.verdict === "dead").length;
  const healthy = at10.filter((d) => d?.verdict === "healthy").length;

  // Deepest first.
  const rows = [...run.reports].sort(
    (a, b) => Number(exitAt(b, 1000)?.total_usd ?? 0) - Number(exitAt(a, 1000)?.total_usd ?? 0),
  );

  return (
    <main className="wrap">
      <Header>
        <div className="meta">
          <span>
            <i className="dot" style={{ background: "var(--healthy)" }} />
            measured <b>{utc(run.measured_at)}</b>
          </span>
          <span>
            XLM/USD <b className="num">{Number(run.xlm_usd).toFixed(4)}</b>
          </span>
          <span>{run.venues}</span>
          {run.stats && (
            <span>
              full scan <b className="num">{(run.stats.duration_ms / 1000).toFixed(1)}s</b> ·{" "}
              {run.stats.venues} venues
            </span>
          )}
        </div>
      </Header>

      <section className="kpis five">
        <Kpi
          label="Full scan latency"
          value={run.stats ? `${(run.stats.duration_ms / 1000).toFixed(1)}s` : "-"}
        />
        <Kpi label="Assets · venues" value={`${run.reports.length} · ${run.stats?.venues ?? "-"}`} />
        <Kpi label="Total exit @10%" value={usd(String(total10))} />
        <Kpi label="Healthy" value={String(healthy)} cls="v-healthy" />
        <Kpi label="Dead (no market)" value={String(dead)} cls="v-dead" />
      </section>

      <section className="panel">
        <table>
          <thead>
            <tr>
              <th>Asset</th>
              <th className="num">Reference</th>
              {BANDS.map((b) => (
                <th key={b} className="num">
                  Exit @{b / 100}%
                </th>
              ))}
              <th className="num">24h min @10%</th>
              <th className="num" title="Collateral that can be liquidated on Stellar right now at a 5% liquidation bonus">
                Liquidatable @5% bonus
              </th>
              <th>Verdict</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const det = details.get(r.code);
              const w10 = det?.ok ? det.data.windowed.find((w) => w.band === 1000) : undefined;
              const d10 = exitAt(r, 1000);
              return [
                <tr key={r.code} className="asset">
                  <td>
                    <div className="code">{r.code}</div>
                    <div className="sub">{r.issuer.slice(0, 4)}…{r.issuer.slice(-4)}</div>
                  </td>
                  <td className="num">
                    {r.reference ? usd(r.reference.price_usd, 4) : <span className="sub">none</span>}
                  </td>
                  {BANDS.map((b) => (
                    <td key={b} className="num">
                      {usd(exitAt(r, b)?.total_usd ?? "0")}
                    </td>
                  ))}
                  <td className="num">
                    <WindowCell w={w10} />
                  </td>
                  <td className="num">
                    {(() => {
                      const v = liquidatableAt(r, BONUS_BPS);
                      return v === null ? <span className="sub">-</span> : usd(v);
                    })()}
                  </td>
                  <td>{d10 && <Chip v={d10.verdict} />}</td>
                </tr>,
                <tr key={`${r.code}-d`} className="detail">
                  <td />
                  <td colSpan={7}>
                    <div className="detail-grid">
                      <div>
                        <Venues r={r} />
                    {!r.reference && (
                      <div className="flag muted">
                        No venue holds $100 of depth at its own price, so there is no price to
                        measure from.
                      </div>
                    )}
                    <Notes r={r} />
                      </div>
                      <HistoryChart h={histories.get(r.code)} />
                      <Curve r={r} />
                    </div>
                  </td>
                </tr>,
              ];
            })}
          </tbody>
        </table>
      </section>

      <footer>
        Exit = USD that could be raised selling the asset across SDEX order books and Soroban
        AMM pools before the <em>average</em> sale price falls more than the band below one
        reference price. The reference is the median price of venues holding real depth, so no
        venue sets its own price. The published figure is the minimum over 24 hours. Read-only;
        every rounding understates depth.
      </footer>
    </main>
  );
}

function Header({ children }: { children?: React.ReactNode }) {
  return (
    <header className="top">
      <div>
        <h1>Exit liquidity on Stellar</h1>
        <p>How much of each real-world asset can actually be sold before the price breaks.</p>
      </div>
      {children}
    </header>
  );
}

function Kpi({ label, value, cls }: { label: string; value: string; cls?: string }) {
  return (
    <div className="kpi">
      <div className="label">{label}</div>
      <div className={`value ${cls ?? ""}`}>{value}</div>
    </div>
  );
}
