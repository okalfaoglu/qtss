import type { RangeSignalEventApiRow } from "../api/client";

export type RangeSetupFromEvents = {
  id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  side: "long" | "short";
  entry: RangeSignalEventApiRow;
  exit: RangeSignalEventApiRow | null;
  /** Kapalı: eşleşen çıkış olayı var. Açık: giriş var, henüz çıkış yok. */
  closed: boolean;
};

function evTimeMs(ev: RangeSignalEventApiRow): number {
  const t = new Date(ev.bar_open_time).getTime();
  if (Number.isFinite(t)) return t;
  return new Date(ev.created_at).getTime();
}

function stackPair(
  events: RangeSignalEventApiRow[],
  entryKind: string,
  exitKind: string,
  side: "long" | "short",
): RangeSetupFromEvents[] {
  const stack: RangeSignalEventApiRow[] = [];
  const completed: RangeSetupFromEvents[] = [];
  for (const ev of events) {
    if (ev.event_kind === entryKind) {
      stack.push(ev);
    } else if (ev.event_kind === exitKind) {
      const entry = stack.pop();
      if (entry) {
        completed.push({
          id: `${side}-${entry.id}-${ev.id}`,
          exchange: entry.exchange,
          segment: entry.segment,
          symbol: entry.symbol,
          interval: entry.interval,
          side,
          entry,
          exit: ev,
          closed: true,
        });
      }
    }
  }
  for (const entry of stack) {
    completed.push({
      id: `${side}-open-${entry.id}`,
      exchange: entry.exchange,
      segment: entry.segment,
      symbol: entry.symbol,
      interval: entry.interval,
      side,
      entry,
      exit: null,
      closed: false,
    });
  }
  return completed;
}

/**
 * Aynı sembol+interval için DB olaylarından giriş→çıkış eşleştirmesi (LIFO).
 * Gerçek borsa işlemi değil; `signal_dashboard.durum` kenarı günlüğü. PnL sütunları
 * `reference_price` ile hesaplanır; `setupPnlPctAfterFees` tahmini taker ücreti düşer.
 */
export function rangeSetupsFromEvents(events: RangeSignalEventApiRow[]): RangeSetupFromEvents[] {
  if (!events.length) return [];
  const sorted = [...events].sort((a, b) => {
    const d = evTimeMs(a) - evTimeMs(b);
    if (d !== 0) return d;
    return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
  });
  const longs = stackPair(sorted, "long_entry", "long_exit", "long");
  const shorts = stackPair(sorted, "short_entry", "short_exit", "short");
  return [...longs, ...shorts].sort((a, b) => evTimeMs(b.entry) - evTimeMs(a.entry));
}

export function setupPnlPct(setup: RangeSetupFromEvents): number | null {
  const ep = setup.entry.reference_price;
  const xp = setup.exit?.reference_price;
  if (ep == null || !Number.isFinite(ep) || ep === 0 || xp == null || !Number.isFinite(xp)) {
    return null;
  }
  if (setup.side === "long") {
    return ((xp - ep) / ep) * 100;
  }
  return ((ep - xp) / ep) * 100;
}

/**
 * Round-trip fee (`entryRate`/`exitRate` as decimal fraction of leg notional, e.g. 0.0004)
 * deducted from gross move; still **not** a venue fill — same `reference_price` caveats as `setupPnlPct`.
 */
export function setupPnlPctAfterFees(
  setup: RangeSetupFromEvents,
  entryRate: number,
  exitRate: number,
): number | null {
  const ep = setup.entry.reference_price;
  const xp = setup.exit?.reference_price;
  if (ep == null || !Number.isFinite(ep) || ep === 0 || xp == null || !Number.isFinite(xp)) {
    return null;
  }
  if (!Number.isFinite(entryRate) || !Number.isFinite(exitRate) || entryRate < 0 || exitRate < 0) {
    return null;
  }
  const fees = ep * entryRate + xp * exitRate;
  if (setup.side === "long") {
    return ((xp - ep - fees) / ep) * 100;
  }
  return ((ep - xp - fees) / ep) * 100;
}
