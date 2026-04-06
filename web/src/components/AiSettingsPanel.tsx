import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { fetchWithBearerRetry, upsertAppConfig } from "../api/client";

type Props = {
  accessToken: string | null;
  /** `app_config` uçları `require_admin`; false iken istek atılmaz. */
  canAdmin?: boolean;
};

type AiConfig = {
  enabled: boolean;
  tactical_layer_enabled: boolean;
  operational_layer_enabled: boolean;
  strategic_layer_enabled: boolean;
  provider_tactical: string;
  provider_operational: string;
  provider_strategic: string;
  model_tactical: string;
  model_operational: string;
  model_strategic: string;
  max_tokens_tactical: number;
  max_tokens_operational: number;
  max_tokens_strategic: number;
  require_min_confidence: number;
  decision_ttl_secs: number;
  auto_approve_threshold: number;
  auto_approve_enabled: boolean;
  output_locale: string | null;
};

const API_BASE = (import.meta as Record<string, Record<string, string>>).env?.VITE_API_BASE ?? "";

const AI_ENGINE_CONFIG_KEY = "ai_engine_config";

const GEMINI_PROVIDER_IDS = new Set(["gemini", "google", "google_gemini"]);

function detectUniformVendor(c: AiConfig): "anthropic" | "gemini" | "custom" {
  const p = [c.provider_tactical, c.provider_operational, c.provider_strategic].map((x) => x.trim().toLowerCase());
  if (p.every((x) => x === "anthropic")) return "anthropic";
  if (p.every((x) => GEMINI_PROVIDER_IDS.has(x))) return "gemini";
  return "custom";
}

/** Applies recommended models per vendor; keeps all other `ai_engine_config` fields (keys, max_tokens, etc.). */
function applyVendorPreset(base: AiConfig, vendor: "anthropic" | "gemini"): AiConfig {
  if (vendor === "anthropic") {
    return {
      ...base,
      provider_tactical: "anthropic",
      provider_operational: "anthropic",
      provider_strategic: "anthropic",
      model_tactical: "claude-haiku-4-5-20251001",
      model_operational: "claude-haiku-4-5-20251001",
      model_strategic: "claude-sonnet-4-20250514",
    };
  }
  return {
    ...base,
    provider_tactical: "gemini",
    provider_operational: "gemini",
    provider_strategic: "gemini",
    model_tactical: "gemini-2.5-flash",
    model_operational: "gemini-2.5-flash",
    model_strategic: "gemini-2.5-flash",
    // Verbose `reasoning` often hits default 1024 and truncates mid-JSON (parse_error: unbalanced braces).
    max_tokens_tactical: Math.max(base.max_tokens_tactical, 3072),
  };
}

async function fetchAiConfig(token: string): Promise<AiConfig | null> {
  const r = await fetchWithBearerRetry(
    `${API_BASE}/api/v1/config/${encodeURIComponent(AI_ENGINE_CONFIG_KEY)}`,
    token,
  );
  if (!r.ok) return null;
  const j = (await r.json()) as { value?: AiConfig };
  return (j?.value ?? j) as AiConfig;
}

