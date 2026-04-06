import type { ChartOhlcRow } from "../lib/marketBarsToCandles";
import type { AuthSession } from "../lib/rbac";
import { throwQtssApiError } from "../lib/apiErrorFormat";

/**
 * API tabanı: geliştirmede Vite proxy kullanıldığı için "" (relative).
 * Doğrudan backend'e giderken VITE_API_BASE örn. http://127.0.0.1:8080
 */
const API_BASE = import.meta.env.VITE_API_BASE ?? "";

export type TokenResponse = {
  access_token: string;
  token_type: string;
  expires_in: number;
  refresh_token?: string;
};

/** `App.tsx` registers this so 401 responses can refresh the Bearer token. */
export type ApiAuthConfig = {
  getRefreshToken: () => string | null;
  setTokens: (accessToken: string, refreshToken: string | null) => void;
  getOAuthClientCredentials: () => { clientId: string; clientSecret: string };
};

let apiAuthConfig: ApiAuthConfig | null = null;
let refreshInFlight: Promise<string | null> | null = null;

export function configureApiAuth(cfg: ApiAuthConfig | null) {
  apiAuthConfig = cfg;
}

export async function oauthTokenRefresh(params: {
  refreshToken: string;
  clientId: string;
  clientSecret: string;
}): Promise<TokenResponse> {
  const body = new URLSearchParams({
    grant_type: "refresh_token",
    refresh_token: params.refreshToken,
    client_id: params.clientId,
    client_secret: params.clientSecret,
  });
  const r = await fetch(`${API_BASE}/oauth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });
  const t = await r.text();
  if (!r.ok) {
    throwQtssApiError("oauth refresh", r, t);
  }
  return JSON.parse(t) as TokenResponse;
}

async function refreshAccessToken(): Promise<string | null> {
  const cfg = apiAuthConfig;
  if (!cfg) return null;
  if (refreshInFlight) return refreshInFlight;

  refreshInFlight = (async (): Promise<string | null> => {
    try {
      const rt = cfg.getRefreshToken();
      const { clientId, clientSecret } = cfg.getOAuthClientCredentials();
      if (!rt?.trim() || !clientId || !clientSecret) return null;
      const tr = await oauthTokenRefresh({
        refreshToken: rt.trim(),
        clientId,
        clientSecret,
      });
      const nextRt = tr.refresh_token?.trim() ? tr.refresh_token.trim() : rt.trim();
      cfg.setTokens(tr.access_token, nextRt);
      return tr.access_token;
    } catch {
      return null;
    } finally {
      refreshInFlight = null;
    }
  })();

  return refreshInFlight;
}

/**
 * Protected API calls: on 401, attempts one OAuth `refresh_token` grant (single-flight),
 * updates tokens via `configureApiAuth`, then retries the request once.
 */
export async function fetchWithBearerRetry(
  url: string,
  accessToken: string,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${accessToken}`);
  let res = await fetch(url, { ...init, headers });
  if (res.status !== 401) return res;

  const newAccess = await refreshAccessToken();
  if (!newAccess) return res;

  const h2 = new Headers(init.headers);
  h2.set("Authorization", `Bearer ${newAccess}`);
  return fetch(url, { ...init, headers: h2 });
}

export type SystemConfigRowApi = {
  id: string;
  module: string;
  config_key: string;
  value: unknown;
  schema_version: number;
  description: string | null;
  is_secret: boolean;
  updated_at: string;
  updated_by_user_id: string | null;
};

export async function fetchAdminSystemConfig(
  accessToken: string,
  q?: { module?: string; limit?: number },
): Promise<SystemConfigRowApi[]> {
  const params = new URLSearchParams();
  if (q?.module?.trim()) params.set("module", q.module.trim());
  if (q?.limit != null && Number.isFinite(q.limit)) params.set("limit", String(q.limit));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/admin/system-config?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("admin/system-config", r, t);
  return JSON.parse(t) as SystemConfigRowApi[];
}

export async function upsertAdminSystemConfig(
  accessToken: string,
  body: {
    module: string;
    config_key: string;
    value: unknown;
    schema_version?: number;
    description?: string;
    is_secret?: boolean;
  },
): Promise<SystemConfigRowApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/admin/system-config`, accessToken, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("admin/system-config POST", r, t);
  return JSON.parse(t) as SystemConfigRowApi;
}

/** Single row (including secret values — admin only). */
export async function fetchAdminSystemConfigRow(
  accessToken: string,
  module: string,
  configKey: string,
): Promise<SystemConfigRowApi> {
  const m = encodeURIComponent(module.trim());
  const k = encodeURIComponent(configKey.trim());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/admin/system-config/${m}/${k}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("admin/system-config row", r, t);
  return JSON.parse(t) as SystemConfigRowApi;
}

export async function deleteAdminSystemConfig(
  accessToken: string,
  module: string,
  configKey: string,
): Promise<number> {
  const m = encodeURIComponent(module.trim());
  const k = encodeURIComponent(configKey.trim());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/admin/system-config/${m}/${k}`, accessToken, {
    method: "DELETE",
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("admin/system-config DELETE", r, t);
  return JSON.parse(t) as number;
}

export type AppConfigEntryApi = {
  id: string;
  key: string;
  value: unknown;
  description: string | null;
  updated_at: string;
  updated_by_user_id: string | null;
};

export async function fetchHealth(): Promise<unknown> {
  const r = await fetch(`${API_BASE}/health`);
  const t = await r.text();
  if (!r.ok) throwQtssApiError("health", r, t);
  return JSON.parse(t) as unknown;
}

/** Public catalog; aligns with `web/src/locales/supportedLocales.ts` and `GET /api/v1/locales`. */
export type SupportedLocaleApi = {
  code: string;
  nativeName: string;
  dir: string;
};

export async function fetchSupportedLocales(): Promise<SupportedLocaleApi[]> {
  const r = await fetch(`${API_BASE}/api/v1/locales`);
  const t = await r.text();
  if (!r.ok) throwQtssApiError("locales", r, t);
  const j = JSON.parse(t) as { locales: SupportedLocaleApi[] };
  return j.locales ?? [];
}

/** SPA bootstrap: `system_config` seed values via API (no JWT). */
export type WebOAuthBootstrapApi = {
  clientId: string;
  clientSecret: string;
  suggestedLoginEmail: string;
};

function webOAuthBootstrapHeaders(): HeadersInit {
  const tok = import.meta.env.VITE_WEB_OAUTH_BOOTSTRAP_TOKEN?.trim();
  return tok ? { "X-QTSS-Bootstrap-Token": tok } : {};
}

export async function fetchWebOAuthBootstrap(): Promise<WebOAuthBootstrapApi> {
  const r = await fetch(`${API_BASE}/api/v1/bootstrap/web-oauth-client`, {
    headers: webOAuthBootstrapHeaders(),
  });
  const t = await r.text();
  if (!r.ok) {
    const emptyServerError = r.status >= 500 && !t.trim();
    const hint = emptyServerError
      ? " (Often: qtss-api not running, wrong VITE_API_BASE, system_config api.web_dev_proxy_target / Vite DATABASE_URL, or reverse proxy empty 5xx. Check API logs.)"
      : "";
    throwQtssApiError("bootstrap/web-oauth-client", r, t, hint);
  }
  const j = JSON.parse(t) as {
    clientId?: string;
    clientSecret?: string;
    suggestedLoginEmail?: string;
  };
  return {
    clientId: j.clientId?.trim() ?? "",
    clientSecret: j.clientSecret?.trim() ?? "",
    suggestedLoginEmail: j.suggestedLoginEmail?.trim() ?? "",
  };
}

