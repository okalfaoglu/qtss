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

export interface DetectionOverlay {
  id: string;
  kind: string;
  label: string;
  anchor_time: string;
  anchor_price: string;
  confidence: string;
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

export interface DashboardSnapshot {
  portfolio: PortfolioCard;
  risk: RiskCard;
  open_positions: OpenPositionView[];
  equity_curve: EquityPoint[];
  generated_at: string;
}