export function AiSettingsPanel({ accessToken, canAdmin }: Props) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<AiConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const refresh = useCallback(async () => {
    if (!accessToken) return;
    if (canAdmin === false) {
      setConfig(null);
      setErr("");
      return;
    }
    setBusy(true);
    setErr("");
    try {
      const c = await fetchAiConfig(accessToken);
      setConfig(c);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }, [accessToken, canAdmin]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (!accessToken) {
    return <p className="muted">{t("ai.loginPrompt")}</p>;
  }

  if (canAdmin === false) {
    return (
      <div className="card" style={{ marginTop: "0.5rem" }}>
        <p className="tv-drawer__section-head">{t("ai.settings.title")}</p>
        <p className="muted" style={{ fontSize: "0.85rem", marginTop: "0.5rem" }}>
          {t("ai.settings.adminOnly")}
        </p>
      </div>
    );
  }

  if (!config) {
    return (
      <div className="card" style={{ marginTop: "0.5rem" }}>
        <p className="tv-drawer__section-head">{t("ai.settings.title")}</p>
        {err ? <p className="tv-drawer__error">{err}</p> : null}
        <p className="muted">{busy ? t("ai.settings.loading") : t("ai.settings.notConfigured")}</p>
        <button type="button" onClick={() => void refresh()} disabled={busy} style={{ marginTop: "0.5rem" }}>
          {t("ai.refresh")}
        </button>
      </div>
    );
  }

  const layers = [
    {
      key: "tactical",
      label: t("ai.dashboard.tactical"),
      enabled: config.tactical_layer_enabled,
      provider: config.provider_tactical,
      model: config.model_tactical,
      maxTokens: config.max_tokens_tactical,
      color: "#4fc3f7",
    },
    {
      key: "operational",
      label: t("ai.dashboard.operational"),
      enabled: config.operational_layer_enabled,
      provider: config.provider_operational,
      model: config.model_operational,
      maxTokens: config.max_tokens_operational,
      color: "#aed581",
    },
    {
      key: "strategic",
      label: t("ai.dashboard.strategic"),
      enabled: config.strategic_layer_enabled,
      provider: config.provider_strategic,
      model: config.model_strategic,
      maxTokens: config.max_tokens_strategic,
      color: "#ce93d8",
    },
  ];

  const uniformVendor = detectUniformVendor(config);
  const usesGemini = layers.some((l) =>
    ["gemini", "google", "google_gemini"].includes(l.provider.trim().toLowerCase()),
  );

  return (
    <div className="card" style={{ marginTop: "0.5rem" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <p className="tv-drawer__section-head">{t("ai.settings.title")}</p>
        <button type="button" disabled={busy} onClick={() => void refresh()} style={{ fontSize: "0.75rem" }}>
          {t("ai.refresh")}
        </button>
      </div>
      {err ? <p className="tv-drawer__error">{err}</p> : null}

      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "0.5rem",
          marginBottom: "0.75rem",
          fontSize: "0.85rem",
        }}
      >
        <span
          style={{
            display: "inline-block",
            width: 10,
            height: 10,
            borderRadius: "50%",
            background: config.enabled ? "#66bb6a" : "#ef5350",
          }}
        />
        <strong>{config.enabled ? t("ai.settings.engineOn") : t("ai.settings.engineOff")}</strong>
      </div>

      {!config.enabled ? (
        <p
          className="muted"
          style={{
            fontSize: "0.78rem",
            lineHeight: 1.55,
            marginBottom: "0.85rem",
            padding: "0.55rem 0.65rem",
            borderRadius: 6,
            border: "1px solid var(--border-subtle, #444)",
            background: "var(--panel-elevated, rgba(255,255,255,0.04))",
          }}
        >
          {t("ai.settings.engineOffExplainer")}
        </p>
      ) : null}

      <div className="tv-settings__fields" style={{ marginBottom: "0.85rem" }}>
        <label style={{ display: "flex", flexDirection: "column", gap: "0.25rem", maxWidth: "22rem" }}>
          <span className="muted" style={{ fontSize: "0.8rem" }}>
            {t("ai.settings.llmVendor")}
          </span>
          <select
            className="mono"
            value={uniformVendor === "custom" ? "" : uniformVendor}
            disabled={busy}
            onChange={(e) => {
              const v = e.target.value;
              if (v !== "anthropic" && v !== "gemini") return;
              void (async () => {
                if (!accessToken) return;
                setBusy(true);
                setErr("");
                try {
                  const next = applyVendorPreset(config, v);
                  await upsertAppConfig(accessToken, {
                    key: AI_ENGINE_CONFIG_KEY,
                    value: next,
                  });
                  setConfig(next);
                } catch (e) {
                  setErr(String(e));
                } finally {
                  setBusy(false);
                }
              })();
            }}
          >
            <option value="">{t("ai.settings.vendorMixedPlaceholder")}</option>
            <option value="anthropic">{t("ai.settings.vendorAnthropic")}</option>
            <option value="gemini">{t("ai.settings.vendorGemini")}</option>
          </select>
        </label>
        <p className="muted" style={{ fontSize: "0.68rem", lineHeight: 1.45, marginTop: "0.35rem", maxWidth: "28rem" }}>
          {t("ai.settings.llmVendorHint")}
        </p>
        {usesGemini ? (
          <p
            className="muted"
            style={{
              fontSize: "0.68rem",
              lineHeight: 1.5,
              marginTop: "0.5rem",
              maxWidth: "32rem",
              padding: "0.45rem 0.55rem",
              borderRadius: 6,
              border: "1px solid rgba(255, 193, 7, 0.35)",
              background: "rgba(255, 193, 7, 0.06)",
            }}
          >
            {t("ai.settings.geminiKeyReminder")}
          </p>
        ) : null}
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "0.4rem", fontSize: "0.8rem", marginBottom: "1rem" }}>
        <div>
          <span className="muted">{t("ai.settings.minConfidence")}: </span>
          <strong>{config.require_min_confidence}</strong>
        </div>
        <div>
          <span className="muted">{t("ai.settings.decisionTTL")}: </span>
          <strong>{config.decision_ttl_secs}s</strong>
        </div>
        <div>
          <span className="muted">{t("ai.settings.autoApprove")}: </span>
          <strong>
            {config.auto_approve_enabled ? `≥${config.auto_approve_threshold}` : t("ai.settings.disabled")}
          </strong>
        </div>
        <div>
          <span className="muted">{t("ai.settings.locale")}: </span>
          <strong>{config.output_locale ?? "en"}</strong>
        </div>
      </div>

      <p className="tv-drawer__section-head">{t("ai.settings.layerConfig")}</p>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "0.5rem" }}>
        {layers.map((l) => (
          <div
            key={l.key}
            style={{
              border: `1px solid ${l.color}44`,
              borderRadius: 8,
              padding: "0.6rem",
              background: `${l.color}0a`,
              opacity: l.enabled ? 1 : 0.5,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: "0.3rem", marginBottom: "0.3rem" }}>
              <span
                style={{
                  display: "inline-block",
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: l.enabled ? "#66bb6a" : "#9e9e9e",
                }}
              />
              <strong style={{ color: l.color, fontSize: "0.8rem" }}>{l.label}</strong>
            </div>
            <div style={{ fontSize: "0.7rem", lineHeight: 1.7 }}>
              <div>
                <span className="muted">Provider: </span>
                <span className="mono">{l.provider}</span>
              </div>
              <div>
                <span className="muted">Model: </span>
                <span className="mono">{l.model}</span>
              </div>
              <div>
                <span className="muted">Max tokens: </span>
                <span>{l.maxTokens}</span>
              </div>
            </div>
          </div>
        ))}
      </div>

      <p className="muted" style={{ fontSize: "0.7rem", lineHeight: 1.5, marginTop: "0.65rem" }}>
        {t("ai.settings.providersReference")}
      </p>

      {!canAdmin ? (
        <p className="muted" style={{ fontSize: "0.7rem", marginTop: "0.75rem" }}>
          {t("ai.settings.adminOnly")}
        </p>
      ) : null}
    </div>
  );
}
