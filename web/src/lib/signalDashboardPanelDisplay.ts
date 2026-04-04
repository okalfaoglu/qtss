import type { TFunction } from "i18next";
import type { SignalDashboardPayload, SignalDashboardV2Payload } from "./signalDashboardPayload";
import { pickDashboardStr } from "./signalDashboardPayload";

/** True when worker filled this field with the range-setup diagnostics string. */
export function isRangeSetupWireSource(raw: string | undefined): boolean {
  const s = raw?.trim() ?? "";
  return s.includes("RANGE_SETUP(");
}

function numOrUndef(v: unknown): number | undefined {
  if (typeof v !== "number" || !Number.isFinite(v)) return undefined;
  return v;
}

export function pickRsi14Last(
  payloadRoot: Record<string, unknown>,
  v2: SignalDashboardV2Payload | null,
  v1: SignalDashboardPayload,
): number | undefined {
  const fromV2 = numOrUndef(v2?.rsi_14_last);
  if (fromV2 != null) return fromV2;
  const fromRoot = numOrUndef(payloadRoot.rsi_14_last);
  if (fromRoot != null) return fromRoot;
  return numOrUndef(v1.rsi_14_last);
}

/**
 * Panel copy for SMA20 / SMA50 trend axes (matches worker `YUKARI` / `ASAGI` / `YATAY` / `KAPALI`, or v2 English tokens).
 */
export function formatTrendAxisPanel(
  t: TFunction,
  v2Local: string | undefined,
  v1Local: string | undefined,
): string {
  const raw = pickDashboardStr(v2Local, v1Local);
  if (raw === "—") return raw;
  const u = raw.trim().toUpperCase().replace("İ", "I");
  if (u === "UP" || u === "YUKARI") return t("app.signalDashboard.panel.trendAxisUp");
  if (u === "DOWN" || u === "ASAGI" || u === "AŞAĞI") return t("app.signalDashboard.panel.trendAxisDown");
  if (u === "FLAT" || u === "YATAY") return t("app.signalDashboard.panel.trendAxisFlat");
  if (u === "CLOSED" || u === "KAPALI" || u === "OFF") return t("app.signalDashboard.panel.trendAxisClosed");
  return raw;
}

/** RSI(14) and ROC(10) labels from worker (`POZITIF` / …) or v2 English. */
export function formatMomentumPanel(t: TFunction, v2: string | undefined, v1: string | undefined): string {
  const raw = pickDashboardStr(v2, v1);
  if (raw === "—") return raw;
  const v = raw.trim().toLowerCase();
  if (v.includes("positive") || v.includes("pozitif")) return t("app.signalDashboard.panel.momentumPositive");
  if (v.includes("negative") || v.includes("negatif")) return t("app.signalDashboard.panel.momentumNegative");
  if (v.includes("neutral") || v.includes("nötr") || v.includes("notr")) return t("app.signalDashboard.panel.momentumNeutral");
  return raw;
}

/** Entry mode: sweep reversal vs follow-through (`DONUS` / `TAKIP` or v2). */
export function formatEntryModePanel(t: TFunction, v2: string | undefined, v1: string | undefined): string {
  const raw = pickDashboardStr(v2, v1);
  if (raw === "—") return raw;
  const v = raw.trim().toLowerCase();
  if (v === "reversal" || v.includes("dönüş") || v.includes("donus")) return t("app.signalDashboard.panel.entryReversal");
  if (v === "follow_through" || v.includes("takip")) return t("app.signalDashboard.panel.entryFollowThrough");
  if (v === "none" || v === "—" || v === "-") return "—";
  return raw;
}

function normalizedMarketModeKey(v2: string | undefined, v1: string | undefined): string {
  const prefer = (v2?.trim() || v1?.trim() || "").toLowerCase();
  if (!prefer) return "uncertain";
  if (prefer === "breakout" || prefer === "kopus" || prefer === "kopuş") return "breakout";
  if (prefer === "range") return "range";
  if (prefer === "trend") return "trend";
  if (prefer === "uncertain" || prefer === "belirsiz") return "uncertain";
  return "uncertain";
}

/**
 * Market mode label; for breakout, appends RSI(14) context (same bar as dashboard), not range-setup scores.
 */
export function formatMarketModePanel(
  t: TFunction,
  v2: string | undefined,
  v1: string | undefined,
  rsi14: number | undefined,
): string {
  const key = normalizedMarketModeKey(v2, v1);
  const label =
    key === "breakout"
      ? t("app.signalDashboard.panel.marketBreakout")
      : key === "range"
        ? t("app.signalDashboard.panel.marketRange")
        : key === "trend"
          ? t("app.signalDashboard.panel.marketTrend")
          : t("app.signalDashboard.panel.marketUncertain");
  if (key === "breakout" && rsi14 != null) {
    return `${label} (${rsi14.toFixed(2)})`;
  }
  return label;
}

export function formatVolatilityPercentPanel(
  v2: number | null | undefined,
  v1: number | null | undefined,
): string {
  const n =
    v2 != null && typeof v2 === "number" && Number.isFinite(v2)
      ? v2
      : v1 != null && typeof v1 === "number" && Number.isFinite(v1)
        ? v1
        : null;
  if (n == null) return "—";
  return `${n.toFixed(2)}%`;
}

export function formatDetectionPanel(t: TFunction, active: boolean | undefined): { text: string; toneKey: "warn" | "muted" } {
  if (active === true) return { text: t("app.signalDashboard.panel.detected"), toneKey: "warn" };
  return { text: t("app.signalDashboard.panel.notDetected"), toneKey: "muted" };
}

/** Hero-row signal source: trend narrative, not the RANGE_SETUP(...) wire string. */
export function formatTrendSignalSourcePanel(t: TFunction): string {
  return t("app.signalDashboard.panel.trendSignalSource");
}
