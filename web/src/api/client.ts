import type { ChartOhlcRow } from "../lib/marketBarsToCandles";

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

export async function fetchHealth(): Promise<unknown> {
  const r = await fetch(`${API_BASE}/health`);
  if (!r.ok) throw new Error(`health ${r.status}`);
  return r.json();
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
    let detail = t;
    try {
      const j = JSON.parse(t) as { error?: string; error_description?: string };
      if (j.error_description) detail = j.error_description;
      else if (j.error) detail = j.error;
    } catch {
      /* ham metin */
    }
    const hint =
      r.status === 401
        ? " (401: genelde VITE_OAUTH_CLIENT_SECRET, seed çıktısındaki client_secret ile aynı değil veya client_id DB’de yok.)"
        : "";
    throw new Error(`oauth ${r.status}: ${detail}${hint}`);
  }
  return JSON.parse(t) as TokenResponse;
}

export async function fetchConfigList(accessToken: string): Promise<unknown> {
  const r = await fetch(`${API_BASE}/api/v1/config`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`config ${r.status}: ${t}`);
  }
  return r.json();
}

/** Dashboard rolleri — `app_config.acp_chart_patterns` ile aynı JSON (admin değil). */
export async function fetchChartPatternsConfig(accessToken: string): Promise<unknown> {
  const r = await fetch(`${API_BASE}/api/v1/analysis/chart-patterns-config`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`chart-patterns-config ${r.status}: ${t}`);
  }
  return r.json();
}

/** Dashboard rolleri — `app_config.elliott_wave` veya sunucu varsayılanı. */
export async function fetchElliottWaveConfig(accessToken: string): Promise<unknown> {
  const r = await fetch(`${API_BASE}/api/v1/analysis/elliott-wave-config`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`elliott-wave-config ${r.status}: ${t}`);
  }
  return r.json();
}

/** Yalnızca admin — `POST /api/v1/config`. */
export async function upsertAppConfig(
  accessToken: string,
  body: { key: string; value: unknown; description?: string },
): Promise<unknown> {
  const r = await fetch(`${API_BASE}/api/v1/config`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`config upsert ${r.status}: ${t}`);
  }
  return r.json();
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
  const r = await fetch(`${API_BASE}/api/v1/market/bars/recent?${q}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`market/bars/recent ${r.status}: ${t}`);
  }
  return r.json() as Promise<MarketBarRow[]>;
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
  const seg = params.segment?.trim();
  if (seg) q.set("segment", seg);
  const r = await fetch(`${API_BASE}/api/v1/market/binance/klines?${q}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`market/binance/klines ${r.status}: ${t.slice(0, 200)}`);
  }
  const raw = (await r.json()) as unknown;
  if (!Array.isArray(raw)) throw new Error("klines yanıtı dizi değil");
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
  const r = await fetch(`${API_BASE}/api/v1/market/binance/bars/backfill`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
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
    throw new Error(`market/binance/bars/backfill ${r.status}: ${t}`);
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

export type PatternMatchPayloadJson = {
  outcome: ChannelSixOutcomeJson;
  pattern_name?: string;
  pattern_drawing_batch?: PatternDrawingBatchJson;
};

export type ChannelSixResponse = {
  matched: boolean;
  bar_count: number;
  zigzag_pivot_count: number;
  /** İstekteki `repaint` yansıması (Pine açık mum). */
  repaint?: boolean;
  reject?: ChannelSixRejectJson;
  outcome?: ChannelSixOutcomeJson;
  drawing?: ChannelSixDrawingJson;
  pattern_name?: string;
  pattern_drawing_batch?: PatternDrawingBatchJson;
  pattern_matches?: PatternMatchPayloadJson[];
  used_zigzag?: { length: number; depth: number };
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
  const r = await fetch(`${API_BASE}/api/v1/analysis/patterns/channel-six`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) {
    throw new Error(`channel-six ${r.status}: ${t.slice(0, 400)}`);
  }
  return JSON.parse(t) as ChannelSixResponse;
}