export async function oauthTokenPassword(params: {
  clientId: string;
  clientSecret: string;
  email: string;
  password: string;
}): Promise<TokenResponse> {
  const body = new URLSearchParams({
    grant_type: "password",
    client_id: params.clientId,
    client_secret: params.clientSecret,
    username: params.email,
    password: params.password,
  });
  const r = await fetch(`${API_BASE}/oauth/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: body.toString(),
  });
  const t = await r.text();
  if (!r.ok) {
    const hint =
      r.status === 401
        ? " (401: client_id/client_secret uyuşmuyor veya oauth_clients satırı eksik — seed çalıştırın; web tarafında GET /api/v1/bootstrap/web-oauth-client ile system_config eşleşmeli.)"
        : "";
    throwQtssApiError("oauth", r, t, hint);
  }
  return JSON.parse(t) as TokenResponse;
}

/** JWT doğrulandıktan sonra rol / org özeti (GUI RBAC). */
export async function fetchAuthMe(accessToken: string): Promise<AuthSession> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/me`, accessToken, {});
  const t = await r.text();
  if (!r.ok) {
    throwQtssApiError("me", r, t);
  }
  const j = JSON.parse(t) as {
    sub: string;
    org_id: string;
    roles: string[];
    azp: string;
    permissions?: string[];
    preferred_locale?: string | null;
  };
  return {
    userId: j.sub,
    orgId: j.org_id,
    roles: Array.isArray(j.roles) ? j.roles : [],
    permissions: Array.isArray(j.permissions) ? j.permissions : [],
    oauthClientId: j.azp,
    preferredLocale:
      typeof j.preferred_locale === "string" && j.preferred_locale.trim() !== ""
        ? j.preferred_locale.trim()
        : j.preferred_locale === null
          ? null
          : undefined,
  };
}

/** Persists `users.preferred_locale` (`en` | `tr`); pass `null` to clear. */
export async function patchMePreferredLocale(
  accessToken: string,
  preferred_locale: string | null,
): Promise<void> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/me/locale`, accessToken, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ preferred_locale }),
  });
  const text = await r.text();
  if (!r.ok) {
    throwQtssApiError("me/locale", r, text);
  }
}

export async function fetchConfigList(accessToken: string): Promise<AppConfigEntryApi[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/config`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("config", r, t);
  }
  return r.json() as Promise<AppConfigEntryApi[]>;
}

/** Dashboard rolleri — `app_config.acp_chart_patterns` ile aynı JSON (admin değil). */
export async function fetchChartPatternsConfig(accessToken: string): Promise<unknown> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/chart-patterns-config`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("chart-patterns-config", r, t);
  }
  return r.json();
}

/** Dashboard rolleri — `app_config.elliott_wave` veya sunucu varsayılanı. */
export async function fetchElliottWaveConfig(accessToken: string): Promise<unknown> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/elliott-wave-config`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("elliott-wave-config", r, t);
  }
  return r.json();
}

/** Yalnızca admin — `POST /api/v1/config`. */
export async function upsertAppConfig(
  accessToken: string,
  body: { key: string; value: unknown; description?: string },
): Promise<AppConfigEntryApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/config`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("config upsert", r, t);
  }
  return r.json() as Promise<AppConfigEntryApi>;
}

export async function deleteAppConfig(accessToken: string, key: string): Promise<number> {
  const k = encodeURIComponent(key.trim());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/config/${k}`, accessToken, {
    method: "DELETE",
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("config DELETE", r, t);
  return JSON.parse(t) as number;
}

/** API `MarketBarRow` (snake_case; Decimal alanları JSON sayı veya string olabilir). */
export type MarketBarRow = {
  id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  open_time: string;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
  quote_volume: string | null;
  trade_count: number | null;
  created_at: string;
  updated_at: string;
};

export async function fetchMarketBarsRecent(
  accessToken: string,
  params: {
    exchange: string;
    segment: string;
    symbol: string;
    interval: string;
    limit?: number;
  },
): Promise<MarketBarRow[]> {
  const q = new URLSearchParams({
    exchange: params.exchange,
    segment: params.segment,
    symbol: params.symbol,
    interval: params.interval,
  });
  if (params.limit != null) q.set("limit", String(params.limit));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/market/bars/recent?${q}`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("market/bars/recent", r, t);
  }
  return r.json() as Promise<MarketBarRow[]>;
}

export type CatalogExchangeRowApi = {
  id: string;
  code: string;
  display_name: string;
  is_active: boolean;
  metadata: unknown;
  created_at: string;
  updated_at: string;
};

export type CatalogMarketWithExchangeApi = {
  id: string;
  exchange_id: string;
  segment: string;
  contract_kind: string;
  display_name: string | null;
  is_active: boolean;
  metadata: unknown;
  created_at: string;
  updated_at: string;
  exchange_code: string;
};

export type InstrumentSuggestionRowApi = {
  native_symbol: string;
  base_asset: string;
  quote_asset: string;
  status: string;
};

export async function fetchCatalogExchanges(accessToken: string): Promise<CatalogExchangeRowApi[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/catalog/exchanges`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("catalog/exchanges", r, t);
  }
  return r.json() as Promise<CatalogExchangeRowApi[]>;
}

export async function fetchCatalogMarkets(
  accessToken: string,
  exchangeCode?: string,
): Promise<CatalogMarketWithExchangeApi[]> {
  const q = new URLSearchParams();
  if (exchangeCode?.trim()) q.set("exchange_code", exchangeCode.trim());
  q.set("limit", "200");
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/catalog/markets?${q}`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("catalog/markets", r, t);
  }
  return r.json() as Promise<CatalogMarketWithExchangeApi[]>;
}

export async function fetchInstrumentSuggestions(
  accessToken: string,
  params: { exchangeCode: string; segment: string; query: string; limit?: number },
): Promise<InstrumentSuggestionRowApi[]> {
  const q = new URLSearchParams({
    exchange_code: params.exchangeCode.trim() || "binance",
    segment: params.segment.trim() || "spot",
    query: params.query.trim(),
    limit: String(params.limit ?? 40),
  });
  const r = await fetchWithBearerRetry(
    `${API_BASE}/api/v1/catalog/instrument-suggestions?${q}`,
    accessToken,
    {},
  );
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("catalog/instrument-suggestions", r, t);
  }
  return r.json() as Promise<InstrumentSuggestionRowApi[]>;
}

/** `qtss-binance` connector üzerinden klines (DB yazılmaz). JWT + dashboard rolü gerekir. */
type QtssKlineBarJson = {
  open_time: number;
  open: string;
  high: string;
  low: string;
  close: string;
};

export async function fetchMarketBinanceKlinesForChart(
  accessToken: string,
  params: {
    symbol: string;
    interval: string;
    limit?: number;
    /** `spot` (varsayılan) | `futures` / `usdt_futures` / `fapi` */
    segment?: string;
    /** Binance open_time ile uyumlu ms (API `start_time`). */
    startTimeMs?: number;
    /** Binance open_time ile uyumlu ms (API `end_time`). */
    endTimeMs?: number;
  },
): Promise<ChartOhlcRow[]> {
  /** Binance klines tek istek üst sınırı 1000; daha uzun seri `binanceKlines.ts` sayfalaması ile. */
  const limit = Math.min(1000, Math.max(1, params.limit ?? 500));
  const q = new URLSearchParams({
    symbol: params.symbol.trim().toUpperCase(),
    interval: params.interval.trim(),
    limit: String(limit),
  });
  if (params.startTimeMs != null && Number.isFinite(params.startTimeMs)) {
    q.set("start_time", String(Math.floor(params.startTimeMs)));
  }
  if (params.endTimeMs != null && Number.isFinite(params.endTimeMs)) {
    q.set("end_time", String(Math.floor(params.endTimeMs)));
  }
  const seg = (params.segment?.trim().toLowerCase() || "spot").trim();
  q.set("segment", seg);
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/market/binance/klines?${q}`, accessToken, {});
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("market/binance/klines", r, t);
  }
  const raw = (await r.json()) as unknown;
  if (!Array.isArray(raw)) throw new Error("klines response is not a JSON array");
  const out: ChartOhlcRow[] = [];
  for (const row of raw) {
    if (row === null || typeof row !== "object") continue;
    const o = row as QtssKlineBarJson;
    if (typeof o.open_time !== "number") continue;
    out.push({
      open_time: new Date(o.open_time).toISOString(),
      open: String(o.open),
      high: String(o.high),
      low: String(o.low),
      close: String(o.close),
    });
  }
  return out;
}

