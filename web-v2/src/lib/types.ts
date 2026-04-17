// Wire types — these mirror the DTOs in `qtss-gui-api`. Keep field names in
// snake_case to match the JSON exactly; we don't translate to camelCase.

export interface PortfolioCard {
  equity: string;
  cash: string;
  realized_pnl: string;
  unrealized_pnl: string;
  open_position_count: number;
  open_notional: string;
}

export interface RiskCard {
  drawdown_pct: string;
  daily_loss_pct: string;
  leverage: string;
  killswitch_armed: boolean;
  any_breached: boolean;
}

export interface OpenPositionView {
  symbol: string;
  venue: string;
  side: string;
  quantity: string;
  entry_price: string;
  mark_price: string;
  unrealized_pnl: string;
  unrealized_pnl_pct: string;
}

export interface EquityPoint {
  ts: string;
  equity: string;
}

export interface RiskGauge {
  label: string;
  value: string;
  cap: string;
  utilization: string;
  breached: boolean;
}

export interface RiskHud {
  generated_at: string;
  kill_switch_manual: boolean;
  any_breached: boolean;
  kill_switch_armed: boolean;
  gauges: RiskGauge[];
}

export type BlotterEntry =
  | {
      kind: "order";
      at: string;
      venue: string;
      segment: string;
      symbol: string;
      side: string;
      order_type: string;
      quantity: string | null;
      price: string | null;
      status: string;
      venue_order_id: number | null;
    }
  | {
      kind: "fill";
      at: string;
      venue: string;
      segment: string;
      symbol: string;
      venue_order_id: number;
      venue_trade_id: number | null;
      price: string | null;
      quantity: string | null;
      fee: string | null;
      fee_asset: string | null;
    };

export interface BlotterFeed {
  generated_at: string;
  entries: BlotterEntry[];
}

export type StrategyStatus = "active" | "paused" | "disabled";

export type StrategyParam =
  | { kind: "number"; key: string; value: number }
  | { kind: "integer"; key: string; value: number }
  | { kind: "bool"; key: string; value: boolean }
  | { kind: "text"; key: string; value: string };

export interface StrategyCard {
  id: string;
  label: string;
  evaluator: string;
  status: StrategyStatus;
  params: StrategyParam[];
  signals_seen: number;
  intents_emitted: number;
  last_signal_at: string | null;
  last_intent_at: string | null;
}

export interface StrategyManagerView {
  generated_at: string;
  strategies: StrategyCard[];
}

export interface FanBand {
  percentile: number;
  values: string[];
}

export interface MonteCarloFan {
  generated_at: string;
  venue: string;
  symbol: string;
  timeframe: string;
  horizon_bars: number;
  paths_simulated: number;
  anchor_price: string;
  bands: FanBand[];
}

export type RegimeKind =
  | "trending_up"
  | "trending_down"
  | "ranging"
  | "squeeze"
  | "volatile"
  | "uncertain";

export type TrendStrength = "none" | "weak" | "moderate" | "strong" | "very_strong";

export interface RegimeView {
  at: string;
  kind: RegimeKind;
  trend_strength: TrendStrength;
  adx: string;
  bb_width: string;
  atr_pct: string;
  choppiness: string;
  confidence: number;
}

export interface RegimePoint {
  at: string;
  kind: RegimeKind;
  confidence: number;
}

export interface RegimeHud {
  generated_at: string;
  venue: string;
  symbol: string;
  timeframe: string;
  current: RegimeView | null;
  history: RegimePoint[];
}

export interface CandleBar {
  open_time: string;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
}

export interface RenkoBrick {
  at: string;
  open: string;
  close: string;
  direction: number;
}

export interface DetectionAnchor {
  time: string;
  price: string;
  label: string | null;
}

export interface DetectionOverlay {
  id: string;
  kind: string;
  label: string;
  family: string;
  subkind: string;
  state: string;
  anchor_time: string;
  anchor_price: string;
  confidence: string;
  invalidation_price: string;
  anchors: DetectionAnchor[];
  // Faz 7.6 / A2 — forward projection (rendered dashed).
  projected_anchors?: DetectionAnchor[];
  // Faz 7.6 / A3 — sub-wave decomposition (rendered fainter/thinner).
  sub_wave_anchors?: DetectionAnchor[][];
  /** Elliott Deep: degree breadcrumb e.g. "Cycle III › Primary [3] › Intermediate (3)" */
  wave_context?: string;
  /** True when sub-waves exist on a lower timeframe — enables drill-down */
  has_children?: boolean;
}

export interface OpenOrderOverlay {
  id: string;
  side: string;
  kind: string;
  price: string | null;
  stop_price: string | null;
  quantity: string;
  status: string;
}

export interface ChartWorkspace {
  generated_at: string;
  venue: string;
  symbol: string;
  timeframe: string;
  candles: CandleBar[];
  renko: RenkoBrick[];
  detections: DetectionOverlay[];
  positions: OpenPositionView[];
  open_orders: OpenOrderOverlay[];
}

