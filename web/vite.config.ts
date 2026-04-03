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

/** `qtss-api` base URL for Vite dev/preview proxies (`/api`, `/oauth`, `/health`). Not exposed to the browser bundle. */
function qtssApiProxyTargetFromEnv(mode: string): string {
  const env = loadEnv(mode, process.cwd(), "");
  const raw = env.QTSS_API_PROXY_TARGET?.trim() ?? "";
  if (raw && /^https?:\/\//i.test(raw)) {
    return raw.replace(/\/$/, "");
  }
  return "http://127.0.0.1:8080";
}

/** One proxy entry per path so each `http-proxy` instance gets its own `error` listener. */
function qtssApiProxyEntry(mode: string): ProxyOptions {
  const target = qtssApiProxyTargetFromEnv(mode);
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
            error_description: `Vite proxy could not reach qtss-api at ${target} (${msg}). Set QTSS_API_PROXY_TARGET in web/.env to a URL reachable from the Node process (e.g. if preview runs on Windows and qtss-api runs in WSL, use the WSL Ethernet IP:8080, not 127.0.0.1).`,
          }),
        );
      });
    },
  };
}

function qtssApiProxy(mode: string): Record<string, ProxyOptions> {
  return {
    "/api": qtssApiProxyEntry(mode),
    "/oauth": qtssApiProxyEntry(mode),
    "/health": qtssApiProxyEntry(mode),
  };
}

export default defineConfig(({ mode }) => {
  const binanceProxyTarget = binanceProxyTargetFromEnv(mode);
  const binanceFapiProxyTarget = binanceFapiProxyTargetFromEnv(mode);
  const qtssApiProxyMap = qtssApiProxy(mode);
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
