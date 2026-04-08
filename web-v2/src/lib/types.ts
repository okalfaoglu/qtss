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

export interface DashboardSnapshot {
  portfolio: PortfolioCard;
  risk: RiskCard;
  open_positions: OpenPositionView[];
  equity_curve: EquityPoint[];
  generated_at: string;
}
