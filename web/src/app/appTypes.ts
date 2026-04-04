export type Theme = "dark" | "light";

export type SettingsTab =
  | "general"
  | "dashboard"
  | "elliott"
  | "elliott_impulse"
  | "elliott_corrective"
  | "acp"
  | "backtest"
  | "orders"
  | "commission"
  | "engine"
  | "trading_range"
  | "signal_dashboard"
  | "market_context"
  | "nansen"
  | "queues"
  | "ai"
  | "notify"
  | "tbm"
  | "help"
  | "setting";

export type TradingRangeDrawerSubtab = "main" | "data_entry" | "setup" | "trade_summary";

export type AiDrawerSubtab = "ai_dashboard" | "ai_decisions" | "ai_performance" | "ai_settings";

export type ElliottLineStyle = "solid" | "dotted" | "dashed";