export interface TargetBand {
  low: string;
  high: string;
}

export interface ScenarioNode {
  id: string;
  label: string;
  trigger: string;
  probability: string;
  target_band: TargetBand;
  children: ScenarioNode[];
}

export interface ScenarioTree {
  generated_at: string;
  venue: string;
  symbol: string;
  timeframe: string;
  horizon_bars: number;
  anchor_price: string;
  root: ScenarioNode;
}

export interface ConfigEntry {
  module: string;
  config_key: string;
  value: unknown;
  schema_version: number;
  description: string | null;
  is_secret: boolean;
  masked: boolean;
  updated_at: string;
}

export interface ConfigGroup {
  module: string;
  entries: ConfigEntry[];
}

export interface ConfigEditorView {
  generated_at: string;
  groups: ConfigGroup[];
}

export interface ConfigAuditEntry {
  id: number;
  module: string;
  config_key: string;
  action: string;
  old_value: unknown | null;
  new_value: unknown | null;
  changed_by: string | null;
  changed_at: string;
}

export type AiDecisionStatus = "pending" | "approved" | "rejected" | "other";

export interface AiDecisionEntry {
  id: string;
  kind: string;
  status: AiDecisionStatus;
  model_hint: string | null;
  payload_preview: string;
  admin_note: string | null;
  created_at: string;
  decided_at: string | null;
}

export interface AiDecisionsView {
  generated_at: string;
  entries: AiDecisionEntry[];
}

export interface AuditEntry {
  id: string;
  at: string;
  request_id: string | null;
  user_id: string | null;
  org_id: string | null;
  method: string;
  path: string;
  status_code: number;
  roles: string[];
  kind: string | null;
  details_preview: string | null;
}

export interface AuditView {
  generated_at: string;
  entries: AuditEntry[];
}

export interface UserCard {
  id: string;
  email: string;
  display_name: string | null;
  is_admin: boolean;
  created_at: string;
  roles: string[];
  permissions: string[];
}

export interface UsersView {
  generated_at: string;
  users: UserCard[];
}

// Faz 8.0 — Setup Engine feed (mirrors crates/qtss-api/src/routes/v2_setups.rs).
export interface SetupEntry {
  id: string;
  created_at: string;
  updated_at: string;
  venue_class: string;
  exchange: string;
  symbol: string;
  timeframe: string;
  profile: string;
  alt_type: string | null;
  state: string;
  direction: string;
  entry_price: number | null;
  entry_sl: number | null;
  koruma: number | null;
  target_ref: number | null;
  risk_pct: number | null;
  close_reason: string | null;
  close_price: number | null;
  closed_at: string | null;
  pnl_pct: number | null;
  risk_mode: string | null;
  /** Faz 9.3.3 — P(win) stamped at open by the inference sidecar, or null. */
  ai_score: number | null;
  confluence_id: string | null;
  raw_meta: unknown;
}

export interface SetupFeed {
  generated_at: string;
  entries: SetupEntry[];
}

export interface SetupEventEntry {
  id: string;
  created_at: string;
  event_type: string;
  payload: unknown;
  delivery_state: string;
  delivered_at: string | null;
  retries: number;
}

export interface SetupEventsResponse {
  setup_id: string;
  events: SetupEventEntry[];
}

export interface DashboardSnapshot {
  portfolio: PortfolioCard;
  risk: RiskCard;
  open_positions: OpenPositionView[];
  equity_curve: EquityPoint[];
  generated_at: string;
}

// Faz 9.4.1 — AI Shadow observation types.

export interface PredictionRow {
  id: string;
  setup_id: string | null;
  exchange: string;
  symbol: string;
  timeframe: string;
  model_version: string;
  score: number;
  threshold: number;
  gate_enabled: boolean;
  decision: "pass" | "block" | "shadow";
  shap_top10: { feature: string; value: number; contribution: number }[] | null;
  latency_ms: number;
  inference_ts: string;
}

export interface PredictionSummary {
  total: number;
  n_pass: number;
  n_block: number;
  n_shadow: number;
  avg_score: number | null;
  avg_latency_ms: number | null;
  avg_pnl_pass: number | null;
  avg_pnl_block: number | null;
  block_wouldve_won: number;
  block_with_outcome: number;
}

export interface ScoreBucket {
  bucket: number;
  n: number;
  n_pass: number;
  n_block: number;
  n_shadow: number;
}

// ── Feature Inspector ───────────────────────────────────────────────

export interface SourceCoverage {
  source: string;
  spec_version: string;
  n_snapshots: number;
  first_at: string | null;
  last_at: string | null;
  n_features: number;
}

export interface FeatureStat {
  feature: string;
  n: number;
  mean: number | null;
  min_val: number | null;
  max_val: number | null;
  stddev: number | null;
}

export interface FeatureSnapshotRow {
  id: string;
  detection_id: string | null;
  source: string;
  feature_spec_version: string;
  features_json: Record<string, number | string | null>;
  created_at: string;
}
