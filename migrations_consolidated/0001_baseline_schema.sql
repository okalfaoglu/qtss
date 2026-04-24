--
-- PostgreSQL database dump
--

\restrict ndVPCWukwZS8i9ReRo5DYyqmARSzcbX5lgr1YytvyeykGBc9PXk3RW2d7Q6APkc

-- Dumped from database version 17.7 (Ubuntu 17.7-3.pgdg24.04+1)
-- Dumped by pg_dump version 17.7 (Ubuntu 17.7-3.pgdg24.04+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: timescaledb; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS timescaledb WITH SCHEMA public;


--
-- Name: _qtss_register_key(text, text, text, text, jsonb, text, text, text, boolean, text, text[]); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public._qtss_register_key(p_key text, p_category text, p_subcategory text, p_value_type text, p_default jsonb, p_unit text, p_description text, p_ui_widget text, p_requires_restart boolean, p_sensitivity text, p_tags text[]) RETURNS void
    LANGUAGE plpgsql
    AS $$
BEGIN
    INSERT INTO config_schema (
        key, category, subcategory, value_type, default_value,
        unit, description, ui_widget, requires_restart, sensitivity,
        introduced_in, tags
    ) VALUES (
        p_key, p_category, p_subcategory, p_value_type, p_default,
        p_unit, p_description, p_ui_widget, p_requires_restart, p_sensitivity,
        '0194', p_tags
    )
    ON CONFLICT (key) DO NOTHING;
END;
$$;


--
-- Name: audit_log_block_mutation(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.audit_log_block_mutation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    RAISE EXCEPTION 'qtss_audit_log is append-only (% blocked)', TG_OP;
END;
$$;


--
-- Name: fn_system_config_audit(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.fn_system_config_audit() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO system_config_audit (module, config_key, action, new_value, changed_by)
        VALUES (NEW.module, NEW.config_key, 'create', NEW.value, NEW.updated_by_user_id);
        RETURN NEW;
    ELSIF TG_OP = 'UPDATE' THEN
        -- Only log if value actually changed.
        IF OLD.value IS DISTINCT FROM NEW.value THEN
            INSERT INTO system_config_audit (module, config_key, action, old_value, new_value, changed_by)
            VALUES (NEW.module, NEW.config_key, 'update', OLD.value, NEW.value, NEW.updated_by_user_id);
        END IF;
        RETURN NEW;
    ELSIF TG_OP = 'DELETE' THEN
        INSERT INTO system_config_audit (module, config_key, action, old_value, changed_by)
        VALUES (OLD.module, OLD.config_key, 'delete', OLD.value, OLD.updated_by_user_id);
        RETURN OLD;
    END IF;
    RETURN NULL;
END;
$$;


--
-- Name: notify_config_changed(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.notify_config_changed() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    payload JSONB;
BEGIN
    payload := jsonb_build_object(
        'key',      COALESCE(NEW.key, OLD.key),
        'scope_id', COALESCE(NEW.scope_id, OLD.scope_id),
        'action',   TG_OP
    );
    PERFORM pg_notify('config_changed', payload::text);
    RETURN COALESCE(NEW, OLD);
END;
$$;


--
-- Name: qtss_models_sync_active_role(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.qtss_models_sync_active_role() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  -- If the caller updated `role`, derive `active` from it.
  IF TG_OP = 'INSERT' OR NEW.role IS DISTINCT FROM OLD.role THEN
    NEW.active := (NEW.role = 'active');
  -- If the caller only flipped `active`, map it back to role.
  ELSIF NEW.active IS DISTINCT FROM OLD.active THEN
    NEW.role := CASE WHEN NEW.active THEN 'active' ELSE 'archived' END;
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: qtss_v2_detections_set_updated_at(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.qtss_v2_detections_set_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: market_bars; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.market_bars (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    "interval" text NOT NULL,
    open_time timestamp with time zone NOT NULL,
    open numeric NOT NULL,
    high numeric NOT NULL,
    low numeric NOT NULL,
    close numeric NOT NULL,
    volume numeric NOT NULL,
    quote_volume numeric,
    trade_count bigint,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    instrument_id uuid,
    bar_interval_id uuid
);


--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


--
-- Name: ai_approval_requests; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_approval_requests (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    requester_user_id uuid NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    kind text DEFAULT 'generic'::text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    model_hint text,
    admin_note text,
    decided_by_user_id uuid,
    decided_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    gate_scores jsonb,
    rejection_reason text,
    auto_approved boolean DEFAULT false NOT NULL,
    CONSTRAINT ai_approval_requests_status_chk CHECK ((status = ANY (ARRAY['pending'::text, 'approved'::text, 'rejected'::text, 'cancelled'::text])))
);


--
-- Name: ai_decision_outcomes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_decision_outcomes (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    decision_id uuid NOT NULL,
    recorded_at timestamp with time zone DEFAULT now() NOT NULL,
    pnl_pct double precision,
    pnl_usdt double precision,
    outcome text NOT NULL,
    holding_hours double precision,
    notes text,
    CONSTRAINT ai_decision_outcomes_outcome_chk CHECK ((outcome = ANY (ARRAY['profit'::text, 'loss'::text, 'breakeven'::text, 'expired_unused'::text])))
);


--
-- Name: ai_decisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_decisions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    layer text NOT NULL,
    symbol text,
    model_id text,
    prompt_hash text,
    input_snapshot jsonb DEFAULT '{}'::jsonb NOT NULL,
    raw_output text,
    parsed_decision jsonb,
    status text DEFAULT 'pending_approval'::text NOT NULL,
    approved_by text,
    approved_at timestamp with time zone,
    applied_at timestamp with time zone,
    expires_at timestamp with time zone,
    confidence double precision,
    meta_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    approval_request_id uuid,
    CONSTRAINT ai_decisions_layer_chk CHECK ((layer = ANY (ARRAY['strategic'::text, 'tactical'::text, 'operational'::text]))),
    CONSTRAINT ai_decisions_status_chk CHECK ((status = ANY (ARRAY['pending_approval'::text, 'approved'::text, 'applied'::text, 'rejected'::text, 'expired'::text, 'error'::text])))
);


--
-- Name: ai_portfolio_directives; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_portfolio_directives (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    decision_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    valid_until timestamp with time zone,
    risk_budget_pct double precision,
    max_open_positions integer,
    preferred_regime text,
    symbol_scores jsonb DEFAULT '{}'::jsonb NOT NULL,
    macro_note text,
    status text DEFAULT 'pending_approval'::text NOT NULL
);


--
-- Name: ai_position_directives; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_position_directives (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    decision_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    symbol text NOT NULL,
    open_position_ref uuid,
    action text NOT NULL,
    new_stop_loss_pct double precision,
    new_take_profit_pct double precision,
    trailing_callback_pct double precision,
    partial_close_pct double precision,
    reasoning text,
    status text DEFAULT 'pending_approval'::text NOT NULL,
    CONSTRAINT ai_position_directives_action_chk CHECK ((action = ANY (ARRAY['keep'::text, 'tighten_stop'::text, 'widen_stop'::text, 'activate_trailing'::text, 'deactivate_trailing'::text, 'partial_close'::text, 'full_close'::text, 'add_to_position'::text]))),
    CONSTRAINT ai_position_directives_status_chk CHECK ((status = ANY (ARRAY['pending_approval'::text, 'approved'::text, 'applied'::text, 'rejected'::text, 'expired'::text])))
);


--
-- Name: ai_tactical_decisions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.ai_tactical_decisions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    decision_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    valid_until timestamp with time zone NOT NULL,
    symbol text NOT NULL,
    direction text NOT NULL,
    position_size_multiplier double precision DEFAULT 1.0 NOT NULL,
    entry_price_hint double precision,
    stop_loss_pct double precision,
    take_profit_pct double precision,
    reasoning text,
    confidence double precision,
    status text DEFAULT 'pending_approval'::text NOT NULL,
    CONSTRAINT ai_tactical_direction_chk CHECK ((direction = ANY (ARRAY['strong_buy'::text, 'buy'::text, 'neutral'::text, 'sell'::text, 'strong_sell'::text, 'no_trade'::text]))),
    CONSTRAINT ai_tactical_status_chk CHECK ((status = ANY (ARRAY['pending_approval'::text, 'approved'::text, 'applied'::text, 'rejected'::text, 'expired'::text, 'execution_failed'::text])))
);


--
-- Name: analysis_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.analysis_snapshots (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    engine_symbol_id uuid NOT NULL,
    engine_kind text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    last_bar_open_time timestamp with time zone,
    bar_count integer,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    error text
);


--
-- Name: app_config; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.app_config (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    key text NOT NULL,
    value jsonb NOT NULL,
    description text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by_user_id uuid
);


--
-- Name: asset_categories; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.asset_categories (
    id smallint NOT NULL,
    code text NOT NULL,
    label_tr text NOT NULL,
    label_en text,
    description text,
    display_order smallint DEFAULT 0 NOT NULL
);


--
-- Name: audit_log; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_log (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    request_id text,
    user_id uuid,
    org_id uuid,
    method text NOT NULL,
    path text NOT NULL,
    status_code smallint NOT NULL,
    roles text[] DEFAULT '{}'::text[] NOT NULL,
    details jsonb
);


--
-- Name: backfill_progress; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.backfill_progress (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    engine_symbol_id uuid NOT NULL,
    state text DEFAULT 'pending'::text NOT NULL,
    oldest_fetched timestamp with time zone,
    newest_fetched timestamp with time zone,
    bar_count bigint DEFAULT 0 NOT NULL,
    expected_bars bigint,
    gap_count integer DEFAULT 0 NOT NULL,
    max_gap_seconds integer,
    backfill_started_at timestamp with time zone,
    backfill_finished_at timestamp with time zone,
    verified_at timestamp with time zone,
    last_error text,
    pages_fetched integer DEFAULT 0 NOT NULL,
    bars_upserted bigint DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: bar_intervals; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.bar_intervals (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    code text NOT NULL,
    label text,
    duration_seconds integer,
    sort_order integer DEFAULT 0 NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: config_audit; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.config_audit (
    id bigint NOT NULL,
    key text NOT NULL,
    scope_id bigint,
    action text NOT NULL,
    old_value jsonb,
    new_value jsonb,
    changed_by uuid,
    changed_at timestamp with time zone DEFAULT now() NOT NULL,
    reason text NOT NULL,
    correlation uuid,
    hash_prev bytea,
    hash_self bytea,
    CONSTRAINT config_audit_action_chk CHECK ((action = ANY (ARRAY['create'::text, 'update'::text, 'delete'::text, 'rollback'::text, 'migrated_from_v1'::text])))
);


--
-- Name: config_audit_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.config_audit_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: config_audit_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.config_audit_id_seq OWNED BY public.config_audit.id;


--
-- Name: config_schema; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.config_schema (
    key text NOT NULL,
    category text NOT NULL,
    subcategory text,
    value_type text NOT NULL,
    json_schema jsonb DEFAULT '{}'::jsonb NOT NULL,
    default_value jsonb NOT NULL,
    unit text,
    description text NOT NULL,
    ui_widget text,
    requires_restart boolean DEFAULT false NOT NULL,
    is_secret_ref boolean DEFAULT false NOT NULL,
    sensitivity text DEFAULT 'normal'::text NOT NULL,
    deprecated_at timestamp with time zone,
    introduced_in text,
    tags text[] DEFAULT ARRAY[]::text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT config_schema_sensitivity_chk CHECK ((sensitivity = ANY (ARRAY['low'::text, 'normal'::text, 'high'::text]))),
    CONSTRAINT config_schema_value_type_chk CHECK ((value_type = ANY (ARRAY['int'::text, 'float'::text, 'decimal'::text, 'string'::text, 'bool'::text, 'enum'::text, 'object'::text, 'array'::text, 'duration'::text])))
);


--
-- Name: config_scope; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.config_scope (
    id bigint NOT NULL,
    scope_type text NOT NULL,
    scope_key text DEFAULT ''::text NOT NULL,
    parent_id bigint,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT config_scope_type_chk CHECK ((scope_type = ANY (ARRAY['global'::text, 'asset_class'::text, 'venue'::text, 'strategy'::text, 'instrument'::text, 'user'::text])))
);


--
-- Name: config_scope_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.config_scope_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: config_scope_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.config_scope_id_seq OWNED BY public.config_scope.id;


--
-- Name: config_value; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.config_value (
    id bigint NOT NULL,
    key text NOT NULL,
    scope_id bigint NOT NULL,
    value jsonb NOT NULL,
    version integer DEFAULT 1 NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    valid_from timestamp with time zone,
    valid_until timestamp with time zone,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by uuid
);


--
-- Name: config_value_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.config_value_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: config_value_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.config_value_id_seq OWNED BY public.config_value.id;


--
-- Name: confluence_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.confluence_snapshots (
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    bull_score double precision NOT NULL,
    bear_score double precision NOT NULL,
    net_score double precision NOT NULL,
    confidence double precision NOT NULL,
    verdict text NOT NULL,
    contributors jsonb NOT NULL,
    regime text
);


--
-- Name: copy_subscriptions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.copy_subscriptions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    leader_user_id uuid NOT NULL,
    follower_user_id uuid NOT NULL,
    rule jsonb NOT NULL,
    active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: copy_trade_execution_jobs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.copy_trade_execution_jobs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    subscription_id uuid NOT NULL,
    leader_exchange_order_id uuid NOT NULL,
    follower_user_id uuid NOT NULL,
    leader_user_id uuid NOT NULL,
    payload jsonb NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: data_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.data_snapshots (
    source_key text NOT NULL,
    request_json jsonb NOT NULL,
    response_json jsonb,
    meta_json jsonb,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    error text
);


--
-- Name: detections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.detections (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    detected_at timestamp with time zone DEFAULT now() NOT NULL,
    exchange text NOT NULL,
    segment text DEFAULT 'futures'::text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    slot smallint NOT NULL,
    pattern_family text NOT NULL,
    subkind text NOT NULL,
    direction smallint NOT NULL,
    start_bar bigint NOT NULL,
    end_bar bigint NOT NULL,
    start_time timestamp with time zone NOT NULL,
    end_time timestamp with time zone NOT NULL,
    anchors jsonb NOT NULL,
    live boolean,
    next_hint boolean,
    invalidated boolean DEFAULT false NOT NULL,
    raw_meta jsonb DEFAULT '{}'::jsonb NOT NULL,
    mode text DEFAULT 'live'::text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: detections_labeled; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.detections_labeled AS
 SELECT id,
    detected_at,
    exchange,
    segment,
    symbol,
    timeframe,
    slot,
    pattern_family,
    subkind,
    direction,
    start_bar,
    end_bar,
    start_time,
    end_time,
    anchors,
    live,
    next_hint,
    invalidated,
    raw_meta,
    mode,
    created_at,
    updated_at,
    ('Z'::text || (slot + 1)) AS z_label
   FROM public.detections;


--
-- Name: engine_symbol_ingestion_state; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.engine_symbol_ingestion_state (
    engine_symbol_id uuid NOT NULL,
    bar_row_count integer,
    min_open_time timestamp with time zone,
    max_open_time timestamp with time zone,
    gap_count integer DEFAULT 0,
    max_gap_seconds integer,
    last_backfill_at timestamp with time zone,
    last_health_check_at timestamp with time zone,
    last_error text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: engine_symbols; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.engine_symbols (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    exchange text DEFAULT 'binance'::text NOT NULL,
    segment text DEFAULT 'spot'::text NOT NULL,
    symbol text NOT NULL,
    "interval" text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    sort_order integer DEFAULT 0 NOT NULL,
    label text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    signal_direction_mode text DEFAULT 'auto_segment'::text NOT NULL,
    exchange_id uuid,
    market_id uuid,
    instrument_id uuid,
    bar_interval_id uuid,
    lifecycle_state text DEFAULT 'manual'::text NOT NULL,
    source text DEFAULT 'manual'::text NOT NULL,
    pinned boolean DEFAULT true NOT NULL,
    discovered_at timestamp with time zone,
    last_signal_at timestamp with time zone,
    CONSTRAINT engine_symbols_source_chk CHECK ((source = ANY (ARRAY['manual'::text, 'top_volume'::text, 'onchain_discovery'::text])))
);


--
-- Name: exchange_accounts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.exchange_accounts (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    user_id uuid NOT NULL,
    exchange text NOT NULL,
    segment text NOT NULL,
    api_key text NOT NULL,
    api_secret text NOT NULL,
    passphrase text,
    label text,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: exchange_fills; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.exchange_fills (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    user_id uuid NOT NULL,
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    venue_order_id bigint NOT NULL,
    venue_trade_id bigint,
    fill_price numeric,
    fill_quantity numeric,
    fee numeric,
    fee_asset text,
    event_time timestamp with time zone DEFAULT now() NOT NULL,
    raw_event jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: exchange_orders; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.exchange_orders (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    user_id uuid NOT NULL,
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    client_order_id uuid NOT NULL,
    status text DEFAULT 'submitted'::text NOT NULL,
    intent jsonb NOT NULL,
    venue_order_id bigint,
    venue_response jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: exchanges; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.exchanges (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    code text NOT NULL,
    display_name text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: external_data_sources; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.external_data_sources (
    key text NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    method text DEFAULT 'GET'::text NOT NULL,
    url text NOT NULL,
    headers_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    body_json jsonb,
    tick_secs integer DEFAULT 300 NOT NULL,
    description text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT external_data_sources_key_check CHECK ((key ~ '^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$'::text)),
    CONSTRAINT external_data_sources_method_check CHECK ((upper(btrim(method)) = ANY (ARRAY['GET'::text, 'POST'::text]))),
    CONSTRAINT external_data_sources_tick_secs_check CHECK ((tick_secs >= 30))
);


--
-- Name: indicator_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.indicator_snapshots (
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    bar_time timestamp with time zone NOT NULL,
    indicator text NOT NULL,
    "values" jsonb NOT NULL,
    config_hash text,
    computed_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: instruments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.instruments (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    market_id uuid NOT NULL,
    native_symbol text NOT NULL,
    base_asset text NOT NULL,
    quote_asset text NOT NULL,
    status text DEFAULT 'unknown'::text NOT NULL,
    is_trading boolean DEFAULT false NOT NULL,
    price_filter jsonb,
    lot_filter jsonb,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: intake_playbook_candidates; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.intake_playbook_candidates (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    run_id uuid NOT NULL,
    rank integer NOT NULL,
    symbol text NOT NULL,
    chain text,
    direction text NOT NULL,
    intake_tier text DEFAULT 'scan'::text NOT NULL,
    confidence_0_100 integer DEFAULT 0 NOT NULL,
    detail_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    merged_engine_symbol_id uuid,
    merged_at timestamp with time zone
);


--
-- Name: intake_playbook_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.intake_playbook_runs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    playbook_id text NOT NULL,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone,
    market_mode text,
    confidence_0_100 integer DEFAULT 0 NOT NULL,
    key_reason text,
    neutral_guidance text,
    summary_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    inputs_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    meta_json jsonb DEFAULT '{}'::jsonb NOT NULL
);


--
-- Name: job_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.job_runs (
    id bigint NOT NULL,
    job_id bigint NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    finished_at timestamp with time zone,
    status text DEFAULT 'running'::text NOT NULL,
    attempt integer DEFAULT 1 NOT NULL,
    error text,
    output jsonb,
    worker_id text,
    CONSTRAINT job_runs_status_chk CHECK ((status = ANY (ARRAY['running'::text, 'success'::text, 'failed'::text, 'timeout'::text])))
);


--
-- Name: job_runs_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.job_runs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: job_runs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.job_runs_id_seq OWNED BY public.job_runs.id;


--
-- Name: liquidation_guard_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.liquidation_guard_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    position_id uuid NOT NULL,
    severity text NOT NULL,
    action_taken text NOT NULL,
    mark_price numeric(38,18) NOT NULL,
    liquidation_price numeric(38,18) NOT NULL,
    distance_pct numeric(10,6) NOT NULL,
    margin_ratio numeric(10,6),
    details jsonb DEFAULT '{}'::jsonb NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT liquidation_guard_events_action_taken_check CHECK ((action_taken = ANY (ARRAY['none'::text, 'alert'::text, 'add_margin'::text, 'scale_out'::text, 'panic_close'::text]))),
    CONSTRAINT liquidation_guard_events_severity_check CHECK ((severity = ANY (ARRAY['warn'::text, 'high'::text, 'breach'::text])))
);


--
-- Name: live_positions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.live_positions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    user_id uuid NOT NULL,
    setup_id uuid,
    mode text NOT NULL,
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    side text NOT NULL,
    leverage smallint DEFAULT 1 NOT NULL,
    entry_avg numeric(38,18) NOT NULL,
    qty_filled numeric(38,18) NOT NULL,
    qty_remaining numeric(38,18) NOT NULL,
    current_sl numeric(38,18),
    tp_ladder jsonb DEFAULT '[]'::jsonb NOT NULL,
    liquidation_price numeric(38,18),
    maint_margin_ratio numeric(10,6),
    funding_rate_next numeric(10,6),
    last_mark numeric(38,18),
    last_tick_at timestamp with time zone,
    opened_at timestamp with time zone DEFAULT now() NOT NULL,
    closed_at timestamp with time zone,
    close_reason text,
    realized_pnl_quote numeric(38,18),
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT live_positions_mode_check CHECK ((mode = ANY (ARRAY['dry'::text, 'live'::text]))),
    CONSTRAINT live_positions_side_check CHECK ((side = ANY (ARRAY['BUY'::text, 'SELL'::text])))
);


--
-- Name: market_bars_open; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.market_bars_open (
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    "interval" text NOT NULL,
    open_time timestamp with time zone NOT NULL,
    close_time timestamp with time zone NOT NULL,
    open numeric NOT NULL,
    high numeric NOT NULL,
    low numeric NOT NULL,
    close numeric NOT NULL,
    volume numeric NOT NULL,
    trade_count bigint DEFAULT 0 NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: markets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.markets (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    exchange_id uuid NOT NULL,
    segment text NOT NULL,
    contract_kind text DEFAULT ''::text NOT NULL,
    display_name text,
    is_active boolean DEFAULT true NOT NULL,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: nansen_enriched_signals; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.nansen_enriched_signals (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    symbol text NOT NULL,
    signal_type text NOT NULL,
    score double precision NOT NULL,
    direction text DEFAULT 'neutral'::text NOT NULL,
    confidence double precision DEFAULT 0.0 NOT NULL,
    chain_breakdown jsonb,
    details jsonb,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: nansen_raw_flows; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.nansen_raw_flows (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    source_type text NOT NULL,
    chain text,
    token_symbol text,
    token_address text,
    engine_symbol text,
    direction text,
    value_usd double precision,
    balance_pct_change double precision,
    raw_row jsonb NOT NULL,
    snapshot_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: nansen_setup_rows; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.nansen_setup_rows (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    run_id uuid NOT NULL,
    rank integer NOT NULL,
    chain text NOT NULL,
    token_address text NOT NULL,
    token_symbol text NOT NULL,
    direction text NOT NULL,
    score integer NOT NULL,
    probability double precision NOT NULL,
    setup text NOT NULL,
    key_signals jsonb NOT NULL,
    entry double precision NOT NULL,
    stop_loss double precision NOT NULL,
    tp1 double precision NOT NULL,
    tp2 double precision NOT NULL,
    tp3 double precision NOT NULL,
    rr double precision NOT NULL,
    pct_to_tp2 double precision NOT NULL,
    ohlc_enriched boolean DEFAULT false NOT NULL,
    raw_metrics jsonb NOT NULL
);


--
-- Name: nansen_setup_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.nansen_setup_runs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    request_json jsonb NOT NULL,
    source text DEFAULT 'token_screener'::text NOT NULL,
    candidate_count integer DEFAULT 0 NOT NULL,
    meta_json jsonb,
    error text
);


--
-- Name: nansen_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.nansen_snapshots (
    snapshot_kind text NOT NULL,
    request_json jsonb NOT NULL,
    response_json jsonb,
    meta_json jsonb,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    error text
);


--
-- Name: notify_delivery_prefs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.notify_delivery_prefs (
    user_id uuid NOT NULL,
    telegram_chat_id text,
    telegram_enabled boolean DEFAULT false NOT NULL,
    x_handle text,
    x_enabled boolean DEFAULT false NOT NULL,
    telegram_filters jsonb DEFAULT '{}'::jsonb NOT NULL,
    x_filters jsonb DEFAULT '{}'::jsonb NOT NULL,
    notify_entry_touched boolean,
    notify_tp_partial boolean,
    notify_tp_final boolean,
    notify_sl_hit boolean,
    notify_invalidated boolean,
    notify_cancelled boolean,
    notify_daily_digest boolean DEFAULT true NOT NULL,
    notify_weekly_digest boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    last_digest_sent_utc timestamp with time zone
);


--
-- Name: notify_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.notify_outbox (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid,
    title text NOT NULL,
    body text NOT NULL,
    channels jsonb DEFAULT '["webhook"]'::jsonb NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempt_count integer DEFAULT 0 NOT NULL,
    last_error text,
    sent_at timestamp with time zone,
    delivery_detail jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    event_key text,
    severity text DEFAULT 'info'::text NOT NULL,
    exchange text,
    segment text,
    symbol text,
    CONSTRAINT notify_outbox_severity_chk CHECK ((severity = ANY (ARRAY['info'::text, 'warn'::text, 'error'::text]))),
    CONSTRAINT notify_outbox_status_chk CHECK ((status = ANY (ARRAY['pending'::text, 'sending'::text, 'sent'::text, 'failed'::text])))
);


--
-- Name: oauth_clients; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.oauth_clients (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    client_id text NOT NULL,
    client_secret_hash text NOT NULL,
    allowed_grant_types text[] NOT NULL,
    service_user_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: onchain_signal_scores; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.onchain_signal_scores (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    symbol text NOT NULL,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    funding_score double precision,
    oi_score double precision,
    ls_ratio_score double precision,
    taker_vol_score double precision,
    exchange_netflow_score double precision,
    exchange_balance_score double precision,
    hl_bias_score double precision,
    hl_whale_score double precision,
    liquidation_score double precision,
    nansen_sm_score double precision,
    tvl_trend_score double precision,
    aggregate_score double precision NOT NULL,
    confidence double precision DEFAULT 0.5 NOT NULL,
    direction text NOT NULL,
    market_regime text,
    conflict_detected boolean DEFAULT false NOT NULL,
    conflict_detail text,
    snapshot_keys text[] DEFAULT '{}'::text[] NOT NULL,
    meta_json jsonb,
    nansen_netflow_score double precision,
    nansen_perp_score double precision,
    nansen_buyer_quality_score double precision,
    CONSTRAINT onchain_signal_scores_direction_check CHECK ((direction = ANY (ARRAY['strong_buy'::text, 'buy'::text, 'neutral'::text, 'sell'::text, 'strong_sell'::text])))
);


--
-- Name: organizations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organizations (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: paper_balances; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.paper_balances (
    user_id uuid NOT NULL,
    org_id uuid NOT NULL,
    quote_balance numeric NOT NULL,
    base_positions jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    strategy_key text DEFAULT 'default'::text NOT NULL
);


--
-- Name: paper_fills; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.paper_fills (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    user_id uuid NOT NULL,
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    client_order_id uuid NOT NULL,
    side text NOT NULL,
    quantity numeric NOT NULL,
    avg_price numeric NOT NULL,
    fee numeric NOT NULL,
    quote_balance_after numeric NOT NULL,
    base_positions_after jsonb NOT NULL,
    intent jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    strategy_key text DEFAULT 'default'::text NOT NULL
);


--
-- Name: pattern_outcomes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pattern_outcomes (
    exchange text NOT NULL,
    segment text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    slot smallint NOT NULL,
    pattern_family text NOT NULL,
    subkind text NOT NULL,
    start_time timestamp with time zone NOT NULL,
    mode text NOT NULL,
    outcome text NOT NULL,
    tp_hit_count smallint DEFAULT 0 NOT NULL,
    bars_to_outcome integer,
    outcome_time timestamp with time zone,
    outcome_price double precision,
    mfe double precision,
    mae double precision,
    target_json jsonb,
    evaluated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: pivots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pivots (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    engine_symbol_id uuid NOT NULL,
    level smallint NOT NULL,
    bar_index bigint NOT NULL,
    open_time timestamp with time zone NOT NULL,
    direction smallint NOT NULL,
    price numeric NOT NULL,
    volume numeric DEFAULT 0 NOT NULL,
    swing_tag text,
    computed_at timestamp with time zone DEFAULT now() NOT NULL,
    prominence numeric DEFAULT 0 NOT NULL,
    CONSTRAINT pivots_direction_check CHECK ((direction = ANY (ARRAY['-2'::integer, '-1'::integer, 1, 2]))),
    CONSTRAINT pivots_level_check CHECK (((level >= 0) AND (level <= 4))),
    CONSTRAINT pivots_swing_tag_check CHECK ((swing_tag = ANY (ARRAY['HH'::text, 'HL'::text, 'LL'::text, 'LH'::text])))
);


--
-- Name: pnl_rollups; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.pnl_rollups (
    org_id uuid NOT NULL,
    exchange text NOT NULL,
    symbol text DEFAULT ''::text NOT NULL,
    ledger text NOT NULL,
    bucket text NOT NULL,
    period_start timestamp with time zone NOT NULL,
    realized_pnl numeric DEFAULT 0 NOT NULL,
    fees numeric DEFAULT 0 NOT NULL,
    volume numeric DEFAULT 0 NOT NULL,
    trade_count bigint DEFAULT 0 NOT NULL,
    closed_trade_count bigint DEFAULT 0 NOT NULL,
    segment text DEFAULT 'spot'::text NOT NULL
);


--
-- Name: position_scale_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.position_scale_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    position_id uuid NOT NULL,
    event_kind text NOT NULL,
    price numeric(38,18) NOT NULL,
    qty_delta numeric(38,18) NOT NULL,
    qty_after numeric(38,18) NOT NULL,
    entry_avg_after numeric(38,18) NOT NULL,
    realized_pnl_quote numeric(38,18),
    reason text,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT position_scale_events_event_kind_check CHECK ((event_kind = ANY (ARRAY['scale_in'::text, 'scale_out'::text, 'add_on_dip'::text, 'partial_tp'::text, 'ratchet_sl'::text])))
);


--
-- Name: q_radar_portfolio; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.q_radar_portfolio (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    total_capital real DEFAULT 1500000.0 NOT NULL,
    allocated_capital real DEFAULT 0.0 NOT NULL,
    available_capital real DEFAULT 1500000.0 NOT NULL,
    realized_pnl real DEFAULT 0.0 NOT NULL,
    unrealized_pnl real DEFAULT 0.0 NOT NULL,
    open_positions integer DEFAULT 0 NOT NULL,
    total_trades integer DEFAULT 0 NOT NULL,
    win_trades integer DEFAULT 0 NOT NULL,
    loss_trades integer DEFAULT 0 NOT NULL
);


--
-- Name: q_radar_position_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.q_radar_position_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    position_id uuid NOT NULL,
    event_type text NOT NULL,
    quantity real NOT NULL,
    price real NOT NULL,
    pnl real,
    notes text,
    raw_meta jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT q_radar_position_events_event_type_check CHECK ((event_type = ANY (ARRAY['open'::text, 'add_on'::text, 'partial_sell'::text, 'close'::text])))
);


--
-- Name: qtss_positions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_positions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    setup_id uuid NOT NULL,
    symbol text NOT NULL,
    direction text NOT NULL,
    allocated_amount real NOT NULL,
    quantity real DEFAULT 0.0 NOT NULL,
    avg_entry_price real DEFAULT 0.0 NOT NULL,
    total_bought_qty real DEFAULT 0.0 NOT NULL,
    total_sold_qty real DEFAULT 0.0 NOT NULL,
    realized_pnl real DEFAULT 0.0 NOT NULL,
    state text DEFAULT 'open'::text NOT NULL,
    closed_at timestamp with time zone,
    CONSTRAINT qtss_positions_direction_check CHECK ((direction = ANY (ARRAY['long'::text, 'short'::text]))),
    CONSTRAINT qtss_positions_state_check CHECK ((state = ANY (ARRAY['open'::text, 'closed'::text])))
);


--
-- Name: q_radar_positions; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.q_radar_positions AS
 SELECT id,
    created_at,
    updated_at,
    setup_id,
    symbol,
    direction,
    allocated_amount,
    quantity,
    avg_entry_price,
    total_bought_qty,
    total_sold_qty,
    realized_pnl,
    state,
    closed_at
   FROM public.qtss_positions;


--
-- Name: qtss_audit_log; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_audit_log (
    id bigint NOT NULL,
    at timestamp with time zone DEFAULT now() NOT NULL,
    actor text NOT NULL,
    action text NOT NULL,
    subject text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    correlation_id uuid,
    prev_hash bytea,
    row_hash bytea NOT NULL
);


--
-- Name: qtss_audit_log_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.qtss_audit_log_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: qtss_audit_log_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.qtss_audit_log_id_seq OWNED BY public.qtss_audit_log.id;


--
-- Name: qtss_market_regime_daily; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_market_regime_daily (
    day date NOT NULL,
    exchange text NOT NULL,
    sector text DEFAULT '*'::text NOT NULL,
    regime text NOT NULL,
    breadth_pct numeric,
    momentum_20d numeric,
    volatility_index numeric,
    dominant_trend text,
    source text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT qtss_market_regime_chk CHECK ((regime = ANY (ARRAY['risk_on'::text, 'neutral'::text, 'risk_off'::text, 'panic'::text])))
);


--
-- Name: qtss_models; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_models (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    model_family text NOT NULL,
    model_version text NOT NULL,
    feature_spec_version integer NOT NULL,
    algorithm text DEFAULT 'lightgbm'::text NOT NULL,
    task text NOT NULL,
    n_train bigint NOT NULL,
    n_valid bigint NOT NULL,
    metrics_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    params_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    feature_names text[] DEFAULT ARRAY[]::text[] NOT NULL,
    artifact_path text NOT NULL,
    artifact_sha256 text,
    trained_at timestamp with time zone DEFAULT now() NOT NULL,
    trained_by text,
    notes text,
    active boolean DEFAULT false NOT NULL,
    role text DEFAULT 'archived'::text NOT NULL,
    CONSTRAINT qtss_models_role_check CHECK ((role = ANY (ARRAY['active'::text, 'shadow'::text, 'archived'::text])))
);


--
-- Name: qtss_position_health_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_position_health_snapshots (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    setup_id uuid NOT NULL,
    health_score numeric NOT NULL,
    prev_health_score numeric,
    band text NOT NULL,
    prev_band text,
    momentum_score numeric,
    structural_score numeric,
    orderbook_score numeric,
    regime_match_score numeric,
    correlation_score numeric,
    ai_rescore numeric,
    price numeric NOT NULL,
    captured_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: qtss_reports_runs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_reports_runs (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    kind text NOT NULL,
    window_start timestamp with time zone NOT NULL,
    window_end timestamp with time zone NOT NULL,
    generated_at timestamp with time zone DEFAULT now() NOT NULL,
    telegram_ok boolean,
    x_ok boolean,
    body_telegram text,
    body_x text,
    aggregate_json jsonb NOT NULL,
    last_error text,
    CONSTRAINT qtss_reports_runs_kind_check CHECK ((kind = ANY (ARRAY['weekly'::text, 'monthly'::text, 'yearly'::text])))
);


--
-- Name: qtss_roles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_roles (
    id integer NOT NULL,
    name text NOT NULL,
    description text
);


--
-- Name: qtss_roles_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.qtss_roles_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: qtss_roles_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.qtss_roles_id_seq OWNED BY public.qtss_roles.id;


--
-- Name: qtss_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_sessions (
    id uuid NOT NULL,
    user_id bigint NOT NULL,
    issued_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    user_agent text,
    ip_addr inet
);


--
-- Name: qtss_setup_lifecycle_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_setup_lifecycle_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    setup_id uuid NOT NULL,
    event_kind text NOT NULL,
    price numeric NOT NULL,
    pnl_pct numeric,
    pnl_r numeric,
    health_score numeric,
    duration_ms bigint,
    ai_action text,
    ai_reasoning text,
    ai_confidence numeric,
    notify_outbox_id uuid,
    x_outbox_id uuid,
    emitted_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT qtss_setup_lifecycle_events_ai_action_chk CHECK (((ai_action IS NULL) OR (ai_action = ANY (ARRAY['ride'::text, 'scale'::text, 'exit'::text, 'tighten'::text])))),
    CONSTRAINT qtss_setup_lifecycle_events_kind_chk CHECK ((event_kind = ANY (ARRAY['entry_touched'::text, 'tp_hit'::text, 'tp_partial'::text, 'tp_final'::text, 'sl_hit'::text, 'sl_ratcheted'::text, 'invalidated'::text, 'cancelled'::text, 'health_warn'::text, 'health_danger'::text])))
);


--
-- Name: qtss_setup_outcomes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_setup_outcomes (
    setup_id uuid NOT NULL,
    label text NOT NULL,
    close_reason text,
    close_reason_category text NOT NULL,
    realized_rr real,
    pnl_pct real,
    max_favorable_r real,
    max_adverse_r real,
    time_to_outcome_bars integer,
    bars_to_first_tp integer,
    closed_at timestamp with time zone,
    labeled_at timestamp with time zone DEFAULT now() NOT NULL,
    labeler_version integer DEFAULT 1 NOT NULL,
    meta_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT qtss_setup_outcomes_close_reason_category_check CHECK ((close_reason_category = ANY (ARRAY['tp'::text, 'sl'::text, 'manual'::text, 'reverse'::text, 'conflict'::text, 'expiry'::text, 'unknown'::text]))),
    CONSTRAINT qtss_setup_outcomes_label_check CHECK ((label = ANY (ARRAY['win'::text, 'loss'::text, 'neutral'::text, 'invalidated'::text, 'timeout'::text])))
);


--
-- Name: qtss_setups; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_setups (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    venue_class text NOT NULL,
    exchange text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    profile text NOT NULL,
    alt_type text,
    state text NOT NULL,
    direction text NOT NULL,
    confluence_id uuid,
    entry_price real,
    entry_sl real,
    koruma real,
    target_ref real,
    risk_pct real,
    close_reason text,
    close_price real,
    closed_at timestamp with time zone,
    raw_meta jsonb DEFAULT '{}'::jsonb NOT NULL,
    detection_id uuid,
    pnl_pct real,
    risk_mode text,
    mode text DEFAULT 'dry'::text NOT NULL,
    tp_ladder jsonb DEFAULT '[]'::jsonb NOT NULL,
    bars_to_first_tp integer,
    bars_to_close integer,
    max_favorable_r real,
    max_adverse_r real,
    wyckoff_classic jsonb,
    idempotency_key text,
    last_tracked_bar_ts timestamp with time zone,
    ai_score real,
    entry_touched_at timestamp with time zone,
    realized_pnl_pct numeric,
    realized_r numeric,
    tp_hits_bitmap integer DEFAULT 0 NOT NULL,
    remaining_qty_pct numeric DEFAULT 100.0 NOT NULL,
    current_sl numeric,
    ratchet_reference_price numeric,
    ratchet_last_update_at timestamp with time zone,
    ratchet_cumulative_pct numeric DEFAULT 0.0 NOT NULL,
    trail_mode boolean DEFAULT false NOT NULL,
    trail_anchor numeric(18,8),
    ai_advised_tp_idx smallint,
    ai_advised_at timestamp with time zone,
    tp1_hit boolean DEFAULT false NOT NULL,
    tp1_hit_at timestamp with time zone,
    tp1_price real,
    CONSTRAINT qtss_setups_alt_type_check CHECK ((alt_type = ANY (ARRAY['reaction_low'::text, 'trend_low'::text, 'reversal_high'::text, 'selling_high'::text, 'wyckoff_spring'::text, 'wyckoff_lps'::text, 'wyckoff_buec'::text, 'wyckoff_ut'::text, 'wyckoff_utad'::text, 'wyckoff_lpsy'::text, 'wyckoff_ice_retest'::text]))),
    CONSTRAINT qtss_setups_close_reason_chk CHECK (((close_reason IS NULL) OR (close_reason = ANY (ARRAY['tp_final'::text, 'sl_hit'::text, 'trail_stop'::text, 'invalidated'::text, 'cancelled'::text, 'scratch'::text, 'early_warning'::text, 'time_stop'::text, 'hard_invalidation'::text])))),
    CONSTRAINT qtss_setups_direction_check CHECK ((direction = ANY (ARRAY['long'::text, 'short'::text, 'neutral'::text]))),
    CONSTRAINT qtss_setups_mode_check CHECK ((mode = ANY (ARRAY['dry'::text, 'live'::text, 'backtest'::text]))),
    CONSTRAINT qtss_setups_profile_check CHECK ((profile = ANY (ARRAY['t'::text, 'q'::text, 'd'::text]))),
    CONSTRAINT qtss_setups_state_check CHECK ((state = ANY (ARRAY['flat'::text, 'armed'::text, 'active'::text, 'closed'::text, 'closed_win'::text, 'closed_loss'::text, 'closed_manual'::text, 'closed_partial_win'::text, 'closed_scratch'::text]))),
    CONSTRAINT qtss_setups_venue_class_check CHECK ((venue_class = ANY (ARRAY['crypto'::text, 'bist'::text, 'us_equities'::text, 'commodities'::text, 'fx'::text])))
);


--
-- Name: qtss_symbol_profile; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_symbol_profile (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    exchange text NOT NULL,
    symbol text NOT NULL,
    asset_class text NOT NULL,
    category text NOT NULL,
    risk_tier text NOT NULL,
    sector text,
    country text,
    market_cap_usd numeric,
    circulating_supply numeric,
    free_float_pct numeric,
    avg_daily_vol_usd numeric,
    price_usd numeric,
    lot_size numeric,
    tick_size numeric,
    min_notional numeric,
    step_size numeric,
    fundamental_score smallint,
    liquidity_score smallint,
    volatility_score smallint,
    manual_override boolean DEFAULT false NOT NULL,
    notes text,
    source text,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT qtss_symbol_profile_asset_chk CHECK ((asset_class = ANY (ARRAY['crypto'::text, 'equity'::text, 'commodity'::text, 'fx'::text, 'futures'::text, 'index'::text]))),
    CONSTRAINT qtss_symbol_profile_category_chk CHECK ((category = ANY (ARRAY['mega_cap'::text, 'large_cap'::text, 'mid_cap'::text, 'small_cap'::text, 'growth'::text, 'speculative'::text, 'micro_penny'::text, 'holding'::text, 'endeks'::text, 'emtia'::text, 'forex'::text, 'vadeli'::text, 'kripto'::text]))),
    CONSTRAINT qtss_symbol_profile_tier_chk CHECK ((risk_tier = ANY (ARRAY['core'::text, 'balanced'::text, 'growth'::text, 'speculative'::text, 'extreme'::text])))
);


--
-- Name: qtss_user_roles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_user_roles (
    user_id bigint NOT NULL,
    role_id integer NOT NULL
);


--
-- Name: qtss_users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_users (
    id bigint NOT NULL,
    username text NOT NULL,
    email text,
    password_hash text NOT NULL,
    is_active boolean DEFAULT true NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_login_at timestamp with time zone
);


--
-- Name: qtss_users_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.qtss_users_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: qtss_users_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.qtss_users_id_seq OWNED BY public.qtss_users.id;


--
-- Name: qtss_v2_setup_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_v2_setup_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    setup_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    event_type text NOT NULL,
    payload jsonb NOT NULL,
    delivery_state text DEFAULT 'pending'::text NOT NULL,
    delivered_at timestamp with time zone,
    retries integer DEFAULT 0 NOT NULL,
    mode text DEFAULT 'dry'::text NOT NULL,
    CONSTRAINT qtss_v2_setup_events_delivery_state_check CHECK ((delivery_state = ANY (ARRAY['pending'::text, 'delivered'::text, 'failed'::text, 'skipped'::text]))),
    CONSTRAINT qtss_v2_setup_events_event_type_check CHECK ((event_type = ANY (ARRAY['opened'::text, 'updated'::text, 'closed'::text, 'rejected'::text]))),
    CONSTRAINT qtss_v2_setup_events_mode_check CHECK ((mode = ANY (ARRAY['dry'::text, 'live'::text, 'backtest'::text])))
);


--
-- Name: qtss_v2_setup_rejections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.qtss_v2_setup_rejections (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    venue_class text NOT NULL,
    exchange text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    profile text NOT NULL,
    direction text NOT NULL,
    reject_reason text NOT NULL,
    confluence_id uuid,
    raw_meta jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT qtss_v2_setup_rejections_reject_reason_check CHECK ((reject_reason = ANY (ARRAY['total_risk_cap'::text, 'max_concurrent'::text, 'correlation_cap'::text, 'commission_gate'::text, 'gate_kill_switch'::text, 'gate_stale_data'::text, 'gate_news_blackout'::text, 'gate_regime_opposite'::text, 'gate_direction_consensus'::text, 'gate_below_min_score'::text, 'gate_no_direction'::text, 'ai_gate'::text, 'llm_block'::text])))
);


--
-- Name: qtss_v2_setups; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.qtss_v2_setups AS
 SELECT id,
    created_at,
    updated_at,
    venue_class,
    exchange,
    symbol,
    timeframe,
    profile,
    alt_type,
    state,
    direction,
    confluence_id,
    entry_price,
    entry_sl,
    koruma,
    target_ref,
    risk_pct,
    close_reason,
    close_price,
    closed_at,
    raw_meta,
    detection_id,
    pnl_pct,
    risk_mode,
    mode,
    tp_ladder,
    bars_to_first_tp,
    bars_to_close,
    max_favorable_r,
    max_adverse_r,
    wyckoff_classic,
    idempotency_key,
    last_tracked_bar_ts,
    ai_score,
    entry_touched_at,
    realized_pnl_pct,
    realized_r,
    tp_hits_bitmap,
    remaining_qty_pct,
    current_sl,
    ratchet_reference_price,
    ratchet_last_update_at,
    ratchet_cumulative_pct
   FROM public.qtss_setups;


--
-- Name: range_signal_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.range_signal_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    engine_symbol_id uuid NOT NULL,
    event_kind text NOT NULL,
    bar_open_time timestamp with time zone NOT NULL,
    reference_price double precision,
    source text DEFAULT 'signal_dashboard_durum'::text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT range_signal_events_event_kind_check CHECK ((event_kind = ANY (ARRAY['long_entry'::text, 'long_exit'::text, 'short_entry'::text, 'short_exit'::text])))
);


--
-- Name: range_signal_paper_executions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.range_signal_paper_executions (
    range_signal_event_id uuid NOT NULL,
    status text NOT NULL,
    client_order_id uuid,
    error_message text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: reconcile_reports; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.reconcile_reports (
    id bigint NOT NULL,
    user_id uuid NOT NULL,
    venue text NOT NULL,
    overall text NOT NULL,
    position_drifts jsonb DEFAULT '[]'::jsonb NOT NULL,
    order_drifts jsonb DEFAULT '[]'::jsonb NOT NULL,
    position_count integer DEFAULT 0 NOT NULL,
    order_count integer DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT reconcile_reports_overall_check CHECK ((overall = ANY (ARRAY['none'::text, 'soft'::text, 'drift'::text, 'critical'::text])))
);


--
-- Name: reconcile_reports_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.reconcile_reports_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: reconcile_reports_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.reconcile_reports_id_seq OWNED BY public.reconcile_reports.id;


--
-- Name: refresh_tokens; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.refresh_tokens (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    client_uuid uuid NOT NULL,
    user_id uuid NOT NULL,
    token_hash text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    revoked_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: regime_param_overrides; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.regime_param_overrides (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    module text NOT NULL,
    config_key text NOT NULL,
    regime text NOT NULL,
    value jsonb NOT NULL,
    description text,
    created_at timestamp with time zone DEFAULT now(),
    updated_at timestamp with time zone DEFAULT now()
);


--
-- Name: regime_snapshots; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.regime_snapshots (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    symbol text NOT NULL,
    "interval" text NOT NULL,
    regime text NOT NULL,
    trend_strength text,
    confidence double precision NOT NULL,
    adx double precision,
    plus_di double precision,
    minus_di double precision,
    bb_width double precision,
    atr_pct double precision,
    choppiness double precision,
    hmm_state text,
    hmm_confidence double precision,
    computed_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: regime_transitions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.regime_transitions (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    symbol text NOT NULL,
    "interval" text NOT NULL,
    from_regime text NOT NULL,
    to_regime text NOT NULL,
    transition_speed double precision,
    confidence double precision NOT NULL,
    confirming_indicators jsonb DEFAULT '[]'::jsonb,
    hmm_probability double precision,
    detected_at timestamp with time zone DEFAULT now() NOT NULL,
    resolved_at timestamp with time zone,
    was_correct boolean
);


--
-- Name: roles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.roles (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    key text NOT NULL,
    description text
);


--
-- Name: scheduled_jobs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.scheduled_jobs (
    id bigint NOT NULL,
    name text NOT NULL,
    description text,
    schedule_kind text NOT NULL,
    schedule_expr text NOT NULL,
    handler text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    enabled boolean DEFAULT true NOT NULL,
    timeout_s integer DEFAULT 60 NOT NULL,
    max_retries integer DEFAULT 3 NOT NULL,
    next_run_at timestamp with time zone DEFAULT now() NOT NULL,
    last_run_at timestamp with time zone,
    last_status text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT scheduled_jobs_kind_chk CHECK ((schedule_kind = ANY (ARRAY['interval'::text, 'cron'::text]))),
    CONSTRAINT scheduled_jobs_status_chk CHECK (((last_status IS NULL) OR (last_status = ANY (ARRAY['success'::text, 'failed'::text, 'timeout'::text]))))
);


--
-- Name: scheduled_jobs_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.scheduled_jobs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: scheduled_jobs_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.scheduled_jobs_id_seq OWNED BY public.scheduled_jobs.id;


--
-- Name: secrets_vault; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.secrets_vault (
    id bigint NOT NULL,
    name text NOT NULL,
    description text,
    wrapped_dek bytea NOT NULL,
    ciphertext bytea NOT NULL,
    nonce bytea NOT NULL,
    kek_version integer DEFAULT 1 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    rotated_at timestamp with time zone,
    created_by text DEFAULT 'system'::text NOT NULL
);


--
-- Name: secrets_vault_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.secrets_vault_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: secrets_vault_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.secrets_vault_id_seq OWNED BY public.secrets_vault.id;


--
-- Name: selected_candidates; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.selected_candidates (
    id bigint NOT NULL,
    setup_id uuid NOT NULL,
    exchange text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    direction text NOT NULL,
    entry_price numeric NOT NULL,
    sl_price numeric NOT NULL,
    tp_ladder jsonb DEFAULT '[]'::jsonb NOT NULL,
    risk_pct numeric NOT NULL,
    mode text NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    reject_reason text,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    selector_score numeric,
    selector_meta jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    claimed_at timestamp with time zone,
    placed_at timestamp with time zone,
    CONSTRAINT selected_candidates_direction_check CHECK ((direction = ANY (ARRAY['long'::text, 'short'::text]))),
    CONSTRAINT selected_candidates_mode_check CHECK ((mode = ANY (ARRAY['dry'::text, 'live'::text, 'backtest'::text]))),
    CONSTRAINT selected_candidates_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'claimed'::text, 'placed'::text, 'rejected'::text, 'errored'::text])))
);


--
-- Name: selected_candidates_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.selected_candidates_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: selected_candidates_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.selected_candidates_id_seq OWNED BY public.selected_candidates.id;


--
-- Name: setup_broadcast_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.setup_broadcast_outbox (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    setup_id uuid NOT NULL,
    status text DEFAULT 'pending'::text NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    telegram_sent_at timestamp with time zone,
    x_enqueued_at timestamp with time zone,
    claimed_at timestamp with time zone,
    sent_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT setup_broadcast_outbox_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'claimed'::text, 'sent'::text, 'failed'::text])))
);


--
-- Name: symbol_category_map; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.symbol_category_map (
    exchange text NOT NULL,
    symbol text NOT NULL,
    category_id smallint NOT NULL,
    source text DEFAULT 'auto'::text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: system_config; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.system_config (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    module text NOT NULL,
    config_key text NOT NULL,
    value jsonb DEFAULT '{}'::jsonb NOT NULL,
    schema_version integer DEFAULT 1 NOT NULL,
    description text,
    is_secret boolean DEFAULT false NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_by_user_id uuid
);


--
-- Name: system_config_audit; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.system_config_audit (
    id bigint NOT NULL,
    module text NOT NULL,
    config_key text NOT NULL,
    action text NOT NULL,
    old_value jsonb,
    new_value jsonb,
    changed_by uuid,
    changed_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT system_config_audit_action_check CHECK ((action = ANY (ARRAY['create'::text, 'update'::text, 'delete'::text, 'rollback'::text])))
);


--
-- Name: system_config_audit_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.system_config_audit_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: system_config_audit_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.system_config_audit_id_seq OWNED BY public.system_config_audit.id;


--
-- Name: user_permissions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_permissions (
    user_id uuid NOT NULL,
    permission text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: user_roles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_roles (
    user_id uuid NOT NULL,
    role_id uuid NOT NULL
);


--
-- Name: users; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.users (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    org_id uuid NOT NULL,
    email text NOT NULL,
    password_hash text NOT NULL,
    display_name text,
    is_admin boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    preferred_locale text,
    tz_offset_minutes integer DEFAULT 0 NOT NULL,
    tz_label text
);


--
-- Name: wave_chain; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.wave_chain (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    parent_id uuid,
    exchange text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    degree text NOT NULL,
    kind text NOT NULL,
    direction text NOT NULL,
    wave_number text,
    bar_start bigint NOT NULL,
    bar_end bigint NOT NULL,
    price_start numeric NOT NULL,
    price_end numeric NOT NULL,
    structural_score real DEFAULT 0 NOT NULL,
    state text DEFAULT 'forming'::text NOT NULL,
    detection_id uuid,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    time_start timestamp with time zone,
    time_end timestamp with time zone,
    subkind text DEFAULT ''::text NOT NULL
);


--
-- Name: wave_projections; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.wave_projections (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    source_wave_id uuid NOT NULL,
    alt_group uuid NOT NULL,
    exchange text NOT NULL,
    symbol text NOT NULL,
    timeframe text NOT NULL,
    degree text NOT NULL,
    projected_kind text NOT NULL,
    projected_label text NOT NULL,
    direction text NOT NULL,
    fib_basis text,
    projected_legs jsonb DEFAULT '[]'::jsonb NOT NULL,
    probability real DEFAULT 0.5 NOT NULL,
    rank integer DEFAULT 1 NOT NULL,
    state text DEFAULT 'active'::text NOT NULL,
    elimination_reason text,
    bars_validated integer DEFAULT 0 NOT NULL,
    last_validated_at timestamp with time zone,
    confirmed_detection_id uuid,
    time_start_est timestamp with time zone,
    time_end_est timestamp with time zone,
    price_target_min numeric,
    price_target_max numeric,
    invalidation_price numeric,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: x_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.x_outbox (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    setup_id uuid,
    lifecycle_event_id uuid,
    event_key text NOT NULL,
    body text NOT NULL,
    image_path text,
    status text DEFAULT 'pending'::text NOT NULL,
    attempt_count smallint DEFAULT 0 NOT NULL,
    last_error text,
    tweet_id text,
    permalink text,
    sent_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT x_outbox_status_chk CHECK ((status = ANY (ARRAY['pending'::text, 'sending'::text, 'sent'::text, 'failed'::text, 'skipped'::text])))
);


--
-- Name: config_audit id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_audit ALTER COLUMN id SET DEFAULT nextval('public.config_audit_id_seq'::regclass);


--
-- Name: config_scope id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_scope ALTER COLUMN id SET DEFAULT nextval('public.config_scope_id_seq'::regclass);


--
-- Name: config_value id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_value ALTER COLUMN id SET DEFAULT nextval('public.config_value_id_seq'::regclass);


--
-- Name: job_runs id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.job_runs ALTER COLUMN id SET DEFAULT nextval('public.job_runs_id_seq'::regclass);


--
-- Name: qtss_audit_log id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_audit_log ALTER COLUMN id SET DEFAULT nextval('public.qtss_audit_log_id_seq'::regclass);


--
-- Name: qtss_roles id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_roles ALTER COLUMN id SET DEFAULT nextval('public.qtss_roles_id_seq'::regclass);


--
-- Name: qtss_users id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_users ALTER COLUMN id SET DEFAULT nextval('public.qtss_users_id_seq'::regclass);


--
-- Name: reconcile_reports id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reconcile_reports ALTER COLUMN id SET DEFAULT nextval('public.reconcile_reports_id_seq'::regclass);


--
-- Name: scheduled_jobs id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.scheduled_jobs ALTER COLUMN id SET DEFAULT nextval('public.scheduled_jobs_id_seq'::regclass);


--
-- Name: secrets_vault id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.secrets_vault ALTER COLUMN id SET DEFAULT nextval('public.secrets_vault_id_seq'::regclass);


--
-- Name: selected_candidates id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.selected_candidates ALTER COLUMN id SET DEFAULT nextval('public.selected_candidates_id_seq'::regclass);


--
-- Name: system_config_audit id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_config_audit ALTER COLUMN id SET DEFAULT nextval('public.system_config_audit_id_seq'::regclass);


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: ai_approval_requests ai_approval_requests_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_approval_requests
    ADD CONSTRAINT ai_approval_requests_pkey PRIMARY KEY (id);


--
-- Name: ai_decision_outcomes ai_decision_outcomes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_decision_outcomes
    ADD CONSTRAINT ai_decision_outcomes_pkey PRIMARY KEY (id);


--
-- Name: ai_decisions ai_decisions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_decisions
    ADD CONSTRAINT ai_decisions_pkey PRIMARY KEY (id);


--
-- Name: ai_portfolio_directives ai_portfolio_directives_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_portfolio_directives
    ADD CONSTRAINT ai_portfolio_directives_pkey PRIMARY KEY (id);


--
-- Name: ai_position_directives ai_position_directives_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_position_directives
    ADD CONSTRAINT ai_position_directives_pkey PRIMARY KEY (id);


--
-- Name: ai_tactical_decisions ai_tactical_decisions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_tactical_decisions
    ADD CONSTRAINT ai_tactical_decisions_pkey PRIMARY KEY (id);


--
-- Name: analysis_snapshots analysis_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.analysis_snapshots
    ADD CONSTRAINT analysis_snapshots_pkey PRIMARY KEY (id);


--
-- Name: analysis_snapshots analysis_snapshots_unique_target_kind; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.analysis_snapshots
    ADD CONSTRAINT analysis_snapshots_unique_target_kind UNIQUE (engine_symbol_id, engine_kind);


--
-- Name: app_config app_config_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.app_config
    ADD CONSTRAINT app_config_key_key UNIQUE (key);


--
-- Name: app_config app_config_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.app_config
    ADD CONSTRAINT app_config_pkey PRIMARY KEY (id);


--
-- Name: asset_categories asset_categories_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.asset_categories
    ADD CONSTRAINT asset_categories_code_key UNIQUE (code);


--
-- Name: asset_categories asset_categories_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.asset_categories
    ADD CONSTRAINT asset_categories_pkey PRIMARY KEY (id);


--
-- Name: audit_log audit_log_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_log
    ADD CONSTRAINT audit_log_pkey PRIMARY KEY (id);


--
-- Name: qtss_audit_log audit_log_row_hash_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_audit_log
    ADD CONSTRAINT audit_log_row_hash_unique UNIQUE (row_hash);


--
-- Name: backfill_progress backfill_progress_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.backfill_progress
    ADD CONSTRAINT backfill_progress_pkey PRIMARY KEY (id);


--
-- Name: backfill_progress backfill_progress_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.backfill_progress
    ADD CONSTRAINT backfill_progress_unique UNIQUE (engine_symbol_id);


--
-- Name: bar_intervals bar_intervals_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.bar_intervals
    ADD CONSTRAINT bar_intervals_code_key UNIQUE (code);


--
-- Name: bar_intervals bar_intervals_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.bar_intervals
    ADD CONSTRAINT bar_intervals_pkey PRIMARY KEY (id);


--
-- Name: config_audit config_audit_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_audit
    ADD CONSTRAINT config_audit_pkey PRIMARY KEY (id);


--
-- Name: config_schema config_schema_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_schema
    ADD CONSTRAINT config_schema_pkey PRIMARY KEY (key);


--
-- Name: config_scope config_scope_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_scope
    ADD CONSTRAINT config_scope_pkey PRIMARY KEY (id);


--
-- Name: config_scope config_scope_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_scope
    ADD CONSTRAINT config_scope_unique UNIQUE (scope_type, scope_key);


--
-- Name: config_value config_value_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_value
    ADD CONSTRAINT config_value_pkey PRIMARY KEY (id);


--
-- Name: config_value config_value_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_value
    ADD CONSTRAINT config_value_unique UNIQUE (key, scope_id);


--
-- Name: confluence_snapshots confluence_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.confluence_snapshots
    ADD CONSTRAINT confluence_snapshots_pkey PRIMARY KEY (exchange, segment, symbol, timeframe, computed_at);


--
-- Name: copy_subscriptions copy_subscriptions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_subscriptions
    ADD CONSTRAINT copy_subscriptions_pkey PRIMARY KEY (id);


--
-- Name: copy_trade_execution_jobs copy_trade_execution_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_trade_execution_jobs
    ADD CONSTRAINT copy_trade_execution_jobs_pkey PRIMARY KEY (id);


--
-- Name: copy_trade_execution_jobs copy_trade_execution_jobs_sub_leader; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_trade_execution_jobs
    ADD CONSTRAINT copy_trade_execution_jobs_sub_leader UNIQUE (subscription_id, leader_exchange_order_id);


--
-- Name: data_snapshots data_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_snapshots
    ADD CONSTRAINT data_snapshots_pkey PRIMARY KEY (source_key);


--
-- Name: detections detections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.detections
    ADD CONSTRAINT detections_pkey PRIMARY KEY (id);


--
-- Name: detections detections_unique_span; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.detections
    ADD CONSTRAINT detections_unique_span UNIQUE (exchange, segment, symbol, timeframe, slot, pattern_family, subkind, start_time, end_time, mode);


--
-- Name: engine_symbol_ingestion_state engine_symbol_ingestion_state_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbol_ingestion_state
    ADD CONSTRAINT engine_symbol_ingestion_state_pkey PRIMARY KEY (engine_symbol_id);


--
-- Name: engine_symbols engine_symbols_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbols
    ADD CONSTRAINT engine_symbols_pkey PRIMARY KEY (id);


--
-- Name: engine_symbols engine_symbols_unique_series; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbols
    ADD CONSTRAINT engine_symbols_unique_series UNIQUE (exchange, segment, symbol, "interval");


--
-- Name: exchange_accounts exchange_accounts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_accounts
    ADD CONSTRAINT exchange_accounts_pkey PRIMARY KEY (id);


--
-- Name: exchange_fills exchange_fills_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_fills
    ADD CONSTRAINT exchange_fills_pkey PRIMARY KEY (id);


--
-- Name: exchange_fills exchange_fills_unique_trade; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_fills
    ADD CONSTRAINT exchange_fills_unique_trade UNIQUE (exchange, segment, user_id, venue_order_id, venue_trade_id);


--
-- Name: exchange_orders exchange_orders_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_orders
    ADD CONSTRAINT exchange_orders_pkey PRIMARY KEY (id);


--
-- Name: exchange_orders exchange_orders_user_client; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_orders
    ADD CONSTRAINT exchange_orders_user_client UNIQUE (user_id, client_order_id);


--
-- Name: exchanges exchanges_code_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchanges
    ADD CONSTRAINT exchanges_code_key UNIQUE (code);


--
-- Name: exchanges exchanges_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchanges
    ADD CONSTRAINT exchanges_pkey PRIMARY KEY (id);


--
-- Name: external_data_sources external_data_sources_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.external_data_sources
    ADD CONSTRAINT external_data_sources_pkey PRIMARY KEY (key);


--
-- Name: indicator_snapshots indicator_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.indicator_snapshots
    ADD CONSTRAINT indicator_snapshots_pkey PRIMARY KEY (exchange, segment, symbol, timeframe, bar_time, indicator);


--
-- Name: instruments instruments_market_id_native_symbol_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.instruments
    ADD CONSTRAINT instruments_market_id_native_symbol_key UNIQUE (market_id, native_symbol);


--
-- Name: instruments instruments_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.instruments
    ADD CONSTRAINT instruments_pkey PRIMARY KEY (id);


--
-- Name: intake_playbook_candidates intake_playbook_candidates_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.intake_playbook_candidates
    ADD CONSTRAINT intake_playbook_candidates_pkey PRIMARY KEY (id);


--
-- Name: intake_playbook_runs intake_playbook_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.intake_playbook_runs
    ADD CONSTRAINT intake_playbook_runs_pkey PRIMARY KEY (id);


--
-- Name: job_runs job_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.job_runs
    ADD CONSTRAINT job_runs_pkey PRIMARY KEY (id);


--
-- Name: liquidation_guard_events liquidation_guard_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.liquidation_guard_events
    ADD CONSTRAINT liquidation_guard_events_pkey PRIMARY KEY (id);


--
-- Name: live_positions live_positions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.live_positions
    ADD CONSTRAINT live_positions_pkey PRIMARY KEY (id);


--
-- Name: market_bars_open market_bars_open_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.market_bars_open
    ADD CONSTRAINT market_bars_open_pkey PRIMARY KEY (exchange, segment, symbol, "interval");


--
-- Name: market_bars market_bars_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.market_bars
    ADD CONSTRAINT market_bars_pkey PRIMARY KEY (id, open_time);


--
-- Name: market_bars market_bars_unique_bar; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.market_bars
    ADD CONSTRAINT market_bars_unique_bar UNIQUE (exchange, segment, symbol, "interval", open_time);


--
-- Name: markets markets_exchange_id_segment_contract_kind_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.markets
    ADD CONSTRAINT markets_exchange_id_segment_contract_kind_key UNIQUE (exchange_id, segment, contract_kind);


--
-- Name: markets markets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.markets
    ADD CONSTRAINT markets_pkey PRIMARY KEY (id);


--
-- Name: nansen_enriched_signals nansen_enriched_signals_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.nansen_enriched_signals
    ADD CONSTRAINT nansen_enriched_signals_pkey PRIMARY KEY (id);


--
-- Name: nansen_raw_flows nansen_raw_flows_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.nansen_raw_flows
    ADD CONSTRAINT nansen_raw_flows_pkey PRIMARY KEY (id);


--
-- Name: nansen_setup_rows nansen_setup_rows_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.nansen_setup_rows
    ADD CONSTRAINT nansen_setup_rows_pkey PRIMARY KEY (id);


--
-- Name: nansen_setup_runs nansen_setup_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.nansen_setup_runs
    ADD CONSTRAINT nansen_setup_runs_pkey PRIMARY KEY (id);


--
-- Name: nansen_snapshots nansen_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.nansen_snapshots
    ADD CONSTRAINT nansen_snapshots_pkey PRIMARY KEY (snapshot_kind);


--
-- Name: notify_delivery_prefs notify_delivery_prefs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notify_delivery_prefs
    ADD CONSTRAINT notify_delivery_prefs_pkey PRIMARY KEY (user_id);


--
-- Name: notify_outbox notify_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notify_outbox
    ADD CONSTRAINT notify_outbox_pkey PRIMARY KEY (id);


--
-- Name: oauth_clients oauth_clients_client_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_clients
    ADD CONSTRAINT oauth_clients_client_id_key UNIQUE (client_id);


--
-- Name: oauth_clients oauth_clients_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_clients
    ADD CONSTRAINT oauth_clients_pkey PRIMARY KEY (id);


--
-- Name: onchain_signal_scores onchain_signal_scores_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.onchain_signal_scores
    ADD CONSTRAINT onchain_signal_scores_pkey PRIMARY KEY (id);


--
-- Name: organizations organizations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organizations
    ADD CONSTRAINT organizations_pkey PRIMARY KEY (id);


--
-- Name: paper_balances paper_balances_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.paper_balances
    ADD CONSTRAINT paper_balances_pkey PRIMARY KEY (user_id, strategy_key);


--
-- Name: paper_fills paper_fills_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.paper_fills
    ADD CONSTRAINT paper_fills_pkey PRIMARY KEY (id);


--
-- Name: pattern_outcomes pattern_outcomes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pattern_outcomes
    ADD CONSTRAINT pattern_outcomes_pkey PRIMARY KEY (exchange, segment, symbol, timeframe, slot, pattern_family, subkind, start_time, mode);


--
-- Name: pivots pivots_engine_symbol_id_level_open_time_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pivots
    ADD CONSTRAINT pivots_engine_symbol_id_level_open_time_key UNIQUE (engine_symbol_id, level, open_time);


--
-- Name: pivots pivots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pivots
    ADD CONSTRAINT pivots_pkey PRIMARY KEY (id);


--
-- Name: pnl_rollups pnl_rollups_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pnl_rollups
    ADD CONSTRAINT pnl_rollups_pkey PRIMARY KEY (org_id, exchange, segment, symbol, ledger, bucket, period_start);


--
-- Name: position_scale_events position_scale_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.position_scale_events
    ADD CONSTRAINT position_scale_events_pkey PRIMARY KEY (id);


--
-- Name: q_radar_portfolio q_radar_portfolio_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.q_radar_portfolio
    ADD CONSTRAINT q_radar_portfolio_pkey PRIMARY KEY (id);


--
-- Name: q_radar_position_events q_radar_position_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.q_radar_position_events
    ADD CONSTRAINT q_radar_position_events_pkey PRIMARY KEY (id);


--
-- Name: qtss_audit_log qtss_audit_log_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_audit_log
    ADD CONSTRAINT qtss_audit_log_pkey PRIMARY KEY (id);


--
-- Name: qtss_market_regime_daily qtss_market_regime_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_market_regime_daily
    ADD CONSTRAINT qtss_market_regime_daily_pkey PRIMARY KEY (day, exchange, sector);


--
-- Name: qtss_models qtss_models_family_version_uq; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_models
    ADD CONSTRAINT qtss_models_family_version_uq UNIQUE (model_family, model_version);


--
-- Name: qtss_models qtss_models_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_models
    ADD CONSTRAINT qtss_models_pkey PRIMARY KEY (id);


--
-- Name: qtss_position_health_snapshots qtss_position_health_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_position_health_snapshots
    ADD CONSTRAINT qtss_position_health_snapshots_pkey PRIMARY KEY (id);


--
-- Name: qtss_positions qtss_positions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_positions
    ADD CONSTRAINT qtss_positions_pkey PRIMARY KEY (id);


--
-- Name: qtss_positions qtss_positions_setup_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_positions
    ADD CONSTRAINT qtss_positions_setup_id_key UNIQUE (setup_id);


--
-- Name: qtss_reports_runs qtss_reports_runs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_reports_runs
    ADD CONSTRAINT qtss_reports_runs_pkey PRIMARY KEY (id);


--
-- Name: qtss_roles qtss_roles_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_roles
    ADD CONSTRAINT qtss_roles_name_key UNIQUE (name);


--
-- Name: qtss_roles qtss_roles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_roles
    ADD CONSTRAINT qtss_roles_pkey PRIMARY KEY (id);


--
-- Name: qtss_sessions qtss_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_sessions
    ADD CONSTRAINT qtss_sessions_pkey PRIMARY KEY (id);


--
-- Name: qtss_setup_lifecycle_events qtss_setup_lifecycle_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_setup_lifecycle_events
    ADD CONSTRAINT qtss_setup_lifecycle_events_pkey PRIMARY KEY (id);


--
-- Name: qtss_setup_outcomes qtss_setup_outcomes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_setup_outcomes
    ADD CONSTRAINT qtss_setup_outcomes_pkey PRIMARY KEY (setup_id);


--
-- Name: qtss_setups qtss_setups_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_setups
    ADD CONSTRAINT qtss_setups_pkey PRIMARY KEY (id);


--
-- Name: qtss_symbol_profile qtss_symbol_profile_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_symbol_profile
    ADD CONSTRAINT qtss_symbol_profile_pkey PRIMARY KEY (id);


--
-- Name: qtss_symbol_profile qtss_symbol_profile_uniq; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_symbol_profile
    ADD CONSTRAINT qtss_symbol_profile_uniq UNIQUE (exchange, symbol);


--
-- Name: qtss_user_roles qtss_user_roles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_user_roles
    ADD CONSTRAINT qtss_user_roles_pkey PRIMARY KEY (user_id, role_id);


--
-- Name: qtss_users qtss_users_email_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_users
    ADD CONSTRAINT qtss_users_email_key UNIQUE (email);


--
-- Name: qtss_users qtss_users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_users
    ADD CONSTRAINT qtss_users_pkey PRIMARY KEY (id);


--
-- Name: qtss_users qtss_users_username_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_users
    ADD CONSTRAINT qtss_users_username_key UNIQUE (username);


--
-- Name: qtss_v2_setup_events qtss_v2_setup_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_v2_setup_events
    ADD CONSTRAINT qtss_v2_setup_events_pkey PRIMARY KEY (id);


--
-- Name: qtss_v2_setup_events qtss_v2_setup_events_setup_id_event_type_created_at_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_v2_setup_events
    ADD CONSTRAINT qtss_v2_setup_events_setup_id_event_type_created_at_key UNIQUE (setup_id, event_type, created_at);


--
-- Name: qtss_v2_setup_rejections qtss_v2_setup_rejections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_v2_setup_rejections
    ADD CONSTRAINT qtss_v2_setup_rejections_pkey PRIMARY KEY (id);


--
-- Name: range_signal_events range_signal_events_dedupe; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.range_signal_events
    ADD CONSTRAINT range_signal_events_dedupe UNIQUE (engine_symbol_id, event_kind, bar_open_time);


--
-- Name: range_signal_events range_signal_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.range_signal_events
    ADD CONSTRAINT range_signal_events_pkey PRIMARY KEY (id);


--
-- Name: range_signal_paper_executions range_signal_paper_executions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.range_signal_paper_executions
    ADD CONSTRAINT range_signal_paper_executions_pkey PRIMARY KEY (range_signal_event_id);


--
-- Name: reconcile_reports reconcile_reports_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reconcile_reports
    ADD CONSTRAINT reconcile_reports_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_pkey PRIMARY KEY (id);


--
-- Name: refresh_tokens refresh_tokens_token_hash_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_token_hash_key UNIQUE (token_hash);


--
-- Name: regime_param_overrides regime_param_overrides_module_config_key_regime_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.regime_param_overrides
    ADD CONSTRAINT regime_param_overrides_module_config_key_regime_key UNIQUE (module, config_key, regime);


--
-- Name: regime_param_overrides regime_param_overrides_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.regime_param_overrides
    ADD CONSTRAINT regime_param_overrides_pkey PRIMARY KEY (id);


--
-- Name: regime_snapshots regime_snapshots_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.regime_snapshots
    ADD CONSTRAINT regime_snapshots_pkey PRIMARY KEY (id);


--
-- Name: regime_transitions regime_transitions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.regime_transitions
    ADD CONSTRAINT regime_transitions_pkey PRIMARY KEY (id);


--
-- Name: roles roles_key_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.roles
    ADD CONSTRAINT roles_key_key UNIQUE (key);


--
-- Name: roles roles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.roles
    ADD CONSTRAINT roles_pkey PRIMARY KEY (id);


--
-- Name: scheduled_jobs scheduled_jobs_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.scheduled_jobs
    ADD CONSTRAINT scheduled_jobs_name_key UNIQUE (name);


--
-- Name: scheduled_jobs scheduled_jobs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.scheduled_jobs
    ADD CONSTRAINT scheduled_jobs_pkey PRIMARY KEY (id);


--
-- Name: secrets_vault secrets_vault_name_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.secrets_vault
    ADD CONSTRAINT secrets_vault_name_key UNIQUE (name);


--
-- Name: secrets_vault secrets_vault_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.secrets_vault
    ADD CONSTRAINT secrets_vault_pkey PRIMARY KEY (id);


--
-- Name: selected_candidates selected_candidates_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.selected_candidates
    ADD CONSTRAINT selected_candidates_pkey PRIMARY KEY (id);


--
-- Name: selected_candidates selected_candidates_setup_id_mode_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.selected_candidates
    ADD CONSTRAINT selected_candidates_setup_id_mode_key UNIQUE (setup_id, mode);


--
-- Name: setup_broadcast_outbox setup_broadcast_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.setup_broadcast_outbox
    ADD CONSTRAINT setup_broadcast_outbox_pkey PRIMARY KEY (id);


--
-- Name: setup_broadcast_outbox setup_broadcast_outbox_setup_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.setup_broadcast_outbox
    ADD CONSTRAINT setup_broadcast_outbox_setup_id_key UNIQUE (setup_id);


--
-- Name: symbol_category_map symbol_category_map_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.symbol_category_map
    ADD CONSTRAINT symbol_category_map_pkey PRIMARY KEY (exchange, symbol);


--
-- Name: system_config_audit system_config_audit_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_config_audit
    ADD CONSTRAINT system_config_audit_pkey PRIMARY KEY (id);


--
-- Name: system_config system_config_module_key_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_config
    ADD CONSTRAINT system_config_module_key_unique UNIQUE (module, config_key);


--
-- Name: system_config system_config_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_config
    ADD CONSTRAINT system_config_pkey PRIMARY KEY (id);


--
-- Name: user_permissions user_permissions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_permissions
    ADD CONSTRAINT user_permissions_pkey PRIMARY KEY (user_id, permission);


--
-- Name: user_roles user_roles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_pkey PRIMARY KEY (user_id, role_id);


--
-- Name: users users_email_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_email_key UNIQUE (email);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: wave_chain wave_chain_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wave_chain
    ADD CONSTRAINT wave_chain_pkey PRIMARY KEY (id);


--
-- Name: wave_projections wave_projections_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wave_projections
    ADD CONSTRAINT wave_projections_pkey PRIMARY KEY (id);


--
-- Name: x_outbox x_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.x_outbox
    ADD CONSTRAINT x_outbox_pkey PRIMARY KEY (id);


--
-- Name: audit_log_action_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_log_action_idx ON public.qtss_audit_log USING btree (action);


--
-- Name: audit_log_actor_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_log_actor_idx ON public.qtss_audit_log USING btree (actor);


--
-- Name: audit_log_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_log_at_idx ON public.qtss_audit_log USING btree (at);


--
-- Name: audit_log_corr_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_log_corr_idx ON public.qtss_audit_log USING btree (correlation_id);


--
-- Name: audit_log_subject_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_log_subject_idx ON public.qtss_audit_log USING btree (subject);


--
-- Name: confluence_snapshots_recent_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX confluence_snapshots_recent_idx ON public.confluence_snapshots USING btree (exchange, segment, symbol, timeframe, computed_at DESC);


--
-- Name: detections_family_subkind_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX detections_family_subkind_idx ON public.detections USING btree (pattern_family, subkind);


--
-- Name: detections_mode_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX detections_mode_idx ON public.detections USING btree (mode) WHERE (mode <> 'live'::text);


--
-- Name: detections_series_time_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX detections_series_time_idx ON public.detections USING btree (exchange, segment, symbol, timeframe, detected_at DESC);


--
-- Name: detections_slot_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX detections_slot_idx ON public.detections USING btree (slot);


--
-- Name: idx_ai_approval_org_status_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_approval_org_status_created ON public.ai_approval_requests USING btree (org_id, status, created_at DESC);


--
-- Name: idx_ai_approval_rejection_reason; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_approval_rejection_reason ON public.ai_approval_requests USING btree (rejection_reason) WHERE (rejection_reason IS NOT NULL);


--
-- Name: idx_ai_decision_outcomes_decision_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_decision_outcomes_decision_id ON public.ai_decision_outcomes USING btree (decision_id);


--
-- Name: idx_ai_decision_outcomes_recorded; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_decision_outcomes_recorded ON public.ai_decision_outcomes USING btree (recorded_at DESC);


--
-- Name: idx_ai_decisions_approval_request; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_decisions_approval_request ON public.ai_decisions USING btree (approval_request_id) WHERE (approval_request_id IS NOT NULL);


--
-- Name: idx_ai_decisions_status_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_decisions_status_pending ON public.ai_decisions USING btree (status) WHERE (status = ANY (ARRAY['pending_approval'::text, 'approved'::text]));


--
-- Name: idx_ai_decisions_symbol_layer_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_decisions_symbol_layer_created ON public.ai_decisions USING btree (symbol, layer, created_at DESC);


--
-- Name: idx_ai_portfolio_directives_decision_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_portfolio_directives_decision_id ON public.ai_portfolio_directives USING btree (decision_id);


--
-- Name: idx_ai_portfolio_directives_status_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_portfolio_directives_status_created ON public.ai_portfolio_directives USING btree (status, created_at DESC);


--
-- Name: idx_ai_position_directives_decision_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_position_directives_decision_id ON public.ai_position_directives USING btree (decision_id);


--
-- Name: idx_ai_position_directives_symbol_status_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_position_directives_symbol_status_created ON public.ai_position_directives USING btree (symbol, status, created_at DESC);


--
-- Name: idx_ai_tactical_decision_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_tactical_decision_id ON public.ai_tactical_decisions USING btree (decision_id);


--
-- Name: idx_ai_tactical_symbol_status_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ai_tactical_symbol_status_created ON public.ai_tactical_decisions USING btree (symbol, status, created_at DESC);


--
-- Name: idx_analysis_snapshots_computed; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_analysis_snapshots_computed ON public.analysis_snapshots USING btree (computed_at DESC);


--
-- Name: idx_audit_log_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_log_created ON public.audit_log USING btree (created_at DESC);


--
-- Name: idx_audit_log_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_audit_log_user ON public.audit_log USING btree (user_id, created_at DESC);


--
-- Name: idx_backfill_progress_state; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_backfill_progress_state ON public.backfill_progress USING btree (state);


--
-- Name: idx_bar_intervals_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_bar_intervals_active ON public.bar_intervals USING btree (is_active, sort_order);


--
-- Name: idx_config_audit_corr; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_config_audit_corr ON public.config_audit USING btree (correlation);


--
-- Name: idx_config_audit_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_config_audit_key ON public.config_audit USING btree (key, changed_at DESC);


--
-- Name: idx_config_schema_category; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_config_schema_category ON public.config_schema USING btree (category, subcategory);


--
-- Name: idx_config_value_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_config_value_key ON public.config_value USING btree (key);


--
-- Name: idx_config_value_scope; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_config_value_scope ON public.config_value USING btree (scope_id);


--
-- Name: idx_config_value_validity; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_config_value_validity ON public.config_value USING btree (valid_from, valid_until) WHERE ((valid_from IS NOT NULL) OR (valid_until IS NOT NULL));


--
-- Name: idx_copy_follower; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_copy_follower ON public.copy_subscriptions USING btree (follower_user_id);


--
-- Name: idx_copy_leader; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_copy_leader ON public.copy_subscriptions USING btree (leader_user_id);


--
-- Name: idx_copy_trade_jobs_pending_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_copy_trade_jobs_pending_created ON public.copy_trade_execution_jobs USING btree (created_at) WHERE (status = 'pending'::text);


--
-- Name: idx_data_snapshots_computed_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_data_snapshots_computed_at ON public.data_snapshots USING btree (computed_at DESC);


--
-- Name: idx_engine_symbols_bar_interval_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_bar_interval_id ON public.engine_symbols USING btree (bar_interval_id);


--
-- Name: idx_engine_symbols_discovery_ttl; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_discovery_ttl ON public.engine_symbols USING btree (last_signal_at) WHERE ((source = 'onchain_discovery'::text) AND (pinned = false));


--
-- Name: idx_engine_symbols_enabled; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_enabled ON public.engine_symbols USING btree (enabled, sort_order, exchange, segment, symbol);


--
-- Name: idx_engine_symbols_exchange_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_exchange_id ON public.engine_symbols USING btree (exchange_id);


--
-- Name: idx_engine_symbols_instrument_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_instrument_id ON public.engine_symbols USING btree (instrument_id);


--
-- Name: idx_engine_symbols_lifecycle; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_lifecycle ON public.engine_symbols USING btree (lifecycle_state) WHERE (lifecycle_state <> ALL (ARRAY['retired'::text, 'manual'::text]));


--
-- Name: idx_engine_symbols_market_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_market_id ON public.engine_symbols USING btree (market_id);


--
-- Name: idx_engine_symbols_source; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_engine_symbols_source ON public.engine_symbols USING btree (source);


--
-- Name: idx_exchange_accounts_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_exchange_accounts_user ON public.exchange_accounts USING btree (user_id);


--
-- Name: idx_exchange_fills_order; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_exchange_fills_order ON public.exchange_fills USING btree (exchange, segment, user_id, venue_order_id);


--
-- Name: idx_exchange_fills_user_time; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_exchange_fills_user_time ON public.exchange_fills USING btree (user_id, event_time DESC);


--
-- Name: idx_exchange_orders_org_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_exchange_orders_org_created ON public.exchange_orders USING btree (org_id, created_at DESC);


--
-- Name: idx_exchange_orders_user_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_exchange_orders_user_created ON public.exchange_orders USING btree (user_id, created_at DESC);


--
-- Name: idx_instruments_base_quote; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_instruments_base_quote ON public.instruments USING btree (base_asset, quote_asset);


--
-- Name: idx_instruments_market; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_instruments_market ON public.instruments USING btree (market_id);


--
-- Name: idx_instruments_native_symbol; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_instruments_native_symbol ON public.instruments USING btree (native_symbol);


--
-- Name: idx_instruments_trading; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_instruments_trading ON public.instruments USING btree (market_id) WHERE (is_trading = true);


--
-- Name: idx_intake_playbook_candidates_run; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_intake_playbook_candidates_run ON public.intake_playbook_candidates USING btree (run_id, rank);


--
-- Name: idx_intake_playbook_runs_playbook_computed; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_intake_playbook_runs_playbook_computed ON public.intake_playbook_runs USING btree (playbook_id, computed_at DESC);


--
-- Name: idx_lifecycle_events_kind_time; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_lifecycle_events_kind_time ON public.qtss_setup_lifecycle_events USING btree (event_kind, emitted_at DESC);


--
-- Name: idx_lifecycle_events_setup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_lifecycle_events_setup ON public.qtss_setup_lifecycle_events USING btree (setup_id, emitted_at DESC);


--
-- Name: idx_liq_guard_position; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_liq_guard_position ON public.liquidation_guard_events USING btree (position_id, occurred_at DESC);


--
-- Name: idx_liq_guard_severity; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_liq_guard_severity ON public.liquidation_guard_events USING btree (severity, occurred_at DESC);


--
-- Name: idx_live_positions_open; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_live_positions_open ON public.live_positions USING btree (mode, exchange, symbol) WHERE (closed_at IS NULL);


--
-- Name: idx_live_positions_setup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_live_positions_setup ON public.live_positions USING btree (setup_id) WHERE (setup_id IS NOT NULL);


--
-- Name: idx_live_positions_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_live_positions_user ON public.live_positions USING btree (user_id, opened_at DESC);


--
-- Name: idx_market_bars_series; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_market_bars_series ON public.market_bars USING btree (exchange, segment, symbol, "interval", open_time DESC);


--
-- Name: idx_markets_exchange; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_markets_exchange ON public.markets USING btree (exchange_id);


--
-- Name: idx_nansen_setup_rows_run; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nansen_setup_rows_run ON public.nansen_setup_rows USING btree (run_id, rank);


--
-- Name: idx_nansen_setup_runs_computed; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nansen_setup_runs_computed ON public.nansen_setup_runs USING btree (computed_at DESC);


--
-- Name: idx_nansen_snapshots_computed; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nansen_snapshots_computed ON public.nansen_snapshots USING btree (computed_at DESC);


--
-- Name: idx_nes_symbol_type; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nes_symbol_type ON public.nansen_enriched_signals USING btree (symbol, signal_type, computed_at DESC);


--
-- Name: idx_notify_outbox_event_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_outbox_event_key ON public.notify_outbox USING btree (event_key);


--
-- Name: idx_notify_outbox_instrument; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_outbox_instrument ON public.notify_outbox USING btree (exchange, segment, symbol);


--
-- Name: idx_notify_outbox_org_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_outbox_org_created ON public.notify_outbox USING btree (org_id, created_at DESC);


--
-- Name: idx_notify_outbox_pending_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_outbox_pending_created ON public.notify_outbox USING btree (created_at) WHERE (status = 'pending'::text);


--
-- Name: idx_notify_prefs_digest; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_prefs_digest ON public.notify_delivery_prefs USING btree (notify_daily_digest) WHERE (notify_daily_digest = true);


--
-- Name: idx_notify_prefs_last_digest; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_prefs_last_digest ON public.notify_delivery_prefs USING btree (last_digest_sent_utc);


--
-- Name: idx_notify_prefs_telegram_enabled; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_prefs_telegram_enabled ON public.notify_delivery_prefs USING btree (telegram_enabled) WHERE (telegram_enabled = true);


--
-- Name: idx_notify_prefs_x_enabled; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_notify_prefs_x_enabled ON public.notify_delivery_prefs USING btree (x_enabled) WHERE (x_enabled = true);


--
-- Name: idx_nrf_chain_token; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nrf_chain_token ON public.nansen_raw_flows USING btree (chain, token_symbol, snapshot_at DESC);


--
-- Name: idx_nrf_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nrf_created ON public.nansen_raw_flows USING btree (created_at DESC);


--
-- Name: idx_nrf_symbol_type; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_nrf_symbol_type ON public.nansen_raw_flows USING btree (engine_symbol, source_type, snapshot_at DESC);


--
-- Name: idx_oauth_clients_org; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_oauth_clients_org ON public.oauth_clients USING btree (org_id);


--
-- Name: idx_ocs_symbol_time; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_ocs_symbol_time ON public.onchain_signal_scores USING btree (symbol, computed_at DESC);


--
-- Name: idx_paper_balances_org; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_paper_balances_org ON public.paper_balances USING btree (org_id);


--
-- Name: idx_paper_fills_strategy; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_paper_fills_strategy ON public.paper_fills USING btree (user_id, strategy_key, created_at DESC);


--
-- Name: idx_paper_fills_user_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_paper_fills_user_created ON public.paper_fills USING btree (user_id, created_at DESC);


--
-- Name: idx_pnl_ledger_bucket; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_pnl_ledger_bucket ON public.pnl_rollups USING btree (ledger, bucket, period_start DESC);


--
-- Name: idx_position_health_setup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_position_health_setup ON public.qtss_position_health_snapshots USING btree (setup_id, captured_at DESC);


--
-- Name: idx_q_radar_position_events_pos; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_q_radar_position_events_pos ON public.q_radar_position_events USING btree (position_id, created_at DESC);


--
-- Name: idx_qtss_models_family_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_models_family_active ON public.qtss_models USING btree (model_family, active, trained_at DESC);


--
-- Name: idx_qtss_positions_open; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_positions_open ON public.qtss_positions USING btree (state) WHERE (state = 'open'::text);


--
-- Name: idx_qtss_positions_setup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_positions_setup ON public.qtss_positions USING btree (setup_id);


--
-- Name: idx_qtss_setups_active_watcher; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_active_watcher ON public.qtss_setups USING btree (exchange, symbol, timeframe) WHERE (closed_at IS NULL);


--
-- Name: idx_qtss_setups_ai_score; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_ai_score ON public.qtss_setups USING btree (ai_score DESC NULLS LAST);


--
-- Name: idx_qtss_setups_detection; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_detection ON public.qtss_setups USING btree (detection_id) WHERE (detection_id IS NOT NULL);


--
-- Name: idx_qtss_setups_idempotency; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_qtss_setups_idempotency ON public.qtss_setups USING btree (idempotency_key) WHERE (idempotency_key IS NOT NULL);


--
-- Name: idx_qtss_setups_mode_state; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_mode_state ON public.qtss_setups USING btree (mode, state) WHERE (state = ANY (ARRAY['armed'::text, 'active'::text]));


--
-- Name: idx_qtss_setups_open; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_open ON public.qtss_setups USING btree (venue_class, profile, state) WHERE (state = ANY (ARRAY['armed'::text, 'active'::text]));


--
-- Name: idx_qtss_setups_symbol; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_symbol ON public.qtss_setups USING btree (exchange, symbol, timeframe, created_at DESC);


--
-- Name: idx_qtss_setups_tracker_cursor; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_tracker_cursor ON public.qtss_setups USING btree (last_tracked_bar_ts) WHERE (state = ANY (ARRAY['armed'::text, 'active'::text]));


--
-- Name: idx_qtss_setups_trail_mode; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_qtss_setups_trail_mode ON public.qtss_setups USING btree (trail_mode) WHERE (trail_mode = true);


--
-- Name: idx_range_signal_events_symbol_bar; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_range_signal_events_symbol_bar ON public.range_signal_events USING btree (engine_symbol_id, bar_open_time DESC);


--
-- Name: idx_range_signal_paper_executions_status_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_range_signal_paper_executions_status_created ON public.range_signal_paper_executions USING btree (status, created_at DESC);


--
-- Name: idx_reconcile_reports_severity; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_reconcile_reports_severity ON public.reconcile_reports USING btree (overall, created_at DESC) WHERE (overall <> 'none'::text);


--
-- Name: idx_reconcile_reports_user_venue; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_reconcile_reports_user_venue ON public.reconcile_reports USING btree (user_id, venue, created_at DESC);


--
-- Name: idx_refresh_expires; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_refresh_expires ON public.refresh_tokens USING btree (expires_at) WHERE (revoked_at IS NULL);


--
-- Name: idx_refresh_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_refresh_user ON public.refresh_tokens USING btree (user_id);


--
-- Name: idx_regime_snapshots_lookup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_regime_snapshots_lookup ON public.regime_snapshots USING btree (symbol, "interval", computed_at DESC);


--
-- Name: idx_regime_transitions_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_regime_transitions_active ON public.regime_transitions USING btree (symbol, "interval") WHERE (resolved_at IS NULL);


--
-- Name: idx_reports_runs_generated; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_reports_runs_generated ON public.qtss_reports_runs USING btree (generated_at DESC);


--
-- Name: idx_sc_audit_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_sc_audit_key ON public.system_config_audit USING btree (module, config_key, changed_at DESC);


--
-- Name: idx_scale_events_position; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_scale_events_position ON public.position_scale_events USING btree (position_id, occurred_at DESC);


--
-- Name: idx_selected_candidates_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_selected_candidates_pending ON public.selected_candidates USING btree (created_at) WHERE (status = 'pending'::text);


--
-- Name: idx_selected_candidates_setup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_selected_candidates_setup ON public.selected_candidates USING btree (setup_id);


--
-- Name: idx_setup_bcast_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_bcast_outbox_pending ON public.setup_broadcast_outbox USING btree (created_at) WHERE (status = 'pending'::text);


--
-- Name: idx_setup_bcast_outbox_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_bcast_outbox_status ON public.setup_broadcast_outbox USING btree (status, updated_at DESC);


--
-- Name: idx_setup_events_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_events_pending ON public.qtss_v2_setup_events USING btree (delivery_state, created_at) WHERE (delivery_state = 'pending'::text);


--
-- Name: idx_setup_outcomes_closed_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_outcomes_closed_at ON public.qtss_setup_outcomes USING btree (closed_at DESC);


--
-- Name: idx_setup_outcomes_label; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_outcomes_label ON public.qtss_setup_outcomes USING btree (label);


--
-- Name: idx_setup_outcomes_labeled_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_outcomes_labeled_at ON public.qtss_setup_outcomes USING btree (labeled_at DESC);


--
-- Name: idx_setup_rejections_recent; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_setup_rejections_recent ON public.qtss_v2_setup_rejections USING btree (created_at DESC);


--
-- Name: idx_symbol_category_map_cat; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_symbol_category_map_cat ON public.symbol_category_map USING btree (category_id);


--
-- Name: idx_system_config_module; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_system_config_module ON public.system_config USING btree (module);


--
-- Name: idx_system_config_module_config_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_system_config_module_config_key ON public.system_config USING btree (module, config_key);


--
-- Name: idx_user_permissions_user; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_user_permissions_user ON public.user_permissions USING btree (user_id);


--
-- Name: idx_wave_chain_detection; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wave_chain_detection ON public.wave_chain USING btree (detection_id) WHERE (detection_id IS NOT NULL);


--
-- Name: idx_wave_chain_parent; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wave_chain_parent ON public.wave_chain USING btree (parent_id) WHERE (parent_id IS NOT NULL);


--
-- Name: idx_wave_chain_series; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wave_chain_series ON public.wave_chain USING btree (exchange, symbol, timeframe, bar_start DESC);


--
-- Name: idx_wave_chain_time_range; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wave_chain_time_range ON public.wave_chain USING btree (exchange, symbol, timeframe, degree, time_start, time_end) WHERE (state <> 'invalidated'::text);


--
-- Name: idx_wp_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wp_active ON public.wave_projections USING btree (exchange, symbol, timeframe, state) WHERE (state = ANY (ARRAY['active'::text, 'leading'::text]));


--
-- Name: idx_wp_alt_group; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wp_alt_group ON public.wave_projections USING btree (alt_group);


--
-- Name: idx_wp_series; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wp_series ON public.wave_projections USING btree (exchange, symbol, timeframe, created_at DESC);


--
-- Name: idx_wp_source; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_wp_source ON public.wave_projections USING btree (source_wave_id);


--
-- Name: idx_x_outbox_sent_at; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_x_outbox_sent_at ON public.x_outbox USING btree (sent_at DESC) WHERE (status = 'sent'::text);


--
-- Name: idx_x_outbox_setup; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_x_outbox_setup ON public.x_outbox USING btree (setup_id);


--
-- Name: idx_x_outbox_status_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_x_outbox_status_created ON public.x_outbox USING btree (status, created_at) WHERE (status = ANY (ARRAY['pending'::text, 'sending'::text]));


--
-- Name: indicator_snapshots_computed_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX indicator_snapshots_computed_at_idx ON public.indicator_snapshots USING btree (computed_at);


--
-- Name: indicator_snapshots_recent_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX indicator_snapshots_recent_idx ON public.indicator_snapshots USING btree (exchange, segment, symbol, timeframe, indicator, bar_time DESC);


--
-- Name: ix_qtss_symbol_profile_category; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_qtss_symbol_profile_category ON public.qtss_symbol_profile USING btree (category);


--
-- Name: ix_qtss_symbol_profile_exchange; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_qtss_symbol_profile_exchange ON public.qtss_symbol_profile USING btree (exchange);


--
-- Name: ix_qtss_symbol_profile_tier; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_qtss_symbol_profile_tier ON public.qtss_symbol_profile USING btree (risk_tier);


--
-- Name: ix_qtss_symbol_profile_updated; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX ix_qtss_symbol_profile_updated ON public.qtss_symbol_profile USING btree (updated_at DESC);


--
-- Name: job_runs_job_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX job_runs_job_idx ON public.job_runs USING btree (job_id, started_at DESC);


--
-- Name: job_runs_status_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX job_runs_status_idx ON public.job_runs USING btree (status) WHERE (status = 'running'::text);


--
-- Name: market_bars_open_time_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX market_bars_open_time_idx ON public.market_bars USING btree (open_time DESC);


--
-- Name: pattern_outcomes_active_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX pattern_outcomes_active_idx ON public.pattern_outcomes USING btree (evaluated_at) WHERE (outcome = 'active'::text);


--
-- Name: pattern_outcomes_by_family; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX pattern_outcomes_by_family ON public.pattern_outcomes USING btree (pattern_family, subkind, timeframe, evaluated_at DESC);


--
-- Name: pattern_outcomes_by_symbol; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX pattern_outcomes_by_symbol ON public.pattern_outcomes USING btree (symbol, timeframe, evaluated_at DESC);


--
-- Name: pivots_symbol_level_bar_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX pivots_symbol_level_bar_idx ON public.pivots USING btree (engine_symbol_id, level, bar_index);


--
-- Name: pivots_symbol_level_time_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX pivots_symbol_level_time_idx ON public.pivots USING btree (engine_symbol_id, level, open_time DESC);


--
-- Name: qtss_models_one_active_per_family; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX qtss_models_one_active_per_family ON public.qtss_models USING btree (model_family) WHERE active;


--
-- Name: qtss_models_one_role_active; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX qtss_models_one_role_active ON public.qtss_models USING btree (model_family) WHERE (role = 'active'::text);


--
-- Name: scheduled_jobs_due_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX scheduled_jobs_due_idx ON public.scheduled_jobs USING btree (next_run_at) WHERE (enabled = true);


--
-- Name: secrets_vault_kek_version_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX secrets_vault_kek_version_idx ON public.secrets_vault USING btree (kek_version);


--
-- Name: sessions_expires_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX sessions_expires_idx ON public.qtss_sessions USING btree (expires_at);


--
-- Name: sessions_user_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX sessions_user_idx ON public.qtss_sessions USING btree (user_id);


--
-- Name: uq_open_setup_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX uq_open_setup_key ON public.qtss_setups USING btree (exchange, symbol, timeframe, profile, mode) WHERE (state = ANY (ARRAY['armed'::text, 'active'::text]));


--
-- Name: uq_reports_runs_kind_window; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX uq_reports_runs_kind_window ON public.qtss_reports_runs USING btree (kind, window_start);


--
-- Name: ux_qtss_setups_idempotency_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX ux_qtss_setups_idempotency_key ON public.qtss_setups USING btree (idempotency_key) WHERE (idempotency_key IS NOT NULL);


--
-- Name: qtss_audit_log audit_log_no_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON public.qtss_audit_log FOR EACH ROW EXECUTE FUNCTION public.audit_log_block_mutation();


--
-- Name: qtss_audit_log audit_log_no_update; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON public.qtss_audit_log FOR EACH ROW EXECUTE FUNCTION public.audit_log_block_mutation();


--
-- Name: config_value trg_config_value_notify; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_config_value_notify AFTER INSERT OR DELETE OR UPDATE ON public.config_value FOR EACH ROW EXECUTE FUNCTION public.notify_config_changed();


--
-- Name: qtss_models trg_qtss_models_sync_active_role; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_qtss_models_sync_active_role BEFORE INSERT OR UPDATE ON public.qtss_models FOR EACH ROW EXECUTE FUNCTION public.qtss_models_sync_active_role();


--
-- Name: system_config trg_system_config_audit; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_system_config_audit AFTER INSERT OR DELETE OR UPDATE ON public.system_config FOR EACH ROW EXECUTE FUNCTION public.fn_system_config_audit();


--
-- Name: ai_approval_requests ai_approval_requests_decided_by_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_approval_requests
    ADD CONSTRAINT ai_approval_requests_decided_by_user_id_fkey FOREIGN KEY (decided_by_user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: ai_approval_requests ai_approval_requests_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_approval_requests
    ADD CONSTRAINT ai_approval_requests_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: ai_approval_requests ai_approval_requests_requester_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_approval_requests
    ADD CONSTRAINT ai_approval_requests_requester_user_id_fkey FOREIGN KEY (requester_user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: ai_decision_outcomes ai_decision_outcomes_decision_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_decision_outcomes
    ADD CONSTRAINT ai_decision_outcomes_decision_id_fkey FOREIGN KEY (decision_id) REFERENCES public.ai_decisions(id) ON DELETE CASCADE;


--
-- Name: ai_decisions ai_decisions_approval_request_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_decisions
    ADD CONSTRAINT ai_decisions_approval_request_id_fkey FOREIGN KEY (approval_request_id) REFERENCES public.ai_approval_requests(id) ON DELETE SET NULL;


--
-- Name: ai_portfolio_directives ai_portfolio_directives_decision_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_portfolio_directives
    ADD CONSTRAINT ai_portfolio_directives_decision_id_fkey FOREIGN KEY (decision_id) REFERENCES public.ai_decisions(id) ON DELETE CASCADE;


--
-- Name: ai_position_directives ai_position_directives_decision_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_position_directives
    ADD CONSTRAINT ai_position_directives_decision_id_fkey FOREIGN KEY (decision_id) REFERENCES public.ai_decisions(id) ON DELETE CASCADE;


--
-- Name: ai_tactical_decisions ai_tactical_decisions_decision_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.ai_tactical_decisions
    ADD CONSTRAINT ai_tactical_decisions_decision_id_fkey FOREIGN KEY (decision_id) REFERENCES public.ai_decisions(id) ON DELETE CASCADE;


--
-- Name: analysis_snapshots analysis_snapshots_engine_symbol_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.analysis_snapshots
    ADD CONSTRAINT analysis_snapshots_engine_symbol_id_fkey FOREIGN KEY (engine_symbol_id) REFERENCES public.engine_symbols(id) ON DELETE CASCADE;


--
-- Name: app_config app_config_updated_by_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.app_config
    ADD CONSTRAINT app_config_updated_by_user_id_fkey FOREIGN KEY (updated_by_user_id) REFERENCES public.users(id);


--
-- Name: audit_log audit_log_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_log
    ADD CONSTRAINT audit_log_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE SET NULL;


--
-- Name: audit_log audit_log_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_log
    ADD CONSTRAINT audit_log_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: backfill_progress backfill_progress_engine_symbol_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.backfill_progress
    ADD CONSTRAINT backfill_progress_engine_symbol_id_fkey FOREIGN KEY (engine_symbol_id) REFERENCES public.engine_symbols(id) ON DELETE CASCADE;


--
-- Name: config_audit config_audit_changed_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_audit
    ADD CONSTRAINT config_audit_changed_by_fkey FOREIGN KEY (changed_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: config_scope config_scope_parent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_scope
    ADD CONSTRAINT config_scope_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.config_scope(id) ON DELETE RESTRICT;


--
-- Name: config_value config_value_key_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_value
    ADD CONSTRAINT config_value_key_fkey FOREIGN KEY (key) REFERENCES public.config_schema(key) ON DELETE CASCADE;


--
-- Name: config_value config_value_scope_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_value
    ADD CONSTRAINT config_value_scope_id_fkey FOREIGN KEY (scope_id) REFERENCES public.config_scope(id) ON DELETE CASCADE;


--
-- Name: config_value config_value_updated_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.config_value
    ADD CONSTRAINT config_value_updated_by_fkey FOREIGN KEY (updated_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: copy_subscriptions copy_subscriptions_follower_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_subscriptions
    ADD CONSTRAINT copy_subscriptions_follower_user_id_fkey FOREIGN KEY (follower_user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: copy_subscriptions copy_subscriptions_leader_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_subscriptions
    ADD CONSTRAINT copy_subscriptions_leader_user_id_fkey FOREIGN KEY (leader_user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: copy_trade_execution_jobs copy_trade_execution_jobs_follower_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_trade_execution_jobs
    ADD CONSTRAINT copy_trade_execution_jobs_follower_user_id_fkey FOREIGN KEY (follower_user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: copy_trade_execution_jobs copy_trade_execution_jobs_leader_exchange_order_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_trade_execution_jobs
    ADD CONSTRAINT copy_trade_execution_jobs_leader_exchange_order_id_fkey FOREIGN KEY (leader_exchange_order_id) REFERENCES public.exchange_orders(id) ON DELETE CASCADE;


--
-- Name: copy_trade_execution_jobs copy_trade_execution_jobs_leader_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_trade_execution_jobs
    ADD CONSTRAINT copy_trade_execution_jobs_leader_user_id_fkey FOREIGN KEY (leader_user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: copy_trade_execution_jobs copy_trade_execution_jobs_subscription_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.copy_trade_execution_jobs
    ADD CONSTRAINT copy_trade_execution_jobs_subscription_id_fkey FOREIGN KEY (subscription_id) REFERENCES public.copy_subscriptions(id) ON DELETE CASCADE;


--
-- Name: engine_symbol_ingestion_state engine_symbol_ingestion_state_engine_symbol_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbol_ingestion_state
    ADD CONSTRAINT engine_symbol_ingestion_state_engine_symbol_id_fkey FOREIGN KEY (engine_symbol_id) REFERENCES public.engine_symbols(id) ON DELETE CASCADE;


--
-- Name: engine_symbols engine_symbols_bar_interval_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbols
    ADD CONSTRAINT engine_symbols_bar_interval_id_fkey FOREIGN KEY (bar_interval_id) REFERENCES public.bar_intervals(id) ON DELETE SET NULL;


--
-- Name: engine_symbols engine_symbols_exchange_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbols
    ADD CONSTRAINT engine_symbols_exchange_id_fkey FOREIGN KEY (exchange_id) REFERENCES public.exchanges(id) ON DELETE SET NULL;


--
-- Name: engine_symbols engine_symbols_instrument_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbols
    ADD CONSTRAINT engine_symbols_instrument_id_fkey FOREIGN KEY (instrument_id) REFERENCES public.instruments(id) ON DELETE SET NULL;


--
-- Name: engine_symbols engine_symbols_market_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.engine_symbols
    ADD CONSTRAINT engine_symbols_market_id_fkey FOREIGN KEY (market_id) REFERENCES public.markets(id) ON DELETE SET NULL;


--
-- Name: exchange_accounts exchange_accounts_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_accounts
    ADD CONSTRAINT exchange_accounts_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: exchange_fills exchange_fills_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_fills
    ADD CONSTRAINT exchange_fills_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: exchange_fills exchange_fills_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_fills
    ADD CONSTRAINT exchange_fills_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: exchange_orders exchange_orders_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_orders
    ADD CONSTRAINT exchange_orders_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: exchange_orders exchange_orders_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.exchange_orders
    ADD CONSTRAINT exchange_orders_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: instruments instruments_market_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.instruments
    ADD CONSTRAINT instruments_market_id_fkey FOREIGN KEY (market_id) REFERENCES public.markets(id) ON DELETE CASCADE;


--
-- Name: intake_playbook_candidates intake_playbook_candidates_merged_engine_symbol_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.intake_playbook_candidates
    ADD CONSTRAINT intake_playbook_candidates_merged_engine_symbol_id_fkey FOREIGN KEY (merged_engine_symbol_id) REFERENCES public.engine_symbols(id) ON DELETE SET NULL;


--
-- Name: intake_playbook_candidates intake_playbook_candidates_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.intake_playbook_candidates
    ADD CONSTRAINT intake_playbook_candidates_run_id_fkey FOREIGN KEY (run_id) REFERENCES public.intake_playbook_runs(id) ON DELETE CASCADE;


--
-- Name: job_runs job_runs_job_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.job_runs
    ADD CONSTRAINT job_runs_job_id_fkey FOREIGN KEY (job_id) REFERENCES public.scheduled_jobs(id) ON DELETE CASCADE;


--
-- Name: liquidation_guard_events liquidation_guard_events_position_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.liquidation_guard_events
    ADD CONSTRAINT liquidation_guard_events_position_id_fkey FOREIGN KEY (position_id) REFERENCES public.live_positions(id) ON DELETE CASCADE;


--
-- Name: market_bars market_bars_bar_interval_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.market_bars
    ADD CONSTRAINT market_bars_bar_interval_id_fkey FOREIGN KEY (bar_interval_id) REFERENCES public.bar_intervals(id) ON DELETE SET NULL;


--
-- Name: market_bars market_bars_instrument_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.market_bars
    ADD CONSTRAINT market_bars_instrument_id_fkey FOREIGN KEY (instrument_id) REFERENCES public.instruments(id) ON DELETE SET NULL;


--
-- Name: markets markets_exchange_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.markets
    ADD CONSTRAINT markets_exchange_id_fkey FOREIGN KEY (exchange_id) REFERENCES public.exchanges(id) ON DELETE CASCADE;


--
-- Name: nansen_setup_rows nansen_setup_rows_run_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.nansen_setup_rows
    ADD CONSTRAINT nansen_setup_rows_run_id_fkey FOREIGN KEY (run_id) REFERENCES public.nansen_setup_runs(id) ON DELETE CASCADE;


--
-- Name: notify_delivery_prefs notify_delivery_prefs_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notify_delivery_prefs
    ADD CONSTRAINT notify_delivery_prefs_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: notify_outbox notify_outbox_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.notify_outbox
    ADD CONSTRAINT notify_outbox_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: oauth_clients oauth_clients_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_clients
    ADD CONSTRAINT oauth_clients_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: oauth_clients oauth_clients_service_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_clients
    ADD CONSTRAINT oauth_clients_service_user_id_fkey FOREIGN KEY (service_user_id) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: paper_balances paper_balances_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.paper_balances
    ADD CONSTRAINT paper_balances_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: paper_balances paper_balances_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.paper_balances
    ADD CONSTRAINT paper_balances_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: paper_fills paper_fills_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.paper_fills
    ADD CONSTRAINT paper_fills_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: paper_fills paper_fills_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.paper_fills
    ADD CONSTRAINT paper_fills_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: pivots pivots_engine_symbol_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pivots
    ADD CONSTRAINT pivots_engine_symbol_id_fkey FOREIGN KEY (engine_symbol_id) REFERENCES public.engine_symbols(id) ON DELETE CASCADE;


--
-- Name: pnl_rollups pnl_rollups_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.pnl_rollups
    ADD CONSTRAINT pnl_rollups_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: position_scale_events position_scale_events_position_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.position_scale_events
    ADD CONSTRAINT position_scale_events_position_id_fkey FOREIGN KEY (position_id) REFERENCES public.live_positions(id) ON DELETE CASCADE;


--
-- Name: q_radar_position_events q_radar_position_events_position_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.q_radar_position_events
    ADD CONSTRAINT q_radar_position_events_position_id_fkey FOREIGN KEY (position_id) REFERENCES public.qtss_positions(id) ON DELETE CASCADE;


--
-- Name: qtss_position_health_snapshots qtss_position_health_snapshots_setup_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_position_health_snapshots
    ADD CONSTRAINT qtss_position_health_snapshots_setup_id_fkey FOREIGN KEY (setup_id) REFERENCES public.qtss_setups(id) ON DELETE CASCADE;


--
-- Name: qtss_positions qtss_positions_setup_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_positions
    ADD CONSTRAINT qtss_positions_setup_id_fkey FOREIGN KEY (setup_id) REFERENCES public.qtss_setups(id) ON DELETE CASCADE;


--
-- Name: qtss_sessions qtss_sessions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_sessions
    ADD CONSTRAINT qtss_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.qtss_users(id) ON DELETE CASCADE;


--
-- Name: qtss_setup_lifecycle_events qtss_setup_lifecycle_events_setup_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_setup_lifecycle_events
    ADD CONSTRAINT qtss_setup_lifecycle_events_setup_id_fkey FOREIGN KEY (setup_id) REFERENCES public.qtss_setups(id) ON DELETE CASCADE;


--
-- Name: qtss_setup_outcomes qtss_setup_outcomes_setup_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_setup_outcomes
    ADD CONSTRAINT qtss_setup_outcomes_setup_id_fkey FOREIGN KEY (setup_id) REFERENCES public.qtss_setups(id) ON DELETE CASCADE;


--
-- Name: qtss_user_roles qtss_user_roles_role_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_user_roles
    ADD CONSTRAINT qtss_user_roles_role_id_fkey FOREIGN KEY (role_id) REFERENCES public.qtss_roles(id) ON DELETE CASCADE;


--
-- Name: qtss_user_roles qtss_user_roles_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_user_roles
    ADD CONSTRAINT qtss_user_roles_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.qtss_users(id) ON DELETE CASCADE;


--
-- Name: qtss_v2_setup_events qtss_v2_setup_events_setup_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.qtss_v2_setup_events
    ADD CONSTRAINT qtss_v2_setup_events_setup_id_fkey FOREIGN KEY (setup_id) REFERENCES public.qtss_setups(id) ON DELETE CASCADE;


--
-- Name: range_signal_events range_signal_events_engine_symbol_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.range_signal_events
    ADD CONSTRAINT range_signal_events_engine_symbol_id_fkey FOREIGN KEY (engine_symbol_id) REFERENCES public.engine_symbols(id) ON DELETE CASCADE;


--
-- Name: range_signal_paper_executions range_signal_paper_executions_range_signal_event_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.range_signal_paper_executions
    ADD CONSTRAINT range_signal_paper_executions_range_signal_event_id_fkey FOREIGN KEY (range_signal_event_id) REFERENCES public.range_signal_events(id) ON DELETE CASCADE;


--
-- Name: reconcile_reports reconcile_reports_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.reconcile_reports
    ADD CONSTRAINT reconcile_reports_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: refresh_tokens refresh_tokens_client_uuid_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_client_uuid_fkey FOREIGN KEY (client_uuid) REFERENCES public.oauth_clients(id) ON DELETE CASCADE;


--
-- Name: refresh_tokens refresh_tokens_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.refresh_tokens
    ADD CONSTRAINT refresh_tokens_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: symbol_category_map symbol_category_map_category_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.symbol_category_map
    ADD CONSTRAINT symbol_category_map_category_id_fkey FOREIGN KEY (category_id) REFERENCES public.asset_categories(id);


--
-- Name: system_config_audit system_config_audit_changed_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_config_audit
    ADD CONSTRAINT system_config_audit_changed_by_fkey FOREIGN KEY (changed_by) REFERENCES public.users(id) ON DELETE SET NULL;


--
-- Name: system_config system_config_updated_by_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.system_config
    ADD CONSTRAINT system_config_updated_by_user_id_fkey FOREIGN KEY (updated_by_user_id) REFERENCES public.users(id);


--
-- Name: user_permissions user_permissions_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_permissions
    ADD CONSTRAINT user_permissions_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: user_roles user_roles_role_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_role_id_fkey FOREIGN KEY (role_id) REFERENCES public.roles(id) ON DELETE CASCADE;


--
-- Name: user_roles user_roles_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_roles
    ADD CONSTRAINT user_roles_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: users users_org_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_org_id_fkey FOREIGN KEY (org_id) REFERENCES public.organizations(id) ON DELETE CASCADE;


--
-- Name: wave_chain wave_chain_parent_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wave_chain
    ADD CONSTRAINT wave_chain_parent_id_fkey FOREIGN KEY (parent_id) REFERENCES public.wave_chain(id) ON DELETE SET NULL;


--
-- Name: wave_projections wave_projections_source_wave_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.wave_projections
    ADD CONSTRAINT wave_projections_source_wave_id_fkey FOREIGN KEY (source_wave_id) REFERENCES public.wave_chain(id) ON DELETE CASCADE;


--
-- Name: x_outbox x_outbox_lifecycle_event_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.x_outbox
    ADD CONSTRAINT x_outbox_lifecycle_event_id_fkey FOREIGN KEY (lifecycle_event_id) REFERENCES public.qtss_setup_lifecycle_events(id) ON DELETE SET NULL;


--
-- Name: x_outbox x_outbox_setup_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.x_outbox
    ADD CONSTRAINT x_outbox_setup_id_fkey FOREIGN KEY (setup_id) REFERENCES public.qtss_setups(id) ON DELETE SET NULL;


--
-- PostgreSQL database dump complete
--

\unrestrict ndVPCWukwZS8i9ReRo5DYyqmARSzcbX5lgr1YytvyeykGBc9PXk3RW2d7Q6APkc

