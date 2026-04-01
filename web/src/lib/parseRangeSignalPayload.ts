/** `range_signal_events.payload` — worker sonrası stop/TP ipuçları (eski satırlarda boş olabilir). */
export function payloadOptionalNumber(payload: unknown, ...keys: string[]): number | null {
  if (!payload || typeof payload !== "object") return null;
  const o = payload as Record<string, unknown>;
  for (const k of keys) {
    const v = o[k];
    if (typeof v === "number" && Number.isFinite(v)) return v;
    if (typeof v === "string") {
      const n = Number(v);
      if (Number.isFinite(n)) return n;
    }
  }
  return null;
}