/** Son N mumu Binance REST’ten çekip `market_bars` tablosuna yazar. `admin` veya `trader` rolü gerekir. */
export async function backfillMarketBarsFromRest(
  accessToken: string,
  body: {
    symbol: string;
    interval: string;
    segment?: string;
    limit?: number;
  },
): Promise<{ upserted: number; source?: string; symbol?: string; segment?: string }> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/market/binance/bars/backfill`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      symbol: body.symbol,
      interval: body.interval,
      segment: body.segment,
      limit: body.limit,
    }),
  });
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("market/binance/bars/backfill", r, t);
  }
  return r.json() as Promise<{ upserted: number; source?: string; symbol?: string; segment?: string }>;
}

/** `POST /api/v1/analysis/patterns/channel-six` yanıtı. */
export type ChannelSixScanJson = {
  pattern_type_id: number;
  pick_upper: number;
  pick_lower: number;
  upper_ok: boolean;
  lower_ok: boolean;
  upper_score: number;
  lower_score: number;
};

export type ChannelSixOutcomeJson = {
  scan: ChannelSixScanJson;
  pivots: [number, number, number][];
  zigzag_pivot_count: number;
  /** Pine tarzı pivot penceresi kaydırması (0 = en güncel 6’lı). */
  pivot_tail_skip?: number;
  /** Çoklu seviye zigzag taramasında eşleşmenin bulunduğu seviye (0 = temel). */
  zigzag_level?: number;
};

export type ChannelSixDrawingJson = {
  upper: [{ bar_index: number; price: number }, { bar_index: number; price: number }];
  lower: [{ bar_index: number; price: number }, { bar_index: number; price: number }];
};

/** API `reject.code` — eşleşme yokken hangi aşama elendi. */
export type ChannelSixRejectCode =
  | "insufficient_pivots"
  | "pivot_alternation"
  | "bar_ratio_upper"
  | "bar_ratio_lower"
  | "inspect_upper"
  | "inspect_lower"
  | "pattern_not_allowed"
  | "overlap_ignored"
  | "duplicate_pivot_window"
  | "last_pivot_direction"
  | "size_filter"
  | "entry_not_in_channel";

export type ChannelSixRejectJson = {
  code: ChannelSixRejectCode;
  have_pivots?: number;
  need_pivots?: number;
};

/** ACP literature-style entry / SL / TP hints (`qtss-chart-patterns` `formation_trade_levels`). */
export type FormationTradeSideJson = "long" | "short";

export type FormationTakeProfitJson = {
  id: string;
  price: number;
  note: string;
};

export type FormationTradeLevelsJson = {
  pattern_type_id: number;
  side: FormationTradeSideJson;
  entry: number;
  stop_loss: number;
  take_profits: FormationTakeProfitJson[];
  band_upper_at_bar: number;
  band_lower_at_bar: number;
  reference_bar: number;
  method: string;
};

export type FormationVolumeAnalysisJson = {
  has_volume_data: boolean;
  volume_divergence: boolean;
  divergence_type: string;
  volume_change_ratio?: number;
  breakout_volume?: { breakout_volume: number; avg_volume: number; volume_ratio: number; confirmed: boolean };
  pivot_volumes?: { bar_index: number; price: number; dir: number; volume?: number }[];
  avg_formation_volume?: number;
};

export type FormationMatchJson = {
  pattern_type_id: number;
  pattern_name: string;
  pivots: [number, number, number][];
  neckline?: number;
  height: number;
  target_price?: number;
  quality: number;
  volume_analysis?: FormationVolumeAnalysisJson;
};

export type PatternMatchPayloadJson = {
  outcome: ChannelSixOutcomeJson;
  pattern_name?: string;
  pattern_drawing_batch?: PatternDrawingBatchJson;
  formation_trade_levels?: FormationTradeLevelsJson;
  apex?: { apex_bar: number; apex_price: number; bars_to_apex: number; proximity_ratio: number; is_stale: boolean };
  failure_swing?: { reach_ratio: number; is_failure: boolean; failure_side: string; last_pivot_price: number; target_band_price: number; band_width: number };
  breakout_volume?: { breakout_volume: number; avg_volume: number; volume_ratio: number; confirmed: boolean };
};

export type ChannelSixResponse = {
  matched: boolean;
  bar_count: number;
  zigzag_pivot_count: number;
  /** İstekteki `repaint` yansıması (Pine: açık mum). */
  repaint?: boolean;
  reject?: ChannelSixRejectJson;
  outcome?: ChannelSixOutcomeJson;
  drawing?: ChannelSixDrawingJson;
  pattern_name?: string;
  pattern_drawing_batch?: PatternDrawingBatchJson;
  pattern_matches?: PatternMatchPayloadJson[];
  /** `pattern_matches` içinde `pivot_tail_skip === 0` ve `zigzag_level === 0` olan ilk satırın indeksi — robot / canlı sinyal. */
  live_robot_match_index?: number | null;
  used_zigzag?: { length: number; depth: number };
  /** İlk eşleşme ile aynı: `pattern_matches[0].formation_trade_levels`. */
  formation_trade_levels?: FormationTradeLevelsJson;
  /** Faz 2 formasyonları (Double Top/Bottom, H&S, Triple, Flag). */
  formations?: FormationMatchJson[];
  /** Faz 2 formasyonlarının çizim komutları. */
  formation_drawing_batches?: PatternDrawingBatchJson[];
};

export type PatternDrawingTimePrice = {
  time_ms: number;
  price: number;
  bar_index?: number;
};

export type PatternDrawingCommandJson =
  | {
      kind: "trend_line";
      p1: PatternDrawingTimePrice;
      p2: PatternDrawingTimePrice;
      line_width: number;
      color_hex?: string;
      /** Pine `line.extend` — yoksa yalnızca iki uç. */
      extend?: "none" | "left" | "right" | "both";
      /** Her yöne en fazla kaç `bar_index` (grafik dilimine kırpılır). */
      extend_bars?: number;
    }
  | { kind: "zigzag_polyline"; points: PatternDrawingTimePrice[]; line_width: number; color_hex?: string }
  | { kind: "pattern_label"; at: PatternDrawingTimePrice; text: string; color_hex?: string }
  | {
      kind: "pivot_label";
      at: PatternDrawingTimePrice;
      text: string;
      color_hex?: string;
      anchor?: "high" | "low";
    };

export type PatternDrawingBatchJson = {
  batch_id: string;
  pattern_type_id?: number;
  pattern_name?: string;
  commands: PatternDrawingCommandJson[];
};

export async function scanChannelSix(
  accessToken: string,
  body: {
    bars: Array<{ bar_index: number; open: number; high: number; low: number; close: number }>;
    zigzag_configs?: Array<{ enabled?: boolean; length: number; depth: number }>;
    zigzag_length?: number;
    zigzag_max_pivots?: number;
    zigzag_offset?: number;
    bar_ratio_enabled?: boolean;
    bar_ratio_limit?: number;
    flat_ratio?: number;
    number_of_pivots?: 5 | 6;
    upper_direction?: number;
    lower_direction?: number;
    pivot_tail_skip_max?: number;
    max_zigzag_levels?: number;
    allowed_pattern_ids?: number[];
    error_score_ratio_max?: number;
    avoid_overlap?: boolean;
    /** Pine `repaint`: true = açık mum dahil; false = yalnız kapanmış (sunucu sondaki mumu düşürebilir). */
    repaint?: boolean;
    allowed_last_pivot_directions?: number[];
    theme_dark?: boolean;
    pattern_line_width?: number;
    zigzag_line_width?: number;
    max_matches?: number;
    ignore_if_entry_crossed?: boolean;
    size_filters?: {
      filter_by_bar?: boolean;
      min_pattern_bars?: number;
      max_pattern_bars?: number;
      filter_by_percent?: boolean;
      min_pattern_percent?: number;
      max_pattern_percent?: number;
    };
  },
): Promise<ChannelSixResponse> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/patterns/channel-six`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) {
    throwQtssApiError("channel-six", r, t);
  }
  return JSON.parse(t) as ChannelSixResponse;
}

