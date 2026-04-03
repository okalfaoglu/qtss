import type { EngineSnapshotJoinedApiRow } from "../api/client";
import type { TradingRangeDbPayload } from "./tradingRangeDbOverlay";
import { engineRowMatchesTarget, normalizeEngineMarketSegment, type EngineTargetLookup } from "./engineTargetMatch";

export type TradingRangeSnapshotListKind =
  | "no_snapshot"
  | "error"
  | "insufficient_bars"
  | "empty_payload"
  | "ok";

export function findTradingRangeSnapshotForTarget(
  snapshots: EngineSnapshotJoinedApiRow[],
  target: EngineTargetLookup,
): EngineSnapshotJoinedApiRow | null {
  return (
    snapshots.find((s) => s.engine_kind === "trading_range" && engineRowMatchesTarget(s, target)) ?? null
  );
}

/** Classify `trading_range` snapshot row for list UI (no i18n). */
export function classifyTradingRangeSnapshotRow(trSnap: EngineSnapshotJoinedApiRow | null): {
  kind: TradingRangeSnapshotListKind;
  errorMessage?: string;
  payload?: TradingRangeDbPayload;
} {
  if (!trSnap) return { kind: "no_snapshot" };
  const err = trSnap.error?.trim();
  if (err) return { kind: "error", errorMessage: err };
  const pl = trSnap.payload;
  if (!pl || typeof pl !== "object") return { kind: "empty_payload" };
  const raw = pl as Record<string, unknown>;
  if (raw.reason === "insufficient_bars") return { kind: "insufficient_bars" };
  return { kind: "ok", payload: pl as TradingRangeDbPayload };
}

export function engineSymbolMatchesToolbarVenue(
  row: { exchange: string; segment: string },
  toolbarExchange: string,
  toolbarSegment: string,
): boolean {
  const exOk = row.exchange.trim().toLowerCase() === toolbarExchange.trim().toLowerCase();
  const segOk =
    normalizeEngineMarketSegment(row.segment) === normalizeEngineMarketSegment(toolbarSegment);
  return exOk && segOk;
}
