# Nansen / smart-money playbook (LLM prompts + machine JSON)

This document turns the strategy specs you listed into **copy-paste system/user prompts** and a **single JSON output contract** so every playbook returns comparable fields: symbol, direction, entry, stop, targets, risk, confidence, and reasons.

## How to attach real data (QTSS)

1. **Worker + Nansen**: Ensure `qtss-worker` is running with `NANSEN_API_KEY` and the loops you need enabled (admin toggles / `system_config` / env — see `NansenApiCreditsPanel` and `qtss-worker` Nansen loops).
2. **Rule-based intake (no LLM credits)** — `qtss-worker` `intake_playbook_engine` (off by default): set `QTSS_INTAKE_PLAYBOOK_ENABLED=1` or `system_config` `worker` / `intake_playbook_loop_enabled` → `{ "enabled": true }`. Tick: `QTSS_INTAKE_PLAYBOOK_TICK_SECS` or `intake_playbook_tick_secs` (default 300s). Writes `intake_playbook_runs` + `intake_playbook_candidates` from `data_snapshots` (`nansen_token_screener`, `nansen_netflows`, `nansen_flow_intelligence`, `nansen_perp_trades`, Binance `binance_premium_btcusdt` / `ethusdt`). **API:** `GET /api/v1/analysis/intake-playbook/latest?playbook_id=market_mode` (and `elite_short`, `elite_long`, `ten_x_alert`, `institutional_exit`, `institutional_accumulation`, `explosive_high_risk`, `early_accumulation_24h`), `GET /api/v1/analysis/intake-playbook/recent?limit=50`. **Promote (trader/admin):** `POST …/intake-playbook/promote` `{ "candidate_id": "uuid" }`, `POST …/intake-playbook/promote-bulk` `{ "candidate_ids": ["…"] }` (max 25) — creates `engine_symbols` **disabled** by default; if the series already exists, only links `merged_engine_symbol_id` and leaves `enabled` unchanged. Heuristics are best-effort; use LLM prompts below for refinement.
3. **Pull a context bundle** (Bearer JWT with at least viewer):

   ```bash
   node scripts/fetch_qtss_smart_money_context.mjs > context.json
   ```

   Env:

   - `QTSS_API_BASE` — default `http://127.0.0.1:8080`
   - `QTSS_BEARER_TOKEN` — required

   The script calls:

   - `GET /api/v1/analysis/data-snapshots` (all `source_key` rows: `nansen_netflows`, `nansen_holdings`, `nansen_flow_intelligence`, TGM keys, etc.)
   - `GET /api/v1/analysis/engine/confluence/latest`
   - `GET /api/v1/analysis/nansen/snapshot` (token screener snapshot row)
   - `GET /api/v1/analysis/nansen/setups/latest`
   - `GET /api/v1/analysis/market-context/summary?limit=200&enabled_only=true` (optional broad tape)

4. **Paste** `context.json` (or excerpts) into the user message under a fenced `json` block labeled `qtss_context`.

5. **Direct Nansen API** (outside QTSS): You may add raw Nansen responses for endpoints not yet mirrored in `data_snapshots`. Label each block with `source` and `endpoint`.

## Global rules for the model

- Reply with **one JSON object only** — raw JSON, no markdown fences, no commentary.
- Use **numbers** for prices and USD where possible; use `null` if unknown.
- **Do not invent** fills for missing data: set fields to `null` and list gaps under `data_gaps`.
- **Identifiers** in JSON keys stay English (`symbol`, `direction`, `confidence_pct`, …).

## Shared types (use in every playbook)

```json
{
  "playbook_id": "market_mode | elite_short | elite_long | ten_x_alert | token_deep_dive | institutional_exit | institutional_accumulation",
  "generated_at": "ISO-8601 UTC string",
  "data_gaps": ["string — what was missing for a full read"],

  "trade_candidate": {
    "symbol": "BASE_ASSET or TOKEN_TICKER",
    "chain": "ethereum | base | bnb | solana | null",
    "venue_hint": "binance | mexc | dex | null",
    "direction": "LONG | SHORT | WATCH | AVOID | NEUTRAL",
    "entry": {
      "style": "now | zone | wait_pump",
      "price": null,
      "price_min": null,
      "price_max": null,
      "note": "string"
    },
    "stop_loss": { "price": null, "pct_from_entry": null },
    "take_profit": [
      { "label": "TP1", "pct_from_entry": null, "price": null },
      { "label": "TP2", "pct_from_entry": null, "price": null }
    ],
    "confidence_pct": 0,
    "risk_tier": "low | medium | high",
    "time_horizon": "e.g. 1-4h, 1-6h, minutes",
    "key_reason": "main signal in one line",
    "flow_reason": "on-chain / CEX flow justification",
    "urgency": "low | medium | high"
  }
}
```

