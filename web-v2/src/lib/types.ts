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

export interface DashboardSnapshot {
  portfolio: PortfolioCard;
  risk: RiskCard;
  open_positions: OpenPositionView[];
  equity_curve: EquityPoint[];
  generated_at: string;
}
