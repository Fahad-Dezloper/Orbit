// Typed client for pocky-api. All figures arrive as fixed-point decimal strings with
// 7 places ("271086.0000000"); they are parsed only for display, never for maths.

export const API_URL = process.env.POCKY_API_URL ?? "http://127.0.0.1:8080";

export type Verdict = "dead" | "critical" | "shallow" | "healthy";

export type Venue =
  | { kind: "sdex"; counter: string }
  | { kind: "amm"; protocol: string; pool: string; counter: string };

export interface VenueExit {
  venue: Venue;
  band: number;
  usd: string;
}

export interface AssetDepth {
  asset: string;
  band: number;
  venues: VenueExit[];
  total_usd: string;
  verdict: Verdict;
}

export interface VenueMid {
  label: string;
  mid_usd: string;
  depth_usd: string;
}

export interface AssetReport {
  code: string;
  issuer: string;
  reference: { price_usd: string; qualifying: string[] } | null;
  venue_mids: VenueMid[];
  depths: AssetDepth[];
  notes: string[];
  venues_read?: number;
  curve?: { band: number; exit_usd: string }[];
}

export interface RunStats {
  duration_ms: number;
  assets: number;
  venues: number;
  sep1_domains: number;
}

export interface Run {
  measured_at: number;
  xlm_usd: string;
  venues: string;
  reports: AssetReport[];
  skipped: { code: string; reason: string }[];
  stats?: RunStats;
}

export type Windowed =
  | {
      status: "ok";
      band: number;
      min_exit_usd: string;
      verdict: Verdict;
      samples: number;
      from_ts: number;
      to_ts: number;
    }
  | { status: "insufficient"; band: number; reason: string };

export interface AssetDetail {
  measured_at: number;
  window_secs: number;
  windowed: Windowed[];
}

export type Result<T> = { ok: true; data: T } | { ok: false; error: string };

async function get<T>(path: string): Promise<Result<T>> {
  try {
    const res = await fetch(`${API_URL}${path}`, { cache: "no-store" });
    const body = await res.json();
    if (!res.ok) return { ok: false, error: body?.error ?? `HTTP ${res.status}` };
    return { ok: true, data: body as T };
  } catch (e) {
    return { ok: false, error: `pocky-api unreachable at ${API_URL}` };
  }
}

export const getLatest = () => get<Run>("/v1/latest");
export const getAsset = (code: string) =>
  get<AssetDetail>(`/v1/assets/${encodeURIComponent(code)}`);

export interface History {
  code: string;
  band: number;
  hours: number;
  points: { ts: number; exit_usd: string }[];
}

export interface Exposure {
  lender: string;
  contract: string;
  status: string;
  code: string;
  supplied_usd: string;
  borrowable_usd: string;
  exit_usd_at_bonus: string | null;
  exceeds_exit: boolean;
}

export interface Lenders {
  measured_at: number;
  bonus_bps: number;
  exposures: Exposure[];
  exceeding_exit: number;
  skipped: { code: string; reason: string }[];
}

export const getLenders = (bonusBps = 500) => get<Lenders>(`/v1/lenders?bonus_bps=${bonusBps}`);

export const getHistory = (code: string, band = 1000, hours = 24) =>
  get<History>(
    `/v1/assets/${encodeURIComponent(code)}/history?band=${band}&hours=${hours}`,
  );

// ------------------------------------------------------------------ formatting

export function usd(v: string, digits = 0): string {
  const n = Number(v);
  if (!Number.isFinite(n)) return "-";
  return n.toLocaleString("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: digits,
    minimumFractionDigits: digits,
  });
}

export function venueName(v: Venue): string {
  if (v.kind === "sdex") return `SDEX · ${v.counter}`;
  const name = v.protocol.charAt(0).toUpperCase() + v.protocol.slice(1);
  return `${name} · ${v.counter}`;
}

export function utc(ts: number): string {
  return new Date(ts * 1000).toISOString().replace("T", " ").slice(0, 16) + " UTC";
}

/** Curve value at the largest sampled band not above `bonusBps` (never rounds up). */
export function liquidatableAt(r: AssetReport, bonusBps: number): string | null {
  const pts = (r.curve ?? []).filter((p) => p.band <= bonusBps);
  if (pts.length === 0) return null;
  return pts.reduce((a, b) => (b.band > a.band ? b : a)).exit_usd;
}
