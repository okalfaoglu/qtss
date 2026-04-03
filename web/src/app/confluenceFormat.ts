/** PLAN §4.2: wire keys for `lot_scale_hint`, `data_sources_considered`, `conflicts[].code`. */
export function formatConfluenceExtras(p: Record<string, unknown>): string {
  const parts: string[] = [];
  if (typeof p.lot_scale_hint === "number" && Number.isFinite(p.lot_scale_hint)) {
    parts.push(`lot_scale ${p.lot_scale_hint.toFixed(2)}`);
  }
  const dsc = p.data_sources_considered;
  if (Array.isArray(dsc) && dsc.length > 0 && dsc.every((x) => typeof x === "string")) {
    parts.push(`sources ${(dsc as string[]).join(", ")}`);
  }
  const raw = p.conflicts;
  if (Array.isArray(raw)) {
    const codes = raw
      .map((x) =>
        x && typeof x === "object" && typeof (x as Record<string, unknown>).code === "string"
          ? String((x as Record<string, unknown>).code)
          : null,
      )
      .filter((c): c is string => Boolean(c));
    const n = codes.length;
    if (n === 0) {
      parts.push("conflicts 0");
    } else {
      const preview = codes.slice(0, 3).join(", ");
      parts.push(n > 3 ? `conflicts ${n}: ${preview}…` : `conflicts ${n}: ${preview}`);
    }
  }
  return parts.length ? ` · ${parts.join(" · ")}` : "";
}
