const MAX_RAW_NON_HTML = 240;

function binanceKlinesDebugFromEnv(): boolean {
  const v = import.meta.env.VITE_BINANCE_KLINES_DEBUG;
  return v === "1" || v === "true" || String(v).toLowerCase() === "yes";
}

/** Upstream returned a document page instead of Binance JSON. */
export function isLikelyHtmlErrorPayload(body: string): boolean {
  const s = body.trimStart();
  const head = s.slice(0, 48).toLowerCase();
  if (head.startsWith("<!doctype") || head.startsWith("<html") || head.startsWith("<head")) return true;
  if (head.startsWith("<") && /<title>\s*404/i.test(s.slice(0, 600))) return true;
  return false;
}

function isProbablyJsonObject(body: string): boolean {
  const t = body.trim();
  return t.startsWith("{") || t.startsWith("[");
}

/**
 * Human-readable klines error (Turkish copy matches existing `binanceKlines` UX).
 * Use `describeBinanceKlinesHttpFailureCore` in tests with an explicit debug flag.
 */
export function describeBinanceKlinesHttpFailureCore(
  status: number,
  body: string,
  ctx: { requestUrl: string; segment?: string },
  includeDebugDetails: boolean,
): string {
  const seg = ctx.segment?.trim();
  const segPart = seg ? ` [${seg}]` : "";

  if (isLikelyHtmlErrorPayload(body) || (status === 404 && body.trim().length > 0 && !isProbablyJsonObject(body))) {
    const parts = [
      `Binance klines HTTP ${status}${segPart}: Yanıt JSON değil (çoğunlukla proxy veya statik sunucunun HTML 404 sayfası).`,
      "Geliştirmede `npm run dev` ile Vite’nin `/__binance` ve `/__binance_fapi` köprüleri gerekir. Prod/önizlemede `web/.env` içinde `VITE_BINANCE_API_BASE` ve `VITE_BINANCE_FAPI_API_BASE` ile tam HTTPS tabanlarını verin; veya `VITE_BINANCE_KLINES_VIA_API=1` + giriş ile mumları QTSS API üzerinden çekin.",
    ];
    if (includeDebugDetails) {
      parts.push(`İstek: ${ctx.requestUrl}`);
      const snippet = body.replace(/\s+/g, " ").trim().slice(0, 200);
      if (snippet) parts.push(`Gövde özeti: ${snippet}`);
    } else {
      parts.push("Tam istek URL’si için `VITE_BINANCE_KLINES_DEBUG=1` (web/.env) kullanın.");
    }
    return parts.join(" ");
  }

  const slice = body.length > MAX_RAW_NON_HTML ? `${body.slice(0, MAX_RAW_NON_HTML)}…` : body;
  try {
    const j = JSON.parse(body) as { code?: number; msg?: string };
    if (j.code === -1121) {
      return `Binance klines ${status}: Geçersiz sembol (-1121). Çift spot’ta yoksa segment’i USDT-M yapın; yine olmazsa sembolü Binance’te doğrulayın. Ham: ${slice}`;
    }
    if (typeof j.msg === "string" && j.msg.trim()) {
      const codePart = typeof j.code === "number" ? ` (kod ${j.code})` : "";
      return `Binance klines ${status}: ${j.msg.trim()}${codePart}`;
    }
  } catch {
    /* not json */
  }
  return `Binance klines ${status}: ${slice}`;
}

export function describeBinanceKlinesHttpFailure(
  status: number,
  body: string,
  ctx: { requestUrl: string; segment?: string },
): string {
  return describeBinanceKlinesHttpFailureCore(status, body, ctx, binanceKlinesDebugFromEnv());
}
