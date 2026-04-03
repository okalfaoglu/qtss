import type { ServerResponse } from "node:http";
import react from "@vitejs/plugin-react";
import { loadEnv, type ProxyOptions } from "vite";
import { defineConfig } from "vitest/config";

/** Vite sunucusunun `/__binance` → hedef köprü tabanı (coğrafi kısıtta özelleştirilir). */
function binanceProxyTargetFromEnv(mode: string): string {
  const env = loadEnv(mode, process.cwd(), "");
  const raw = env.VITE_BINANCE_PROXY_TARGET?.trim() ?? "";
  if (raw && /^https?:\/\//i.test(raw)) {
    return raw.replace(/\/$/, "");
  }
  return "https://api.binance.com";
}

function binanceFapiProxyTargetFromEnv(mode: string): string {
  const env = loadEnv(mode, process.cwd(), "");
  const raw = env.VITE_BINANCE_FAPI_PROXY_TARGET?.trim() ?? "";
  if (raw && /^https?:\/\//i.test(raw)) {
    return raw.replace(/\/$/, "");
  }
  return "https://fapi.binance.com";
}

/** One proxy entry per path so each `http-proxy` instance gets its own `error` listener. */
function qtssApiProxyEntry(target: string): ProxyOptions {
  return {
    target,
    changeOrigin: true,
    configure(proxy) {
      proxy.on("error", (err, _req, res) => {
        if (!res || typeof (res as ServerResponse).writeHead !== "function") return;
        const r = res as ServerResponse;
        if (r.headersSent) return;
        const msg = err instanceof Error ? err.message : String(err);
        r.writeHead(502, { "Content-Type": "application/json; charset=utf-8" });
        r.end(
          JSON.stringify({
            error: "proxy_upstream_unreachable",
            error_description: `Vite proxy could not reach qtss-api at ${target} (${msg}). Set system_config api.web_dev_proxy_target (or QTSS_API_PROXY_TARGET when QTSS_CONFIG_ENV_OVERRIDES=1). Ensure DATABASE_URL is set in the Vite process so the config can be read from PostgreSQL.`,
          }),
        );
      });
    },
  };
}

function qtssApiProxyMapForTarget(target: string): Record<string, ProxyOptions> {
  const entry = qtssApiProxyEntry(target);
  return {
    "/api": entry,
    "/oauth": entry,
    "/health": entry,
  };
}

export default defineConfig(async ({ mode }) => {
  const { resolveQtssApiProxyTarget } = await import("./scripts/resolveQtssApiProxyTarget.mjs");
  const qtssApiProxyTarget = await resolveQtssApiProxyTarget(mode);
  const binanceProxyTarget = binanceProxyTargetFromEnv(mode);
  const binanceFapiProxyTarget = binanceFapiProxyTargetFromEnv(mode);
  const qtssApiProxyMap = qtssApiProxyMapForTarget(qtssApiProxyTarget);
  return {
    plugins: [react()],
    build: {
      /** Default 500 — main `index-*.js` stays larger after splitting only react + charts; not an error. */
      chunkSizeWarningLimit: 700,
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (id.includes("node_modules/react-dom")) return "vendor-react";
            if (id.includes("node_modules/react/")) return "vendor-react";
            if (id.includes("node_modules/scheduler/")) return "vendor-react";
            if (id.includes("lightweight-charts")) return "vendor-charts";
            return undefined;
          },
        },
      },
    },
    test: {
      include: ["src/**/*.test.ts"],
      environment: "node",
    },
    /** Binance proxy: `/__binance_fapi` MUST be listed before `/__binance` — otherwise `/__binance_fapi/...` matches the spot prefix and rewrites to a bogus path on api.binance.com (404). */
    server: {
      port: 5173,
      proxy: {
        ...qtssApiProxyMap,
        "/__binance_fapi": {
          target: binanceFapiProxyTarget,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/__binance_fapi/, ""),
        },
        "/__binance": {
          target: binanceProxyTarget,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/__binance/, ""),
        },
      },
    },
    preview: {
      host: true,
      port: 4173,
      proxy: {
        ...qtssApiProxyMap,
        "/__binance_fapi": {
          target: binanceFapiProxyTarget,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/__binance_fapi/, ""),
        },
        "/__binance": {
          target: binanceProxyTarget,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/__binance/, ""),
        },
      },
    },
  };
});