export type BacktestEquityPointApi = {
  ts: string;
  equity: string;
};

export type BacktestClosedTradeApi = {
  entry_ts: string;
  exit_ts: string;
  side: string;
  qty: string;
  entry_px: string;
  exit_px: string;
  pnl: string;
  fee: string;
};

export type BacktestRunResponseApi = {
  meta: {
    exchange: string;
    segment: string;
    symbol: string;
    interval: string;
    start_time: string;
    end_time: string;
    bar_count: number;
    strategy: string;
    initial_equity: string;
    sma_fast?: number | null;
    sma_slow?: number | null;
  };
  equity_curve: BacktestEquityPointApi[];
  trades: BacktestClosedTradeApi[];
  report: unknown;
};

export async function postBacktestRun(
  accessToken: string,
  body: {
    strategy: "buy_and_hold" | "sma_cross" | "trading_range";
    exchange: string;
    segment: string;
    symbol: string;
    interval: string;
    start_time: string;
    end_time: string;
    initial_equity: string | number;
    sma_fast?: number;
    sma_slow?: number;
    /** Slippage in basis points (optional; server default). */
    slippage_bps?: number;
    /** Taker fee in basis points per fill (optional; server default). */
    taker_fee_bps?: number;
  },
): Promise<BacktestRunResponseApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/backtest/run`, accessToken, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("backtest/run", r, t);
  return JSON.parse(t) as BacktestRunResponseApi;
}

export type EngineSymbolApiRow = {
  id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  enabled: boolean;
  sort_order: number;
  label: string | null;
  /** `both` | `long_only` | `short_only` | `auto_segment` */
  signal_direction_mode?: string;
  created_at: string;
  updated_at: string;
};

export type EngineSnapshotJoinedApiRow = {
  engine_symbol_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  engine_kind: string;
  payload: unknown;
  last_bar_open_time: string | null;
  bar_count: number | null;
  computed_at: string;
  error: string | null;
};

export async function fetchEngineSymbols(accessToken: string): Promise<EngineSymbolApiRow[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/symbols`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("engine/symbols", r, t);
  return JSON.parse(t) as EngineSymbolApiRow[];
}

export async function fetchEngineSnapshots(accessToken: string): Promise<EngineSnapshotJoinedApiRow[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/snapshots`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("engine/snapshots", r, t);
  return JSON.parse(t) as EngineSnapshotJoinedApiRow[];
}

/** `GET …/ingestion-state` — worker `engine_symbol_ingestion_state` joined with `engine_symbols`. */
export type EngineSymbolIngestionApiRow = {
  id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  enabled: boolean;
  sort_order: number;
  label: string | null;
  bar_row_count: number | null;
  min_open_time: string | null;
  max_open_time: string | null;
  gap_count: number | null;
  max_gap_seconds: number | null;
  last_backfill_at: string | null;
  last_health_check_at: string | null;
  last_error: string | null;
  ingestion_updated_at: string | null;
};

export async function fetchEngineSymbolIngestion(accessToken: string): Promise<EngineSymbolIngestionApiRow[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/ingestion-state`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("engine/ingestion-state", r, t);
  return JSON.parse(t) as EngineSymbolIngestionApiRow[];
}

/** `app_config.range_engine` — worker + web; `GET /api/v1/analysis/range-engine/config`. */
export type RangeEngineConfigApi = {
  trading_range_params?: {
    lookback?: number | null;
    atr_period?: number | null;
    atr_sma_period?: number | null;
    require_range_regime?: boolean | null;
  };
  execution_gates?: {
    allow_long_open?: boolean;
    allow_short_open?: boolean;
    allow_all_closes?: boolean;
  };
  worker?: {
    refresh_requested?: boolean;
  };
};

export async function fetchRangeEngineConfig(accessToken: string): Promise<RangeEngineConfigApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/range-engine/config`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("analysis/range-engine/config", r, t);
  return JSON.parse(t) as RangeEngineConfigApi;
}

/** `PATCH` — gövde `app_config` ile derin birleştirilir (`trader`/`admin` ops). */
export async function patchRangeEngineConfig(
  accessToken: string,
  patch: Record<string, unknown>,
): Promise<RangeEngineConfigApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/range-engine/config`, accessToken, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(patch),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("analysis/range-engine/config PATCH", r, t);
  return JSON.parse(t) as RangeEngineConfigApi;
}

/** Eski `qtss-api` bu rotayı sunmuyorsa 404 — Bağlam sekmesi diğer uçları yine yükleyebilsin. */
export type ConfluenceSnapshotsLatestResult = {
  rows: EngineSnapshotJoinedApiRow[];
  /** Sunucu `GET …/engine/confluence/latest` için 404 döndürdü (rota yok / eski binary). */
  endpoint_missing?: boolean;
};

/** SPEC_ONCHAIN_SIGNALS §7 — `onchain_signal_scores` + breakdown meta. */
export type OnchainSignalScoreApiRow = {
  id: string;
  symbol: string;
  computed_at: string;
  funding_score: number | null;
  oi_score: number | null;
  ls_ratio_score: number | null;
  taker_vol_score: number | null;
  exchange_netflow_score: number | null;
  exchange_balance_score: number | null;
  hl_bias_score: number | null;
  hl_whale_score: number | null;
  liquidation_score: number | null;
  nansen_sm_score: number | null;
  tvl_trend_score: number | null;
  aggregate_score: number;
  confidence: number;
  direction: string;
  market_regime: string | null;
  conflict_detected: boolean;
  conflict_detail: string | null;
  snapshot_keys: string[];
  meta_json: unknown | null;
};

export type OnchainSignalsBreakdownApi = {
  symbol: string;
  latest_score_row: OnchainSignalScoreApiRow | null;
  onchain_breakdown: unknown | null;
  data_snapshots: DataSnapshotApiRow[];
};

export type OnchainSignalsBreakdownResult = {
  data: OnchainSignalsBreakdownApi | null;
  endpoint_missing: boolean;
};

