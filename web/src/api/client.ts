import type { ChartOhlcRow } from "../lib/marketBarsToCandles";
import type { AuthSession } from "../lib/rbac";

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

/** JWT doğrulandıktan sonra rol / org özeti (GUI RBAC). */
export async function fetchAuthMe(accessToken: string): Promise<AuthSession> {
  const r = await fetch(`${API_BASE}/api/v1/me`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) {
    throw new Error(`me ${r.status}: ${t.slice(0, 300)}`);
  }
  const j = JSON.parse(t) as { sub: string; org_id: string; roles: string[]; azp: string };
  return {
    userId: j.sub,
    orgId: j.org_id,
    roles: Array.isArray(j.roles) ? j.roles : [],
    oauthClientId: j.azp,
  };
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
  | "ratio_diff"
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
  /** `pattern_matches` içinde `pivot_tail_skip === 0` ve `zigzag_level === 0` olan ilk satırın indeksi — robot / canlı sinyal. */
  live_robot_match_index?: number | null;
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
  const r = await fetch(`${API_BASE}/api/v1/analysis/engine/symbols`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`engine/symbols ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as EngineSymbolApiRow[];
}

export async function fetchEngineSnapshots(accessToken: string): Promise<EngineSnapshotJoinedApiRow[]> {
  const r = await fetch(`${API_BASE}/api/v1/analysis/engine/snapshots`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`engine/snapshots ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as EngineSnapshotJoinedApiRow[];
}

export async function fetchConfluenceSnapshotsLatest(
  accessToken: string,
): Promise<EngineSnapshotJoinedApiRow[]> {
  const r = await fetch(`${API_BASE}/api/v1/analysis/engine/confluence/latest`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`engine/confluence/latest ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as EngineSnapshotJoinedApiRow[];
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
  const r = await fetch(`${API_BASE}/api/v1/analysis/data-snapshots`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`analysis/data-snapshots ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as DataSnapshotApiRow[];
}

/** PLAN F7 Phase C — tek hedef için TA + confluence + ilgili `data_snapshots`. */
export type MarketContextLatestApiResponse = {
  engine_symbol_id: string;
  exchange: string;
  segment: string;
  symbol: string;
  interval: string;
  technical: {
    signal_dashboard: unknown | null;
    trading_range: unknown | null;
  };
  confluence: unknown | null;
  context_data_snapshots: DataSnapshotApiRow[];
};

export async function fetchMarketContextLatest(
  accessToken: string,
  q: { symbol: string; interval?: string; exchange?: string; segment?: string },
): Promise<MarketContextLatestApiResponse | null> {
  const params = new URLSearchParams();
  params.set("symbol", q.symbol.trim().toUpperCase());
  if (q.interval?.trim()) params.set("interval", q.interval.trim());
  if (q.exchange?.trim()) params.set("exchange", q.exchange.trim().toLowerCase());
  if (q.segment?.trim()) params.set("segment", q.segment.trim().toLowerCase());
  const r = await fetch(`${API_BASE}/api/v1/analysis/market-context/latest?${params}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`market-context/latest ${r.status}: ${t.slice(0, 300)}`);
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
  const r = await fetch(
    `${API_BASE}/api/v1/analysis/market-context/summary${qs ? `?${qs}` : ""}`,
    { headers: { Authorization: `Bearer ${accessToken}` } },
  );
  const t = await r.text();
  if (!r.ok) throw new Error(`market-context/summary ${r.status}: ${t.slice(0, 300)}`);
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
  const r = await fetch(`${API_BASE}/api/v1/analysis/nansen/snapshot`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`nansen/snapshot ${r.status}: ${t.slice(0, 300)}`);
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

/** Son setup taraması + en iyi 5 LONG ve 5 SHORT satırı (`nansen_setup_*`, migration 0020). */
export async function fetchNansenSetupsLatest(accessToken: string): Promise<NansenSetupsLatestApiResponse> {
  const r = await fetch(`${API_BASE}/api/v1/analysis/nansen/setups/latest`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (r.status === 404) {
    return {
      run: null,
      rows: [],
      setup_endpoint_missing: true,
    };
  }
  if (!r.ok) throw new Error(`nansen/setups/latest ${r.status}: ${t.slice(0, 300)}`);
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
  const r = await fetch(`${API_BASE}/api/v1/analysis/engine/symbols`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`engine/symbols POST ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as EngineSymbolApiRow;
}

export async function patchEngineSymbol(
  accessToken: string,
  id: string,
  body: { enabled?: boolean; signal_direction_mode?: string },
): Promise<void> {
  const r = await fetch(`${API_BASE}/api/v1/analysis/engine/symbols/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: {
      Authorization: `Bearer ${accessToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const t = await r.text();
    throw new Error(`engine/symbols PATCH ${r.status}: ${t.slice(0, 300)}`);
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
  const r = await fetch(`${API_BASE}/api/v1/analysis/engine/range-signals?${params}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`engine/range-signals ${r.status}: ${t.slice(0, 300)}`);
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

export async function fetchPaperBalance(accessToken: string): Promise<PaperBalanceRow | null> {
  const r = await fetch(`${API_BASE}/api/v1/orders/dry/balance`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`orders/dry/balance ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as PaperBalanceRow | null;
}

export async function fetchPaperFills(accessToken: string, limit = 20): Promise<PaperFillRow[]> {
  const params = new URLSearchParams({ limit: String(limit) });
  const r = await fetch(`${API_BASE}/api/v1/orders/dry/fills?${params}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`orders/dry/fills ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as PaperFillRow[];
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
  const r = await fetch(`${API_BASE}/api/v1/market/binance/commission-defaults?${params}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`commission-defaults ${r.status}: ${t.slice(0, 300)}`);
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
  const r = await fetch(`${API_BASE}/api/v1/market/binance/commission-account?${params}`, {
    headers: { Authorization: `Bearer ${accessToken}` },
  });
  const t = await r.text();
  if (!r.ok) throw new Error(`commission-account ${r.status}: ${t.slice(0, 300)}`);
  return JSON.parse(t) as BinanceCommissionAccountApi;
}