### Playbook-specific top-level fields

**`market_mode`**

- `current_mode`: `LONG_MODE | SHORT_MODE | NEUTRAL`
- `confidence_pct`, `key_reason`
- If `LONG_MODE`: `candidates`: array, **top 10** `trade_candidate`, each `direction` LONG, each `take_profit` should reflect **≥10%** potential (or `null` with `data_gaps`).
- If `SHORT_MODE`: **top 3** shorts, **≥10%** downside.
- If `NEUTRAL`: `neutral_guidance`: `"wait / scalp only"`; `candidates` may be `[]`.

**`elite_short`**

- `candidates`: **max 3** `trade_candidate`, `direction` SHORT; targets in **-20% to -30%** range when data supports it.

**`elite_long`**

- `candidates`: **max 3** `trade_candidate`, `direction` LONG; target ≥ **+30%** when data supports it.

**`ten_x_alert`**

- `triggered`: boolean
- If `false`: only `playbook_id`, `generated_at`, `triggered`, `data_gaps` (optional).
- If `true`: single primary `trade_candidate` plus `alert_reason` (smart money + wallets + volume), `take_profit` tiers **+25% / +50% / +100%**, `stop_loss` **-10% to -15%** as `pct_from_entry`.

**`token_deep_dive`**

- `token`: string
- Sections 1–8 as structured objects (`smart_money`, `flow_context`, `narrative`, `volume`, `market_phase`, `risk`, `trade_plan`, `short_opportunity`) plus `verdict`: `BUY | WAIT | SHORT`.

**`institutional_exit`**

- `candidates`: **max 5**; table-oriented fields inside each: same `trade_candidate` plus `flow_reason`, `urgency`; `direction` typically `SHORT` or `AVOID`.

**`institutional_accumulation`**

- `candidates`: **max 5**; `direction` `LONG` or `WATCH`.

---

## 1) Market mode (smart money regime)

**User prompt (strategy)**

Determine current market mode in real-time using smart money data.

LONG MODE CONDITIONS:

- Smart Money net inflow on majors (ETH/BTC) > $10M
- Exchange outflows increasing (accumulation)
- Funding neutral or negative
- Stablecoins moving OUT of exchanges
- Top traders accumulating

SHORT MODE CONDITIONS:

- Whale deposits to CEX increasing (>$5M total)
- Smart Money outflow on majors
- Funding positive (long overcrowded)
- Top traders distributing
- Stablecoins moving INTO exchanges

OUTPUT (human-readable intent — **you must still emit the JSON contract below**):

- Current Mode: LONG MODE / SHORT MODE / NEUTRAL
- Confidence score (0–100%)
- Key reason (main signal)

IF LONG MODE: Show top 10 long candidates (≥10% potential)  
IF SHORT MODE: Show top 3 short candidates (≥10% downside)  
IF NEUTRAL: Recommend “wait / scalp only”

**Machine output:** JSON with `playbook_id`: `market_mode` and fields under “Playbook-specific top-level fields”.

---

## 2) Elite short (-30% dump, 1–4h)

**User prompt (strategy)**

Find only high-probability short opportunities with -30% downside potential within 1–4 hours.

STRICT CONDITIONS:

- Whale or Smart Money deposits to CEX > $500K
- Repeated deposits (same token multiple times)
- Smart Money net outflow increasing
- Top traders reducing positions
- Funding positive (long crowding)
- Liquidity above price already swept (trap)

BONUS SIGNALS:

- Market makers distributing (Wintermute, Cumberland)
- Sudden volume spike + rejection wick
- Weak liquidity / low-mid market cap

AVOID:

- Coins with accumulation signals
- Strong support zones below

OUTPUT (human intent): Top 3 coins only; short entry zone (wait for pump); target (-20% to -30%); stop loss; time to dump (1–4h); confidence score.

**Machine output:** JSON with `playbook_id`: `elite_short`.

---

## 3) Elite long (+30% pump, 1–6h)

**User prompt (strategy)**

Find only explosive long opportunities with +30% or higher upside potential within 1–6 hours.

STRICT CONDITIONS:

- Smart Money net inflow > $500K (last 1–3h)
- Exchange outflows (clear accumulation)
- Multiple whale wallets buying (not single tx)
- Buy/Sell ratio > 2.0
- Volume spike ≥2–3x (just starting, not peaked)
- Price still early (<5–7% move)
- Breakout structure forming (tight consolidation)

BONUS SIGNALS:

- Market makers (Wintermute, Cumberland, Abraxas) buying
- Repeated buys across multiple wallets
- Low–mid market cap ($5M–$100M preferred)

AVOID:

- Tokens with any CEX inflow
- Already pumped coins
- Weak liquidity traps

OUTPUT (human intent): Top 3 coins; entry price NOW; target (≥30%); stop loss; time to breakout; confidence score.

**Machine output:** JSON with `playbook_id`: `elite_long`.

---

## 4) 10X alert system (sparse alerts)

**User prompt (strategy)**

Continuously monitor Smart Money activity and alert ONLY when a high-conviction 10x candidate appears.

TRIGGER CONDITIONS:

- Smart Money inflow (1h) > $100K
- Fresh wallet inflow detected
- At least 3 Smart Money wallets buying
- Buy volume > sell volume (>65%)
- Market cap < $30M
- Liquidity $300K – $5M
- No significant CEX inflow
- Token not pumped >20%

CONFIRMATION:

- Volume increasing rapidly
- Repeated buys (not single spike)

OUTPUT ONLY WHEN CONDITIONS MET: Token; entry price; reason; confidence; TP +25% / +50% / +100%; SL -10% to -15%

**Machine output:** JSON with `playbook_id`: `ten_x_alert` and `triggered` boolean; if false, minimal object.

---

## 5) Single-token deep dive

**User prompt (strategy)**

Analyze this token using smart money and flow data:

Token: [TOKEN NAME]

Evaluate:

1. Smart money: inflow size and tx count; multiple wallets or single whale?
2. Flow context: stablecoin inflow into market? smart money rotating into sector?
3. Narrative: trending sector (AI, meme, L2, RWA)?
4. Volume: expanding or fading?
5. Market phase: accumulation / breakout / distribution
6. Risk: low / medium / high
7. Trade plan: entry, SL, target
8. Short opportunity: smart money exit signs?

Verdict: BUY / WAIT / SHORT

**Machine output:** JSON with `playbook_id`: `token_deep_dive` and structured sections + `verdict`.

---

## 6) Institutional exit detector (distribution)

**User prompt (strategy)**

Act as a real-time institutional exit detector. Scan tokens (ETH, BNB, Base) listed on Binance/MEXC. Find coins where smart money or custody wallets (Coinbase Prime, Wintermute, FalconX, Jump, Amber, GSR) are moving tokens to exchanges.

Criteria:

- CEX inflow >$300k in last 1h
- Repeated transfers (2+ tx) from same entity
- Custody → CEX or fund → CEX patterns
- Exchange inflow spike >2x baseline
- No matching outflow (no accumulation)
- Smart money netflow ≤ 0
- Price up or flat (distribution phase)

Exclude: new launches (<7d), low liquidity (<$500k), isolated transfers

Return top 5: Token | Direction (SHORT/AVOID) | Entry | SL | TP1/TP2 | Flow reason | Urgency

**Machine output:** JSON with `playbook_id`: `institutional_exit`, `candidates` length ≤ 5.

---

## 7) Institutional accumulation detector

**User prompt (strategy)**

Act as a real-time institutional accumulation detector. Scan tokens (ETH, BNB, Base) listed on Binance/MEXC. Find coins where smart money or custody wallets are accumulating from exchanges or fresh wallets.

Criteria:

- CEX outflow >$300k in last 1h
- Repeated transfers (2+ tx) to same wallet/entity
- CEX → wallet or fund accumulation pattern
- Exchange outflow spike >2x baseline
- Smart money netflow positive
- Early stage (no >40% pump in last 7d)
- Volume increasing (trend confirmation)

Exclude: low liquidity (<$500k), one-off transfers, obvious airdrop wallets

Return top 5: Token | Direction (LONG/WATCH) | Entry | SL | TP1/TP2 | Flow reason | Urgency

**Machine output:** JSON with `playbook_id`: `institutional_accumulation`, `candidates` length ≤ 5.

---

## Wiring into QTSS AI (optional)

Tactical layer prompts live in `crates/qtss-ai/src/client.rs` (`ai_prompt_tactical` override in `app_config`). You can add a **separate** `app_config` key via admin API for these playbooks (e.g. `ai_prompt_nansen_playbook`) and call the LLM from a small internal job — that integration is product-specific; this file defines **what** to ask and **what shape** to parse.