/** `GET …/analysis/onchain-signals/breakdown` — skor + `meta_json.source_breakdown` + ham snapshot’lar. */
export async function fetchOnchainSignalsBreakdown(
  accessToken: string,
  symbol: string,
): Promise<OnchainSignalsBreakdownResult> {
  const params = new URLSearchParams({ symbol: symbol.trim().toUpperCase() });
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/onchain-signals/breakdown?${params}`, accessToken, {});
  const t = await r.text();
  if (r.status === 404) {
    return { data: null, endpoint_missing: true };
  }
  if (!r.ok) {
    throwQtssApiError("onchain-signals/breakdown", r, t);
  }
  return {
    data: JSON.parse(t) as OnchainSignalsBreakdownApi,
    endpoint_missing: false,
  };
}

export async function fetchConfluenceSnapshotsLatest(
  accessToken: string,
): Promise<ConfluenceSnapshotsLatestResult> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/confluence/latest`, accessToken, {});
  const t = await r.text();
  if (r.status === 404) {
    return { rows: [], endpoint_missing: true };
  }
  if (!r.ok) throwQtssApiError("engine/confluence/latest", r, t);
  return {
    rows: JSON.parse(t) as EngineSnapshotJoinedApiRow[],
    endpoint_missing: false,
  };
}

/** Birleşik ham kayıt satırları (`data_snapshots`) — Nansen + external_fetch vb. */
export type DataSnapshotApiRow = {
  source_key: string;
  request_json: unknown;
  response_json: unknown | null;
  meta_json: unknown | null;
  computed_at: string;
  error: string | null;
};

export async function fetchDataSnapshots(accessToken: string): Promise<DataSnapshotApiRow[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/data-snapshots`, accessToken, {});
  const t = await r.text();
  if (r.status === 404) return [];
  if (!r.ok) throwQtssApiError("analysis/data-snapshots", r, t);
  return JSON.parse(t) as DataSnapshotApiRow[];
}

/** `external_data_sources` satırları — worker `external_fetch` tanımları (PLAN Phase A). */
export type ExternalDataSourceApiRow = {
  key: string;
  enabled: boolean;
  method: string;
  url: string;
  headers_json: unknown;
  body_json: unknown | null;
  tick_secs: number;
  description: string | null;
};

export async function fetchExternalFetchSources(accessToken: string): Promise<ExternalDataSourceApiRow[]> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/external-fetch/sources`, accessToken, {});
  const t = await r.text();
  if (r.status === 404) return [];
  if (!r.ok) throwQtssApiError("external-fetch/sources", r, t);
  return JSON.parse(t) as ExternalDataSourceApiRow[];
}

/** PLAN F7 Phase C — tek hedef için TA + confluence + ilgili `data_snapshots`. */
export type MarketContextLatestApiResponse = {
  /** `false` when no matching `engine_symbols` row (HTTP 200; older APIs omitted this — treat as true if `engine_symbol_id` is set). */
  found?: boolean;
  engine_symbol_id?: string;
  exchange?: string;
  segment?: string;
  symbol?: string;
  interval?: string;
  technical?: {
    signal_dashboard: unknown | null;
    trading_range: unknown | null;
  };
  confluence?: unknown | null;
  context_data_snapshots?: DataSnapshotApiRow[];
};

/** True when API returned a matching `engine_symbols` row (supports legacy JSON without `found`). */
export function marketContextLatestHasEngineRow(
  r: MarketContextLatestApiResponse | null | undefined,
): r is MarketContextLatestApiResponse & {
  engine_symbol_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  technical: NonNullable<MarketContextLatestApiResponse["technical"]>;
} {
  if (!r) return false;
  if (r.found === false) return false;
  return Boolean(r.engine_symbol_id?.trim() && r.technical);
}

export async function fetchMarketContextLatest(
  accessToken: string,
  q: { symbol: string; interval?: string; exchange?: string; segment?: string },
): Promise<MarketContextLatestApiResponse | null> {
  const params = new URLSearchParams();
  params.set("symbol", q.symbol.trim().toUpperCase());
  if (q.interval?.trim()) params.set("interval", q.interval.trim());
  if (q.exchange?.trim()) params.set("exchange", q.exchange.trim().toLowerCase());
  if (q.segment?.trim()) params.set("segment", q.segment.trim().toLowerCase());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/market-context/latest?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("market-context/latest", r, t);
  return JSON.parse(t) as MarketContextLatestApiResponse;
}

/** F7 — filtreli motor listesi + TA / confluence kısa alanlar (`GET .../market-context/summary`). */
export type MarketContextSummaryConfluenceApi = {
  regime?: string;
  composite_score?: number;
  confidence_0_100?: number;
  lot_scale_hint?: number;
  conflicts_count?: number;
  conflict_codes_preview?: string[];
  computed_at?: string;
  error?: string | null;
};

export type MarketContextSummaryItemApi = {
  engine_symbol_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  enabled: boolean;
  ta_durum?: string;
  ta_piyasa_modu?: string;
  confluence?: MarketContextSummaryConfluenceApi | null;
};

export async function fetchMarketContextSummary(
  accessToken: string,
  q: {
    enabled_only?: boolean;
    limit?: number;
    exchange?: string;
    segment?: string;
    symbol?: string;
  } = {},
): Promise<MarketContextSummaryItemApi[]> {
  const params = new URLSearchParams();
  if (q.enabled_only === false) params.set("enabled_only", "false");
  if (q.limit != null && Number.isFinite(q.limit)) params.set("limit", String(Math.min(200, Math.max(1, q.limit))));
  if (q.exchange?.trim()) params.set("exchange", q.exchange.trim().toLowerCase());
  if (q.segment?.trim()) params.set("segment", q.segment.trim().toLowerCase());
  if (q.symbol?.trim()) params.set("symbol", q.symbol.trim().toUpperCase());
  const qs = params.toString();
  const r = await fetchWithBearerRetry(
    `${API_BASE}/api/v1/analysis/market-context/summary${qs ? `?${qs}` : ""}`,
    accessToken,
    {},
  );
  const t = await r.text();
  if (r.status === 404) return [];
  if (!r.ok) throwQtssApiError("market-context/summary", r, t);
  return JSON.parse(t) as MarketContextSummaryItemApi[];
}

/** Worker’ın `nansen_snapshots` tablosuna yazdığı son token screener sonucu; henüz yoksa `null`. */
export type NansenSnapshotApiRow = {
  snapshot_kind: string;
  request_json: unknown;
  response_json: unknown | null;
  meta_json: unknown | null;
  computed_at: string;
  error: string | null;
};

export async function fetchNansenSnapshot(accessToken: string): Promise<NansenSnapshotApiRow | null> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/nansen/snapshot`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("nansen/snapshot", r, t);
  const j = JSON.parse(t) as NansenSnapshotApiRow | null;
  return j ?? null;
}

/** `nansen_setup_runs` — worker’ın son setup taraması özet satırı. */
export type NansenSetupRunApiRow = {
  id: string;
  computed_at: string;
  request_json: unknown;
  source: string;
  candidate_count: number;
  meta_json: unknown | null;
  error: string | null;
};

/** `nansen_setup_rows` — sıralı aday / seviye çıktısı. */
export type NansenSetupRowApiRow = {
  id: string;
  run_id: string;
  rank: number;
  chain: string;
  token_address: string;
  token_symbol: string;
  direction: string;
  score: number;
  probability: number;
  setup: string;
  key_signals: unknown;
  entry: number;
  stop_loss: number;
  tp1: number;
  tp2: number;
  tp3: number;
  rr: number;
  pct_to_tp2: number;
  ohlc_enriched: boolean;
  raw_metrics: unknown;
};

export type NansenSetupsLatestApiResponse = {
  run: NansenSetupRunApiRow | null;
  rows: NansenSetupRowApiRow[];
  /**
   * Yalnız istemci: sunucu 404 döndüyse (eski `qtss-api` veya yanlış `VITE_API_BASE`).
   * API gövdesinde yoktur.
   */
  setup_endpoint_missing?: boolean;
};

/** Son setup taraması + en iyi 5 LONG ve 5 SHORT satırı (`nansen_setup_*` tabloları; şema `0001_qtss_baseline`). */
export async function fetchNansenSetupsLatest(accessToken: string): Promise<NansenSetupsLatestApiResponse> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/nansen/setups/latest`, accessToken, {});
  const t = await r.text();
  if (r.status === 404) {
    return {
      run: null,
      rows: [],
      setup_endpoint_missing: true,
    };
  }
  if (!r.ok) throwQtssApiError("nansen/setups/latest", r, t);
  const parsed = JSON.parse(t) as NansenSetupsLatestApiResponse;
  return { ...parsed, setup_endpoint_missing: false };
}

