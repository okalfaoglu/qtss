pub mod account_drawdown;
pub mod ai_approval;
pub mod audit_log;
pub mod catalog;
pub mod config;
pub mod config_tick;
pub mod copy_trade;
pub mod copy_trade_jobs;
pub mod data_snapshots;
pub mod engine_analysis;
pub mod error;
pub mod exchange_accounts;
pub mod exchange_fills;
pub mod exchange_orders;
pub mod external_fetch;
pub mod fill_long_estimate;
pub mod ingestion_state;
pub mod intake_playbook;
pub mod market_bars;
pub mod market_confluence_snapshots;
pub mod nansen;
pub mod nansen_enriched;
pub mod nansen_setup_scan;
pub mod notify_outbox;
pub mod onchain_signal_scores;
pub mod paper;
pub mod pnl;
pub mod range_signal_execution;
pub mod migration_checksum_sync;
pub mod pool;
pub mod range_engine;
pub mod regime_param_overrides;
pub mod regime_snapshots;
pub mod regime_transitions;
pub mod reconcile_reports;
pub mod system_config;
pub mod system_config_audit;
pub mod user_permissions;
pub mod users;
pub mod v2_confluence;
pub mod v2_correlation_groups;
pub mod v2_detection_outcomes;
pub mod v2_detections;
pub mod v2_onchain_metrics;
pub mod v2_setup_events;
pub mod v2_setup_rejections;
pub mod v2_setups;
pub mod wyckoff_structures;

pub use account_drawdown::{AccountDrawdownRepository, DrawdownSnapshotRow};
pub use v2_detection_outcomes::{DetectionOutcomeRepository, DetectionOutcomeRow, OutcomeHitRate};
pub use v2_detections::{
    DetectionFilter, DetectionRow, HistoricalOutcomeRow, NewDetection, V2DetectionRepository,
};
pub use v2_correlation_groups::{
    count_open_setups_in_group, list_groups_for_symbol, CorrelationGroupRow,
};
pub use v2_setup_events::{
    insert_v2_setup_event, list_events_for_setup, list_pending_setup_events,
    mark_setup_event_delivered, mark_setup_event_failed, V2SetupEventInsert, V2SetupEventRow,
};
pub use v2_setup_rejections::{
    insert_v2_setup_rejection, list_recent_setup_rejections, V2SetupRejectionInsert,
    V2SetupRejectionRow,
};
pub use v2_setups::{
    fetch_v2_setup, insert_v2_setup, list_open_v2_setups, list_recent_v2_setups,
    update_v2_setup_state, V2SetupInsert, V2SetupRow,
};

pub use wyckoff_structures::{
    complete_wyckoff_structure, fail_wyckoff_structure, find_active_wyckoff_structure,
    get_wyckoff_structure, insert_wyckoff_structure, list_active_wyckoff_structures,
    list_wyckoff_history, update_wyckoff_structure, WyckoffStructureInsert, WyckoffStructureRow,
};

pub use regime_snapshots::{
    insert_regime_snapshot, latest_regime_snapshot, latest_snapshots_all,
    latest_snapshots_for_symbol, purge_old_snapshots, regime_timeline,
    RegimeSnapshotInsert, RegimeSnapshotRow,
};
pub use regime_transitions::{
    insert_regime_transition, list_active_transitions, list_recent_transitions,
    resolve_transition, RegimeTransitionInsert, RegimeTransitionRow,
};
pub use regime_param_overrides::{
    get_override as get_regime_override, list_all_overrides, list_overrides_for_regime,
    resolve_regime_f64, upsert_override as upsert_regime_override,
    RegimeParamOverrideRow,
};

