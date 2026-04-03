/**
 * Resolves qtss-api base URL for Vite proxy (dev/preview).
 * Precedence matches `qtss_storage::resolve_system_string` for this key:
 * 1) QTSS_CONFIG_ENV_OVERRIDES truthy + non-empty QTSS_API_PROXY_TARGET
 * 2) system_config api.web_dev_proxy_target (when DATABASE_URL is set)
 * 3) QTSS_API_PROXY_TARGET
 * 4) http://127.0.0.1:8080
 */
import { loadEnv } from "vite";

const ENV_KEY = "QTSS_API_PROXY_TARGET";

/** @param {Record<string, string>} env */
function envOverridesEnabled(env) {
  const raw =
    (env.QTSS_CONFIG_ENV_OVERRIDES ?? process.env.QTSS_CONFIG_ENV_OVERRIDES ?? "").trim().toLowerCase();
  return ["1", "true", "yes", "on"].includes(raw);
}

/** @param {unknown} value */
function stringFromConfigValue(value) {
  if (value == null) return null;
  if (typeof value === "string") {
    const t = value.trim();
    return t || null;
  }
  if (typeof value === "object" && value !== null && "value" in value) {
    const v = /** @type {{ value?: unknown }} */ (value).value;
    if (typeof v === "string") {
      const t = v.trim();
      return t || null;
    }
  }
  return null;
}

/** @param {string} s */
function normalizeProxyUrl(s) {
  const t = s.trim();
  if (!/^https?:\/\//i.test(t)) return null;
  return t.replace(/\/$/, "");
}

/**
 * @param {string} mode Vite mode (development / production)
 * @returns {Promise<string>}
 */
export async function resolveQtssApiProxyTarget(mode) {
  const env = { ...process.env, ...loadEnv(mode, process.cwd(), "") };

  if (envOverridesEnabled(env)) {
    const o = env[ENV_KEY]?.trim();
    if (o) {
      const n = normalizeProxyUrl(o);
      if (n) return n;
    }
  }

  const dbUrl = env.DATABASE_URL?.trim() || env.QTSS_DATABASE_URL?.trim() || "";

  if (dbUrl) {
    const { default: postgres } = await import("postgres");
    const sql = postgres(dbUrl, { max: 1 });
    try {
      const rows = await sql`
        SELECT value FROM system_config
        WHERE module = ${"api"} AND config_key = ${"web_dev_proxy_target"}
        LIMIT 1
      `;
      const row = rows[0];
      const s = row ? stringFromConfigValue(row.value) : null;
      if (s) {
        const n = normalizeProxyUrl(s);
        if (n) return n;
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.warn("[qtss-web] Could not read system_config api.web_dev_proxy_target:", msg);
    } finally {
      await sql.end({ timeout: 5 }).catch(() => {});
    }
  }

  const fallback = env[ENV_KEY]?.trim() || "";
  if (fallback) {
    const n = normalizeProxyUrl(fallback);
    if (n) return n;
  }

  return "http://127.0.0.1:8080";
}