export async function postEngineSymbol(
  accessToken: string,
  body: {
    exchange?: string;
    segment?: string;
    symbol: string;
    interval: string;
    label?: string;
    signal_direction_mode?: string;
  },
): Promise<EngineSymbolApiRow> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/symbols`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("engine/symbols POST", r, t);
  return JSON.parse(t) as EngineSymbolApiRow;
}

export type EngineSymbolsBulkTarget = {
  exchange?: string;
  segment?: string;
  symbol: string;
  interval: string;
  label?: string;
  signal_direction_mode?: string;
};

export type EngineSymbolsBulkResponse = {
  inserted: EngineSymbolApiRow[];
  errors: { index: number; message: string }[];
};

export async function postEngineSymbolsBulk(
  accessToken: string,
  body: {
    exchange?: string;
    segment?: string;
    label?: string;
    signal_direction_mode?: string;
    targets: EngineSymbolsBulkTarget[];
  },
): Promise<EngineSymbolsBulkResponse> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/symbols-bulk`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("engine/symbols-bulk POST", r, t);
  return JSON.parse(t) as EngineSymbolsBulkResponse;
}

/** GET …/analysis/intake-playbook/latest */
export type IntakePlaybookRunApiRow = {
  id: string;
  playbook_id: string;
  computed_at: string;
  expires_at: string | null;
  market_mode: string | null;
  confidence_0_100: number;
  key_reason: string | null;
  neutral_guidance: string | null;
  summary_json: unknown;
  inputs_json: unknown;
  meta_json: unknown;
};

export type IntakePlaybookCandidateApiRow = {
  id: string;
  run_id: string;
  rank: number;
  symbol: string;
  chain: string | null;
  direction: string;
  intake_tier: string;
  confidence_0_100: number;
  detail_json: unknown;
  merged_engine_symbol_id: string | null;
  merged_at: string | null;
};

export type IntakePlaybookLatestApiResponse = {
  run: IntakePlaybookRunApiRow | null;
  candidates: IntakePlaybookCandidateApiRow[];
};

export async function fetchIntakePlaybookLatest(
  accessToken: string,
  playbookId: string,
): Promise<IntakePlaybookLatestApiResponse> {
  const params = new URLSearchParams({ playbook_id: playbookId });
  const r = await fetchWithBearerRetry(
    `${API_BASE}/api/v1/analysis/intake-playbook/latest?${params}`,
    accessToken,
    {},
  );
  const t = await r.text();
  if (!r.ok) throwQtssApiError("intake-playbook/latest", r, t);
  return JSON.parse(t) as IntakePlaybookLatestApiResponse;
}

export async function fetchIntakePlaybookRecent(
  accessToken: string,
  limit = 40,
): Promise<IntakePlaybookRunApiRow[]> {
  const params = new URLSearchParams({ limit: String(limit) });
  const r = await fetchWithBearerRetry(
    `${API_BASE}/api/v1/analysis/intake-playbook/recent?${params}`,
    accessToken,
    {},
  );
  const t = await r.text();
  if (!r.ok) throwQtssApiError("intake-playbook/recent", r, t);
  return JSON.parse(t) as IntakePlaybookRunApiRow[];
}

export type IntakePlaybookPromoteApiResponse = {
  engine_symbol: EngineSymbolApiRow;
  candidate_id: string;
  linked_existing_row?: boolean;
  note: string;
};

export async function postIntakePlaybookPromote(
  accessToken: string,
  body: {
    candidate_id: string;
    exchange?: string;
    segment?: string;
    interval?: string;
  },
): Promise<IntakePlaybookPromoteApiResponse> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/intake-playbook/promote`, accessToken, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("intake-playbook/promote", r, t);
  return JSON.parse(t) as IntakePlaybookPromoteApiResponse;
}

export type IntakePlaybookPromoteBulkApiResponse = {
  promoted: { candidate_id: string; engine_symbol: EngineSymbolApiRow; linked_existing_row: boolean }[];
  errors: { candidate_id: string; message: string }[];
};

export async function postIntakePlaybookPromoteBulk(
  accessToken: string,
  body: {
    candidate_ids: string[];
    exchange?: string;
    segment?: string;
    interval?: string;
  },
): Promise<IntakePlaybookPromoteBulkApiResponse> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/intake-playbook/promote-bulk`, accessToken, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("intake-playbook/promote-bulk", r, t);
  return JSON.parse(t) as IntakePlaybookPromoteBulkApiResponse;
}

export async function patchEngineSymbol(
  accessToken: string,
  id: string,
  body: { enabled?: boolean; signal_direction_mode?: string },
): Promise<void> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/symbols/${encodeURIComponent(id)}`, accessToken, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const t = await r.text();
    throwQtssApiError("engine/symbols PATCH", r, t);
  }
}

/** F1: `range_signal_events` — `signal_dashboard.durum` kenarı. */
export type RangeSignalEventApiRow = {
  id: string;
  engine_symbol_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  event_kind: string;
  bar_open_time: string;
  reference_price: number | null;
  source: string;
  payload: unknown;
  created_at: string;
};

export async function fetchEngineRangeSignals(
  accessToken: string,
  opts?: { limit?: number; engineSymbolId?: string },
): Promise<RangeSignalEventApiRow[]> {
  const lim = opts?.limit ?? 80;
  const params = new URLSearchParams({ limit: String(lim) });
  if (opts?.engineSymbolId) params.set("engine_symbol_id", opts.engineSymbolId);
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/analysis/engine/range-signals?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("engine/range-signals", r, t);
  return JSON.parse(t) as RangeSignalEventApiRow[];
}

/** F3: `paper_balances` — Decimal alanları genelde JSON string. */
export type PaperBalanceRow = {
  user_id: string;
  org_id: string;
  quote_balance: string | number;
  base_positions: Record<string, string | number>;
  updated_at: string;
};

export type PaperFillRow = {
  id: string;
  org_id: string;
  user_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  client_order_id: string;
  side: string;
  quantity: string | number;
  avg_price: string | number;
  fee: string | number;
  quote_balance_after: string | number;
  base_positions_after: Record<string, string | number>;
  intent: unknown;
  created_at: string;
};

export type ExchangeFillRowApi = {
  id: string;
  org_id: string;
  user_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  venue_order_id: number;
  venue_trade_id: number | null;
  fill_price: string | number | null;
  fill_quantity: string | number | null;
  fee: string | number | null;
  fee_asset: string | null;
  event_time: string;
  raw_event: unknown | null;
  created_at: string;
};

export async function fetchMyExchangeFills(
  accessToken: string,
  opts?: { limit?: number },
): Promise<ExchangeFillRowApi[]> {
  const params = new URLSearchParams();
  params.set("limit", String(opts?.limit ?? 50));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/fills?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("fills", r, t);
  return JSON.parse(t) as ExchangeFillRowApi[];
}

export async function fetchPaperBalance(accessToken: string): Promise<PaperBalanceRow | null> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/orders/dry/balance`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("orders/dry/balance", r, t);
  return JSON.parse(t) as PaperBalanceRow | null;
}

