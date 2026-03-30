# QTSS — Configuration registry (`system_config` + conventions)

Central operational parameters live in PostgreSQL table **`system_config`** (migration `0044_system_config.sql`; worker seeds in **`0045_…`**, **`0046_…`**, **`0047_worker_kill_switch_tick_secs.sql`**). Product/analysis JSON blobs stay in **`app_config`**. This document lists **module** names, naming rules, and secrets policy (FAZ 11.2 / 11.3).

## Rules

- **`system_config`**: `module` + `config_key` (`snake_case`, unique per module). Value is **JSONB**; prefer objects with `schema_version` in code paths when shapes evolve.
- **`app_config`**: Existing rows use `key` text; **no** `module` column was added. Optional convention: prefix keys logically (`confluence_*`, `nansen_*`, `ai_engine_config`) — do **not** introduce both `app_config.module` and ad-hoc prefixes for the same row without a migration plan.
- **Secrets**: `is_secret = true` → list APIs mask JSON as `{ "_masked": true }`; **get** by module/key returns the real value for admins. Prefer storing tokens in **env / secret store** (stage A); DB may hold only non-secret toggles and references.
- **Env overrides**: Set `QTSS_CONFIG_ENV_OVERRIDES=1` for disaster recovery so code using `qtss_common::env_override` can prefer env over DB where wired (incremental; FAZ 11.5).

## Modules (initial set)

| `module`   | Purpose |
|------------|---------|
| `worker`   | Tick intervals, feature flags for background jobs |
| `api`      | Rate limits, HTTP-facing toggles |
| `notify`   | Non-secret notify defaults (not bot tokens) |
| `nansen`   | Polling hints (non-secret) |
| `ai`       | AI worker documentation keys, optional non-secret hints |
| `execution`| Routing / venue hints (non-secret) |
| `oauth`    | OAuth-related non-secret defaults |
| `metrics`  | Scrape or probe-related toggles |

## Example rows

| module   | config_key                 | Example `value` |
|----------|----------------------------|-----------------|
| `ai`     | `worker_doc`               | `{"note":"QTSS_AI_ENGINE_WORKER=0 disables AI spawn in qtss-worker."}` |
| `worker` | `notify_outbox_tick_secs`  | `{"secs":10}` — poll interval for `notify_outbox` consumer; env `QTSS_NOTIFY_OUTBOX_TICK_SECS`; see `qtss_storage::resolve_worker_tick_secs`. |
| `worker` | `pnl_rollup_tick_secs`     | `{"secs":300}` — PnL rollup rebuild interval; env `QTSS_PNL_ROLLUP_TICK_SECS` (min 60s in worker). |
| `worker` | `notify_default_locale`    | `{"code":"tr"}` — default locale for worker bilingual notify copy (`en` / `tr`); env `QTSS_NOTIFY_DEFAULT_LOCALE`. |
| `worker` | `paper_position_notify_tick_secs` | `{"secs":30}` — paper position notify loop interval; env `QTSS_NOTIFY_POSITION_TICK_SECS`; min **10s** in worker (`paper_fill_notify`). |
| `worker` | `live_position_notify_tick_secs`   | `{"secs":45}` — live position notify loop interval; env `QTSS_NOTIFY_LIVE_TICK_SECS`; min **15s** in worker (`live_position_notify`). |
| `worker` | `kill_switch_db_sync_tick_secs`    | `{"secs":5}` — `app_config.kill_switch_trading_halted` ↔ in-process halt sync; env `QTSS_KILL_SWITCH_DB_SYNC_SECS`; min **2s**. |
| `worker` | `kill_switch_pnl_poll_tick_secs`   | `{"secs":60}` — `kill_switch_loop` daily PnL poll when enabled; env `QTSS_KILL_SWITCH_TICK_SECS`; min **15s**. |
| `worker` | `ai_expire_stale_decisions_tick_secs` | `{"secs":300}` — `qtss_ai::expire_stale_ai_decisions_loop`: pending → `expired` sweep; env `QTSS_AI_EXPIRE_STALE_TICK_SECS`; min **60s**. |
| `worker` | `engine_analysis_tick_secs` | `{"secs":120}` — `qtss_analysis::engine_analysis_loop` (`trading_range` / `signal_dashboard` snapshots); env `QTSS_ENGINE_TICK_SECS`; min **15s**. |
| `worker` | `onchain_signal_tick_secs` | `{"secs":60}` — `onchain_signal_scorer::onchain_signal_loop`; env `QTSS_ONCHAIN_SIGNAL_TICK_SECS`; min **30s**. |
| `worker` | `position_manager_tick_secs` | `{"secs":10}` — `position_manager_loop`; env `QTSS_POSITION_MANAGER_TICK_SECS`; min **5s**. |
| `worker` | `nansen_token_screener_tick_secs` | `{"secs":1800}` — `nansen_token_screener_loop`; env `NANSEN_TICK_SECS`; min **60s**. |
| `worker` | `nansen_netflows_tick_secs` | `{"secs":1800}` — `nansen_netflows_loop`; env `NANSEN_NETFLOWS_TICK_SECS`; min **900s**. |
| `worker` | `nansen_holdings_tick_secs` | `{"secs":1800}` — `nansen_holdings_loop`; env `NANSEN_HOLDINGS_TICK_SECS`; min **900s**. |
| `worker` | `nansen_perp_trades_tick_secs` | `{"secs":1800}` — `nansen_perp_trades_loop`; env `NANSEN_PERP_TRADES_TICK_SECS`; min **900s**. |
| `worker` | `nansen_who_bought_tick_secs` | `{"secs":1800}` — `nansen_who_bought_loop`; env `NANSEN_WHO_BOUGHT_TICK_SECS`; min **900s**. |
| `worker` | `nansen_flow_intel_tick_secs` | `{"secs":900}` — `nansen_flow_intel_loop`; env `NANSEN_FLOW_INTEL_TICK_SECS`; min **600s**. |
| `worker` | `nansen_perp_leaderboard_tick_secs` | `{"secs":604800}` — `nansen_perp_leaderboard_loop`; env `NANSEN_PERP_LEADERBOARD_TICK_SECS`; min **3600s**. |
| `worker` | `nansen_whale_perp_positions_tick_secs` | `{"secs":1800}` — `nansen_whale_perp_aggregate_loop`; env `NANSEN_WHALE_PERP_POSITIONS_TICK_SECS`; min **600s**. |

## Admin API

- **List**: `GET /api/v1/admin/system-config?module=ai` (masked secrets) or list all with pagination cap.
- **Get**: `GET /api/v1/admin/system-config/{module}/{key}` (unmasked for editing).
- **Upsert**: `POST /api/v1/admin/system-config` with body `{ module, config_key, value, description?, is_secret?, schema_version? }`.
- **Delete**: `DELETE /api/v1/admin/system-config/{module}/{key}`.

Requires **admin** role; align with `docs/SECURITY.md` and `QTSS_AUDIT_HTTP`.

## PR checklist (new module or key)

1. Add a row description here or extend the table above.
2. Use English `config_key` identifiers.
3. Migration seed only **non-secret** defaults; use `ON CONFLICT DO NOTHING`.
4. Update `migrations/README.md` if adding a new migration file.
5. Wire read path in Rust with a typed helper or `SystemConfigRepository::get` — avoid duplicating raw SQL.

## See also

- `docs/QTSS_MASTER_DEV_GUIDE.md` — FAZ 11, Bölüm 6 bootstrap policy.
- Kök `.env.example` — AI + `QTSS_CONFIG_ENV_OVERRIDES`.
