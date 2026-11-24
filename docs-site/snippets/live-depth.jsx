// Live market depth from Orbit's public data, fetched in the reader's browser.
// Mintlify injects React hooks (useState, useEffect) into snippets, so none are imported.

export const LiveDepth = ({ show = "market", refreshSeconds = 60 }) => {
  const BASE = "https://fahad-dezloper.github.io/Orbit/v1";

  const NAMES = {
    USTRY: "US Treasury bonds",
    CETES: "Mexican government bills",
    TESOURO: "Brazilian government bonds",
    USDY: "Ondo US dollar yield",
    deJAAA: "Centrifuge AAA loan fund",
    deJTRSY: "Centrifuge Treasury fund",
    PYUSD: "PayPal USD",
    EURC: "Circle euro stablecoin",
    USDGLO: "Glo Dollar",
    AQUA: "Aquarius token",
    SolvBTC: "Solv Bitcoin",
    USDx: "FxDAO US dollar",
  };
  const OTHER = new Set([
    "EURC", "PYUSD", "USDGLO", "USDx", "EURx", "GBPx", "oUSD", "AQUA", "SolvBTC", "xSolvBTC",
  ]);
  const STATUS = {
    healthy: { label: "Deep", dot: "#0F9D58" },
    shallow: { label: "Thin", dot: "#E0A100" },
    critical: { label: "Very thin", dot: "#E5484D" },
    dead: { label: "No buyers", dot: "#9A9A9A" },
  };
  const CARD =
    "rounded-2xl bg-white dark:bg-[#1a1b1a] border border-black/5 dark:border-white/10 shadow-[0_1px_2px_rgba(18,19,18,0.04),0_8px_24px_rgba(18,19,18,0.04)]";
  const LABEL = "text-[11px] uppercase tracking-[0.08em] text-black/40 dark:text-white/40";
  const BIG = "font-light tracking-[-0.03em] text-[#121312] dark:text-[#F5F5F5] tabular-nums";
  const MINT = "#7FFAB7";
  const INK = "#121312";

  const [run, setRun] = useState(null);
  const [error, setError] = useState(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    let alive = true;
    const load = () =>
      fetch(`${BASE}/latest.json`, { cache: "no-store" })
        .then((r) => r.json())
        .then((d) => {
          if (!alive) return;
          if (d && d.error) setError(d.error);
          else {
            setRun(d);
            setError(null);
          }
        })
        .catch(() => alive && setError("Could not reach Orbit's data."));
    load();
    const t = setInterval(load, refreshSeconds * 1000);
    const c = setInterval(() => setNow(Date.now()), 15000);
    return () => {
      alive = false;
      clearInterval(t);
      clearInterval(c);
    };
  }, []);

  const money = (v) => {
    const n = Number(v || 0);
    if (n >= 1e6) return `$${(n / 1e6).toFixed(2)}M`;
    if (n >= 1e4) return `$${Math.round(n / 1e3)}K`;
    if (n >= 1e3) return `$${(n / 1e3).toFixed(1)}K`;
    return `$${Math.round(n)}`;
  };
  const at = (r, band) => (r.depths || []).find((d) => d.band === band);
  const ago = (ts) => {
    const m = Math.max(0, Math.round((now / 1000 - ts) / 60));
    return m < 1 ? "just now" : m === 1 ? "1 minute ago" : m < 60 ? `${m} minutes ago` : `${Math.round(m / 60)} hours ago`;
  };

  if (error) {
    return (
      <div className={`not-prose ${CARD} p-6 text-sm text-black/50 dark:text-white/50`}>
        No fresh reading right now ({error}). Orbit measures every 30 minutes; try again shortly.
      </div>
    );
  }
  if (!run) {
    return (
      <div className={`not-prose ${CARD} p-6 text-sm text-black/50 dark:text-white/50`}>
        Loading live data from Orbit…
      </div>
    );
  }

  const rows = run.reports
    .filter((r) => (show === "other" ? OTHER.has(r.code) : !OTHER.has(r.code)))
    .map((r) => {
      const d = at(r, 1000);
      let book = 0;
      let pools = 0;
      for (const v of (d && d.venues) || []) {
        if (v.venue.kind === "sdex") book += Number(v.usd);
        else pools += Number(v.usd);
      }
      return { r, d, book, pools, total: book + pools, verdict: (d && d.verdict) || "dead" };
    })
    .sort((a, b) => b.total - a.total);
  const live = rows.filter((x) => x.verdict !== "dead");
  const total = live.reduce((s, x) => s + x.total, 0);
  const stats = [
    [money(total), "Sellable before a 10% drop"],
    [`${live.length}`, "Tokens with buyers"],
    [`${rows.length - live.length}`, "Tokens nobody is buying"],
  ];

  return (
    <div className="not-prose my-8">
      <div className="flex items-center gap-2 mb-4 text-xs text-black/50 dark:text-white/50">
        <span className="relative flex h-2 w-2">
          <span className="animate-ping absolute inline-flex h-full w-full rounded-full opacity-70" style={{ background: MINT }}></span>
          <span className="relative inline-flex rounded-full h-2 w-2" style={{ background: "#0F9D58" }}></span>
        </span>
        <span className={LABEL} style={{ color: "inherit" }}>Live</span>
        <span>measured {ago(run.measured_at)}</span>
      </div>

      <div className="grid gap-3 grid-cols-1 sm:grid-cols-3 mb-3">
        {stats.map(([v, l]) => (
          <div key={l} className={`${CARD} p-5 flex flex-col justify-between min-h-[132px]`}>
            <div className={`${BIG} text-[44px] leading-none`}>{v}</div>
            <div className={LABEL}>{l}</div>
          </div>
        ))}
      </div>

      <div className="grid gap-3 sm:grid-cols-2">
        {live.map(({ r, book, pools, total: t, verdict }) => {
          const bookPct = t > 0 ? Math.round((book / t) * 100) : 0;
          const st = STATUS[verdict];
          return (
            <div key={r.code} className={`${CARD} p-5 transition-transform duration-200 hover:-translate-y-0.5`}>
              <div className="flex items-start justify-between gap-2">
                <div>
                  <div className="text-base font-medium text-[#121312] dark:text-[#F5F5F5]">{r.code}</div>
                  <div className="text-xs text-black/45 dark:text-white/45">{NAMES[r.code] || ""}</div>
                </div>
                <span className="flex items-center gap-1.5 text-[11px] uppercase tracking-[0.08em] text-black/50 dark:text-white/50">
                  <span className="w-1.5 h-1.5 rounded-full" style={{ background: st.dot }}></span>
                  {st.label}
                </span>
              </div>
              <div className={`${BIG} mt-5 text-[40px] leading-none`}>{money(t)}</div>
              <div className="text-xs text-black/45 dark:text-white/45 mt-2">sellable before the price falls 10%</div>
              <div className="mt-5 flex h-1.5 rounded-full overflow-hidden bg-black/5 dark:bg-white/10">
                <div className="bg-[#121312] dark:bg-[#F5F5F5]" style={{ width: `${bookPct}%` }}></div>
                <div style={{ width: `${100 - bookPct}%`, background: MINT }}></div>
              </div>
              <div className="mt-2 flex justify-between text-xs text-black/45 dark:text-white/45 tabular-nums">
                <span>
                  <span className="inline-block w-1.5 h-1.5 rounded-full mr-1.5 align-middle bg-[#121312] dark:bg-[#F5F5F5]"></span>
                  Order book {money(book)}
                </span>
                <span>
                  <span className="inline-block w-1.5 h-1.5 rounded-full mr-1.5 align-middle" style={{ background: MINT }}></span>
                  AMM pools {money(pools)}
                </span>
              </div>
              <div className="mt-5 grid grid-cols-3 border-t border-black/5 dark:border-white/10 pt-4">
                {[200, 1000, 2500].map((b) => (
                  <div key={b}>
                    <div className={LABEL}>falls {b / 100}%</div>
                    <div className="mt-1 text-lg font-light tracking-tight tabular-nums text-[#121312] dark:text-[#F5F5F5]">
                      {money(at(r, b) && at(r, b).total_usd)}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </div>

      {rows.length > live.length && (
        <div className={`${CARD} mt-3 p-5`}>
          <div className={LABEL}>No buyers on Stellar today</div>
          <div className="mt-2 flex flex-wrap gap-1.5">
            {rows
              .filter((x) => x.verdict === "dead")
              .map((x) => (
                <span key={x.r.code} className="text-xs px-2 py-1 rounded-md bg-black/[0.04] dark:bg-white/[0.06] text-black/60 dark:text-white/60">
                  {x.r.code}
                </span>
              ))}
          </div>
        </div>
      )}
    </div>
  );
};