export async function fetchPaperFills(
  accessToken: string,
  limit = 20,
  sinceRfc3339?: string | null,
): Promise<PaperFillRow[]> {
  const params = new URLSearchParams({ limit: String(limit) });
  if (sinceRfc3339?.trim()) params.set("since", sinceRfc3339.trim());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/orders/dry/fills?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("orders/dry/fills", r, t);
  return JSON.parse(t) as PaperFillRow[];
}

/** `pnl_rollups` dashboard row (`GET /api/v1/dashboard/pnl`). */
export type PnlRollupRowApi = {
  org_id: string;
  exchange: string;
  segment: string;
  symbol: string | null;
  ledger: string;
  bucket: string;
  period_start: string;
  realized_pnl: string | number;
  fees: string | number;
  volume: string | number;
  trade_count: number;
  closed_trade_count: number;
};

export async function fetchDashboardPnlRollups(
  accessToken: string,
  ledger: string,
  bucket: string,
  opts?: { exchange?: string; segment?: string; symbol?: string; limit?: number },
): Promise<PnlRollupRowApi[]> {
  const params = new URLSearchParams({ ledger, bucket });
  if (opts?.exchange?.trim()) params.set("exchange", opts.exchange.trim());
  if (opts?.segment?.trim()) params.set("segment", opts.segment.trim());
  if (opts?.symbol?.trim()) params.set("symbol", opts.symbol.trim().toUpperCase());
  if (typeof opts?.limit === "number" && Number.isFinite(opts.limit)) params.set("limit", String(opts.limit));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/dashboard/pnl?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("dashboard/pnl", r, t);
  return JSON.parse(t) as PnlRollupRowApi[];
}

export type PnlEquityPointApi = {
  t: string;
  equity: string | number;
  realized_pnl: string | number;
  fees: string | number;
};

export async function fetchDashboardPnlEquityCurve(
  accessToken: string,
  ledger: string,
  bucket: string,
  opts?: { exchange?: string; segment?: string; symbol?: string; limit?: number },
): Promise<PnlEquityPointApi[]> {
  const params = new URLSearchParams({ ledger, bucket });
  if (opts?.exchange?.trim()) params.set("exchange", opts.exchange.trim());
  if (opts?.segment?.trim()) params.set("segment", opts.segment.trim());
  if (opts?.symbol?.trim()) params.set("symbol", opts.symbol.trim().toUpperCase());
  if (typeof opts?.limit === "number" && Number.isFinite(opts.limit)) params.set("limit", String(opts.limit));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/dashboard/pnl/equity?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("dashboard/pnl/equity", r, t);
  return JSON.parse(t) as PnlEquityPointApi[];
}

export type PnlRebuildStatsApi = {
  orders_scanned: number;
  orders_with_fills: number;
  rollup_rows_written: number;
};

/** Admin — yeniden `pnl_rollups` üret (live). */
export async function postDashboardPnlRebuild(accessToken: string): Promise<PnlRebuildStatsApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/dashboard/pnl/rebuild`, accessToken, {
    method: "POST",
    headers: {},
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("dashboard/pnl/rebuild", r, t);
  return JSON.parse(t) as PnlRebuildStatsApi;
}

/** Kullanıcının Binance emirleri (`GET /api/v1/orders/binance`). */
export type ExchangeOrderRowApi = {
  id: string;
  org_id: string;
  user_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  client_order_id: string;
  status: string;
  intent: unknown;
  venue_order_id: number | null;
  venue_response: unknown;
  created_at: string;
  updated_at: string;
};

export async function fetchMyBinanceOrders(
  accessToken: string,
  opts?: { limit?: number; sinceRfc3339?: string | null },
): Promise<ExchangeOrderRowApi[]> {
  const params = new URLSearchParams();
  params.set("limit", String(opts?.limit ?? 200));
  if (opts?.sinceRfc3339?.trim()) params.set("since", opts.sinceRfc3339.trim());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/orders/binance?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("orders/binance", r, t);
  return JSON.parse(t) as ExchangeOrderRowApi[];
}

export type ExchangeFillRowApi = {
  id: string;
  org_id: string;
  user_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  venue_order_id: number;
  venue_trade_id: number | null;
  fill_price: string | number | null;
  fill_quantity: string | number | null;
  fee: string | number | null;
  fee_asset: string | null;
  event_time: string;
  raw_event: unknown | null;
  created_at: string;
};

export async function fetchMyBinanceFills(
  accessToken: string,
  limit = 200,
): Promise<ExchangeFillRowApi[]> {
  const params = new URLSearchParams({ limit: String(Math.min(500, Math.max(1, limit))) });
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/orders/binance/fills?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("orders/binance/fills", r, t);
  return JSON.parse(t) as ExchangeFillRowApi[];
}

export async function postMyBinancePlaceOrder(
  accessToken: string,
  intent: unknown,
): Promise<{ client_order_id: string; status: string }> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/orders/binance/place`, accessToken, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ intent }),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("orders/binance/place", r, t);
  return JSON.parse(t) as { client_order_id: string; status: string };
}

export type ReconcileReportApi = {
  venue: string;
  checked_remote_orders: number;
  checked_local_orders: number;
  mismatches: number;
  local_submitted_not_open_on_venue: number;
  remote_open_unknown_locally: number;
  notes: string;
  status_updates_applied?: number | null;
};

export async function postReconcileBinanceSpot(accessToken: string): Promise<ReconcileReportApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/reconcile/binance`, accessToken, {
    method: "POST",
    headers: {},
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("reconcile/binance", r, t);
  return JSON.parse(t) as ReconcileReportApi;
}

export async function postReconcileBinanceFutures(accessToken: string): Promise<ReconcileReportApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/reconcile/binance/futures`, accessToken, {
    method: "POST",
    headers: {},
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("reconcile/binance/futures", r, t);
  return JSON.parse(t) as ReconcileReportApi;
}

/** SPEC §7.2 / F5 — `exchangeInfo` ipucu veya tier0 fallback (hesap anahtarı gerekmez). */
export type BinanceCommissionDefaultsApi = {
  segment: string;
  query_symbol: string | null;
  defaults_bps: { maker_bps: number; taker_bps: number };
  source: string;
};

export async function fetchBinanceCommissionDefaults(
  accessToken: string,
  q: { segment?: string; symbol?: string },
): Promise<BinanceCommissionDefaultsApi> {
  const params = new URLSearchParams();
  params.set("segment", (q.segment ?? "spot").toLowerCase());
  if (q.symbol?.trim()) params.set("symbol", q.symbol.trim().toUpperCase());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/market/binance/commission-defaults?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("commission-defaults", r, t);
  return JSON.parse(t) as BinanceCommissionDefaultsApi;
}

/** F5 — hesaba özel kesir oranları (`exchange_accounts`). */
export type BinanceCommissionAccountApi = {
  symbol: string;
  segment: string;
  maker_rate: string;
  taker_rate: string;
  source: string;
};

export async function fetchBinanceCommissionAccount(
  accessToken: string,
  q: { symbol: string; segment?: string },
): Promise<BinanceCommissionAccountApi> {
  const params = new URLSearchParams();
  params.set("symbol", q.symbol.trim().toUpperCase());
  params.set("segment", (q.segment ?? "spot").toLowerCase());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/market/binance/commission-account?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("commission-account", r, t);
  return JSON.parse(t) as BinanceCommissionAccountApi;
}

/** `notify_outbox` row — mirrors `qtss_storage::NotifyOutboxRow` JSON. */
export type NotifyOutboxRowApi = {
  id: string;
  org_id: string | null;
  event_key: string | null;
  severity: string;
  exchange: string | null;
  segment: string | null;
  symbol: string | null;
  title: string;
  body: string;
  channels: string[];
  status: string;
  attempt_count: number;
  last_error: string | null;
  sent_at: string | null;
  delivery_detail: unknown;
  created_at: string;
  updated_at: string;
};

