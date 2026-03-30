import type { PaperFillRow } from "../api/client";

export type PnlTimeScope = "instant" | "daily" | "weekly" | "monthly" | "yearly";

/** Default initial paper quote when no prior fill exists (matches dry `place` default). */
export const DEFAULT_PAPER_INITIAL_QUOTE = 10_000;

export function num(v: string | number): number {
  if (typeof v === "number") return Number.isFinite(v) ? v : 0;
  const x = parseFloat(String(v).replace(/,/g, ""));
  return Number.isFinite(x) ? x : 0;
}

export function paperSignedCashflow(fill: PaperFillRow): number {
  const q = num(fill.quantity);
  const p = num(fill.avg_price);
  const fee = num(fill.fee);
  const buy = String(fill.side).toLowerCase() === "buy";
  if (buy) return -(q * p + fee);
  return q * p - fee;
}

export function rfc3339Since(scope: PnlTimeScope): string {
  const d = new Date();
  switch (scope) {
    case "instant":
      d.setUTCHours(d.getUTCHours() - 24);
      break;
    case "daily":
      d.setUTCDate(d.getUTCDate() - 45);
      break;
    case "weekly":
      d.setUTCDate(d.getUTCDate() - 450);
      break;
    case "monthly":
      d.setUTCFullYear(d.getUTCFullYear() - 6);
      break;
    case "yearly":
      d.setUTCFullYear(d.getUTCFullYear() - 12);
      break;
    default:
      d.setUTCDate(d.getUTCDate() - 45);
  }
  return d.toISOString();
}

export function startOfDayUtc(d: Date): Date {
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
}

/** ISO week: Monday 00:00 UTC. */
export function startOfWeekMondayUtc(d: Date): Date {
  const day = d.getUTCDay();
  const daysFromMon = (day + 6) % 7;
  const t = new Date(d);
  t.setUTCDate(t.getUTCDate() - daysFromMon);
  return startOfDayUtc(t);
}

export function startOfMonthUtc(d: Date): Date {
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1));
}

export function startOfYearUtc(d: Date): Date {
  return new Date(Date.UTC(d.getUTCFullYear(), 0, 1));
}

function periodKeyForScope(scope: PnlTimeScope, t: Date): Date {
  switch (scope) {
    case "instant":
      return startOfDayUtc(t);
    case "daily":
      return startOfDayUtc(t);
    case "weekly":
      return startOfWeekMondayUtc(t);
    case "monthly":
      return startOfMonthUtc(t);
    case "yearly":
      return startOfYearUtc(t);
    default:
      return startOfDayUtc(t);
  }
}

export type PaperPeriodBar = {
  key: string;
  periodStart: Date;
  pnl: number;
  fees: number;
  volume: number;
  trades: number;
};

/** Fills sorted ascending by time. */
export function paperFillsSortedAsc(fills: PaperFillRow[]): PaperFillRow[] {
  return [...fills].sort(
    (a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime() || String(a.id).localeCompare(String(b.id)),
  );
}

export function paperPeriodBarsFromFills(
  fillsAsc: PaperFillRow[],
  scope: PnlTimeScope,
  maxBars: number,
): PaperPeriodBar[] {
  const map = new Map<
    string,
    { periodStart: Date; pnl: number; fees: number; volume: number; trades: number }
  >();
  for (const f of fillsAsc) {
    const t = new Date(f.created_at);
    const ps = periodKeyForScope(scope === "instant" ? "daily" : scope, t);
    const key = ps.toISOString();
    const cf = paperSignedCashflow(f);
    const vol = Math.abs(num(f.quantity) * num(f.avg_price));
    const row = map.get(key) ?? { periodStart: ps, pnl: 0, fees: 0, volume: 0, trades: 0 };
    row.pnl += cf;
    row.fees += num(f.fee);
    row.volume += vol;
    row.trades += 1;
    map.set(key, row);
  }
  const bars: PaperPeriodBar[] = [...map.values()]
    .map((r) => ({
      key: r.periodStart.toISOString(),
      periodStart: r.periodStart,
      pnl: r.pnl,
      fees: r.fees,
      volume: r.volume,
      trades: r.trades,
    }))
    .sort((a, b) => a.periodStart.getTime() - b.periodStart.getTime());
  if (bars.length <= maxBars) return bars;
  return bars.slice(-maxBars);
}

export type EquityPoint = { t: string; equity: number; fillId: string };

export function paperEquitySeries(fillsAsc: PaperFillRow[], initialQuote: number): EqualitySeriesResult {
  let equity = initialQuote;
  const points: EquityPoint[] = [];
  for (const f of fillsAsc) {
    equity += paperSignedCashflow(f);
    points.push({ t: f.created_at, equity, fillId: f.id });
  }
  return { points, finalEquity: equity };
}

export type EqualitySeriesResult = { points: EquityPoint[]; finalEquity: number };

export function impliedInitialQuoteFromFills(fillsAsc: PaperFillRow[]): number {
  if (fillsAsc.length === 0) return DEFAULT_PAPER_INITIAL_QUOTE;
  const first = fillsAsc[0]!;
  return num(first.quote_balance_after) - paperSignedCashflow(first);
}