pub use ai_approval::{AiApprovalRepository, AiApprovalRequestRow};
pub use audit_log::{
    insert_http_audit, list_recent as audit_list_recent, AuditHttpListRow, AuditHttpRow,
};
pub use catalog::{
    is_binance_futures_tradable, resolve_series_catalog_ids, ui_segment_to_market_keys,
    BarIntervalRow, CatalogRepository, ExchangeRow, InstrumentRow, MarketRow, SeriesCatalogIds,
};
pub use config::{AppConfigEntry, AppConfigRepository};
pub use config_tick::{
    decimal_from_config_value, normalize_notify_locale_code, resolve_notify_default_locale,
    resolve_nansen_loop_default_on, resolve_nansen_loop_opt_in, resolve_system_csv,
    resolve_system_decimal, resolve_system_f64, resolve_system_string, resolve_system_u64,
    resolve_worker_enabled_flag, resolve_worker_tick_secs, tick_secs_from_config_value,
};
pub use copy_trade::{CopySubscriptionRepository, CopySubscriptionRow};
pub use copy_trade_jobs::{CopyTradeJobRepository, CopyTradeJobRow};
pub use data_snapshots::{
    data_snapshot_age_secs, fetch_data_snapshot, fetch_data_snapshot_for_external_http_source,
    list_data_snapshots, list_snapshots_for_external_http_sources, upsert_data_snapshot,
    DataSnapshotRow,
};
pub use engine_analysis::{
    count_engine_symbols_by_lifecycle, delete_engine_symbol, fetch_analysis_snapshot_payload,
    fetch_engine_symbol_by_id, fetch_engine_symbol_by_series, fetch_sibling_tbm_snapshots,
    has_analysis_snapshots, has_applied_tactical_for_symbol, insert_engine_symbol,
    insert_range_signal_event, lifecycle_state_summary, list_analysis_snapshots_with_symbols,
    list_enabled_engine_symbols, list_engine_symbols_all, list_engine_symbols_by_lifecycle,
    list_engine_symbols_matching, list_market_context_summaries, list_range_signal_events_joined,
    list_stale_lifecycle_engine_symbols, net_filled_position_for_symbol,
    update_engine_symbol_enabled, update_engine_symbol_lifecycle_and_enabled,
    update_engine_symbol_lifecycle_state, update_engine_symbol_patch, upsert_analysis_snapshot,
    AnalysisSnapshotJoinedRow, EngineSymbolInsert, EngineSymbolRow, MarketContextSummaryRow,
    RangeSignalEventInsert, RangeSignalEventJoinedRow,
};
pub use error::StorageError;
pub use exchange_accounts::{ExchangeAccountRepository, ExchangeCredentials};
pub use exchange_fills::{ExchangeFillRepository, ExchangeFillRow};
pub use exchange_orders::{ExchangeOrderRepository, ExchangeOrderRow};
pub use external_fetch::{
    delete_external_source, ensure_binance_sources_for_active_symbols,
    external_snapshot_age_secs, list_enabled_external_sources, list_external_sources,
    upsert_external_source, ExternalDataSourceRow,
};
pub use fill_long_estimate::{symbols_with_open_positions_from_fills, symbols_with_positive_long_from_fills, FillPositionKey};
pub use ingestion_state::{
    count_market_bars_series, list_engine_symbols_with_ingestion,
    list_recent_bar_open_times_desc, upsert_engine_symbol_ingestion_state,
    EngineSymbolIngestionJoinedRow,
};
pub use intake_playbook::{
    fetch_intake_playbook_candidate_by_id, fetch_intake_playbook_run_by_id,
    fetch_latest_intake_playbook_run, insert_intake_playbook_candidates, insert_intake_playbook_run,
    list_intake_playbook_candidates_for_run, list_promotable_intake_candidates,
    list_recent_intake_playbook_runs, update_intake_candidate_merged_engine_symbol,
    IntakePlaybookCandidateInsert, IntakePlaybookCandidateRow, IntakePlaybookRunInsert,
    IntakePlaybookRunRow,
};
pub use market_bars::{
    fetch_recent_bars_stats, list_bars_in_range, list_recent_bars, list_recent_bars_before,
    upsert_market_bar, MarketBarRow,
    MarketBarUpsert, RecentBarsStats,
};
pub use market_confluence_snapshots::{
    insert_market_confluence_snapshot, list_market_confluence_snapshots_for_symbol,
    MarketConfluenceSnapshotInsert, MarketConfluenceSnapshotRow,
};
pub use nansen::{fetch_nansen_snapshot, upsert_nansen_snapshot, NansenSnapshotRow};
pub use nansen_setup_scan::{
    fetch_latest_nansen_setup_with_rows, insert_nansen_setup_row, insert_nansen_setup_run,
    NansenSetupRowDetail, NansenSetupRowInsert, NansenSetupRunInsert, NansenSetupRunRow,
};
pub use notify_outbox::{NotifyOutboxRepository, NotifyOutboxRow};
pub use onchain_signal_scores::{
    delete_onchain_signal_scores_older_than, fetch_latest_onchain_signal_score,
    insert_onchain_signal_score, list_onchain_signal_scores_history, OnchainSignalScoreInsert,
    OnchainSignalScoreRow,
};
pub use paper::{
    PaperBalanceRow, PaperFillRow, PaperLedgerRepository, PAPER_LEDGER_DEFAULT_STRATEGY_KEY,
};
pub use pnl::{
    sum_today_daily_realized_pnl, PnlBucket, PnlRebuildStats, PnlRollupRepository, PnlRollupRow,
};
pub use migration_checksum_sync::sync_sqlx_migration_checksums_from_disk;
pub use pool::{create_pool, run_migrations};
pub use range_engine::{
    clear_refresh_requested, default_range_engine_json, fetch_range_engine_json, merge_json_deep,
    upsert_range_engine_json, RANGE_ENGINE_APP_CONFIG_KEY,
};
pub use reconcile_reports::{ReconcileReportRepository, ReconcileReportRow};
pub use range_signal_execution::{
    list_range_signal_events_pending_paper_execution, try_claim_range_signal_event_for_paper_execution,
    update_range_signal_paper_execution_status, RangeSignalEventPendingExecutionRow,
};
pub use system_config::{SystemConfigRepository, SystemConfigRow};
pub use system_config_audit::{SystemConfigAuditRepository, SystemConfigAuditRow};
pub use user_permissions::UserPermissionRepository;
pub use users::UserRepository;