export async function fetchNotifyOutbox(
  accessToken: string,
  q?: {
    limit?: number;
    status?: string;
    event_key?: string;
    exchange?: string;
    segment?: string;
    symbol?: string;
    q?: string;
  },
): Promise<NotifyOutboxRowApi[]> {
  const params = new URLSearchParams({ limit: String(q?.limit ?? 50) });
  if (q?.status?.trim()) params.set("status", q.status.trim());
  if (q?.event_key?.trim()) params.set("event_key", q.event_key.trim());
  if (q?.exchange?.trim()) params.set("exchange", q.exchange.trim());
  if (q?.segment?.trim()) params.set("segment", q.segment.trim());
  if (q?.symbol?.trim()) params.set("symbol", q.symbol.trim().toUpperCase());
  if (q?.q?.trim()) params.set("q", q.q.trim());
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/notify/outbox?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("notify/outbox", r, t);
  return JSON.parse(t) as NotifyOutboxRowApi[];
}

export async function postNotifyOutbox(
  accessToken: string,
  body: {
    title: string;
    body: string;
    channels?: string[];
    event_key?: string;
    severity?: string;
    exchange?: string;
    segment?: string;
    symbol?: string;
  },
): Promise<NotifyOutboxRowApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/notify/outbox`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("notify/outbox POST", r, t);
  return JSON.parse(t) as NotifyOutboxRowApi;
}

export type NotifyTestResponseApi = {
  status: string;
  receipt?: unknown;
};

/** Immediate send via `qtss-notify` — uses merged DB + env config (`load_notify_config_merged`). */
export async function postNotifyTest(
  accessToken: string,
  body: { channel: string; message?: string; title?: string },
): Promise<NotifyTestResponseApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/notify/test`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("notify/test", r, t);
  return JSON.parse(t) as NotifyTestResponseApi;
}

/** `ai_approval_requests` row — mirrors `qtss_storage::AiApprovalRequestRow` JSON. */
export type AiApprovalRequestRowApi = {
  id: string;
  org_id: string;
  requester_user_id: string;
  status: string;
  kind: string;
  payload: unknown;
  model_hint: string | null;
  admin_note: string | null;
  decided_by_user_id: string | null;
  decided_at: string | null;
  created_at: string;
  updated_at: string;
};

export async function fetchAiApprovalRequests(
  accessToken: string,
  q?: { status?: string; limit?: number },
): Promise<AiApprovalRequestRowApi[]> {
  const params = new URLSearchParams();
  if (q?.status?.trim()) params.set("status", q.status.trim());
  params.set("limit", String(q?.limit ?? 50));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/approval-requests?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/approval-requests", r, t);
  return JSON.parse(t) as AiApprovalRequestRowApi[];
}

export async function postAiApprovalRequest(
  accessToken: string,
  body: { kind?: string; payload: unknown; model_hint?: string },
): Promise<AiApprovalRequestRowApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/approval-requests`, accessToken, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/approval-requests POST", r, t);
  return JSON.parse(t) as AiApprovalRequestRowApi;
}

export async function patchAiApprovalRequest(
  accessToken: string,
  id: string,
  body: { status: "approved" | "rejected"; admin_note?: string },
): Promise<{ updated: number }> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/approval-requests/${encodeURIComponent(id)}`, accessToken, {
    method: "PATCH",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/approval-requests PATCH", r, t);
  return JSON.parse(t) as { updated: number };
}

/** `ai_decisions` chain (qtss-ai) — FAZ 7.1 */
export type AiDecisionListRowApi = {
  id: string;
  created_at: string;
  layer: string;
  symbol: string | null;
  status: string;
  confidence: number | null;
  model_id: string | null;
};

export type AiDecisionDetailRowApi = {
  id: string;
  created_at: string;
  layer: string;
  symbol: string | null;
  model_id: string | null;
  prompt_hash: string | null;
  input_snapshot: unknown;
  raw_output: string | null;
  parsed_decision: unknown;
  status: string;
  approved_by: string | null;
  approved_at: string | null;
  applied_at: string | null;
  expires_at: string | null;
  confidence: number | null;
  meta_json: unknown;
};

export async function fetchAiDecisions(
  accessToken: string,
  q?: { layer?: string; symbol?: string; status?: string; limit?: number },
): Promise<AiDecisionListRowApi[]> {
  const params = new URLSearchParams();
  if (q?.layer?.trim()) params.set("layer", q.layer.trim());
  if (q?.symbol?.trim()) params.set("symbol", q.symbol.trim());
  if (q?.status?.trim()) params.set("status", q.status.trim());
  params.set("limit", String(q?.limit ?? 80));
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/decisions?${params}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/decisions", r, t);
  return JSON.parse(t) as AiDecisionListRowApi[];
}

export async function fetchAiDecisionDetail(
  accessToken: string,
  id: string,
): Promise<AiDecisionDetailRowApi> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/decisions/${encodeURIComponent(id)}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError(`ai/decisions/${id}`, r, t);
  return JSON.parse(t) as AiDecisionDetailRowApi;
}

/** Admin — `DELETE /api/v1/ai/decisions?status=error` removes all failed LLM rows (CASCADE children). */
export async function deleteAiDecisionsByStatus(accessToken: string, status: "error"): Promise<{ deleted: number }> {
  const params = new URLSearchParams({ status });
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/decisions?${params}`, accessToken, {
    method: "DELETE",
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/decisions DELETE", r, t);
  return JSON.parse(t) as { deleted: number };
}

export async function postAiDecisionApprove(
  accessToken: string,
  id: string,
): Promise<{ updated: number }> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/decisions/${encodeURIComponent(id)}/approve`, accessToken, {
    method: "POST",
    headers: {},
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/decisions approve", r, t);
  return JSON.parse(t) as { updated: number };
}

export async function postAiDecisionReject(
  accessToken: string,
  id: string,
): Promise<{ updated: number }> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/decisions/${encodeURIComponent(id)}/reject`, accessToken, {
    method: "POST",
    headers: {},
  });
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/decisions reject", r, t);
  return JSON.parse(t) as { updated: number };
}

export async function fetchAiTacticalDirective(
  accessToken: string,
  symbol: string,
): Promise<unknown> {
  const q = new URLSearchParams({ symbol: symbol.trim() });
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/directives/tactical?${q}`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/directives/tactical", r, t);
  return JSON.parse(t) as unknown;
}

export async function fetchAiPortfolioDirective(accessToken: string): Promise<unknown> {
  const r = await fetchWithBearerRetry(`${API_BASE}/api/v1/ai/directives/portfolio`, accessToken, {});
  const t = await r.text();
  if (!r.ok) throwQtssApiError("ai/directives/portfolio", r, t);
  return JSON.parse(t) as unknown;
}

export type TelegramSetupAnalysisStatusApi = {
  webhook_configured: boolean;
  gemini_configured: boolean;
  trigger_phrase: string;
  gemini_model: string;
  max_buffer_turns: number;
  buffer_ttl_secs: number;
  allowlist_restricts: boolean;
  allowlist_size: number;
  webhook_path: string;
};

export async function fetchTelegramSetupAnalysisStatus(
  accessToken: string,
): Promise<TelegramSetupAnalysisStatusApi> {
  const r = await fetchWithBearerRetry(
    `${API_BASE}/api/v1/telegram-setup-analysis/status`,
    accessToken,
    {},
  );
  const t = await r.text();
  if (!r.ok) throwQtssApiError("telegram-setup-analysis/status", r, t);
  return JSON.parse(t) as TelegramSetupAnalysisStatusApi;
}
