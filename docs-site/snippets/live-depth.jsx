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
    healthy: { label: "Deep market", cls: "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300" },
    shallow: { label: "Thin market", cls: "bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300" },
    critical: { label: "Very thin", cls: "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300" },
    dead: { label: "No buyers", cls: "bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400" },
  };

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
      <div className="not-prose rounded-2xl border border-zinc-200 dark:border-zinc-800 p-6 text-sm text-zinc-500">
        No fresh reading right now ({error}). Orbit measures every 30 minutes; try again shortly.
      </div>
    );
  }
  if (!run) {
    return (
      <div className="not-prose rounded-2xl border border-zinc-200 dark:border-zinc-800 p-6 text-sm text-zinc-500">
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

  return (
    <div className="not-prose my-6">
      <div className="flex items-center justify-between flex-wrap gap-3 mb-4">
        <div className="flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
          <span className="relative flex h-2.5 w-2.5">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-60"></span>
            <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-emerald-500"></span>
          </span>
          Live · measured {ago(run.measured_at)}
        </div>
        <div className="text-sm text-zinc-500 dark:text-zinc-400">
          <b className="text-zinc-900 dark:text-zinc-100">{money(total)}</b> sellable before a 10% drop ·{" "}
          <b className="text-zinc-900 dark:text-zinc-100">{live.length}</b> of {rows.length} tokens have buyers
        </div>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        {live.map(({ r, d, book, pools, total: t, verdict }) => {
          const bookPct = t > 0 ? Math.round((book / t) * 100) : 0;
          const st = STATUS[verdict];
          return (
            <div key={r.code} className="rounded-2xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 p-5">
              <div className="flex items-start justify-between gap-2">
                <div>
                  <div className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">{r.code}</div>
                  <div className="text-xs text-zinc-500">{NAMES[r.code] || ""}</div>
                </div>
                <span className={`text-xs font-semibold px-2.5 py-1 rounded-full ${st.cls}`}>{st.label}</span>
              </div>
              <div className="mt-4 text-3xl font-bold tracking-tight text-zinc-900 dark:text-zinc-100">{money(t)}</div>
              <div className="text-xs text-zinc-500 mt-1">can be sold before the price falls 10%</div>
              <div className="mt-4 flex h-2.5 rounded-full overflow-hidden bg-zinc-100 dark:bg-zinc-800">
                <div style={{ width: `${bookPct}%`, background: "#2F5BD8" }}></div>
                <div style={{ width: `${100 - bookPct}%`, background: "#18A2A8" }}></div>
              </div>
              <div className="mt-2 flex gap-4 text-xs text-zinc-500">
                <span>
                  <span className="inline-block w-2 h-2 rounded-full mr-1.5" style={{ background: "#2F5BD8" }}></span>
                  Order book <b className="text-zinc-800 dark:text-zinc-200">{money(book)}</b>
                </span>
                <span>
                  <span className="inline-block w-2 h-2 rounded-full mr-1.5" style={{ background: "#18A2A8" }}></span>
                  AMM pools <b className="text-zinc-800 dark:text-zinc-200">{money(pools)}</b>
                </span>
              </div>
              <div className="mt-4 grid grid-cols-3 gap-2 text-center">
                {[200, 1000, 2500].map((b) => (
                  <div key={b} className="rounded-lg bg-zinc-50 dark:bg-zinc-800/60 py-2">
                    <div className="text-[11px] text-zinc-500">falls {b / 100}%</div>
                    <div className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
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
        <div className="mt-4 text-xs text-zinc-500">
          No buyers on Stellar today:{" "}
          {rows
            .filter((x) => x.verdict === "dead")
            .map((x) => x.r.code)
            .join(", ")}
          .
        </div>
      )}
    </div>
  );
};
