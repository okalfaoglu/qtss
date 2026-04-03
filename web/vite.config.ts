import react from "@vitejs/plugin-react";
import { loadEnv } from "vite";
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

export default defineConfig(({ mode }) => {
  const binanceProxyTarget = binanceProxyTargetFromEnv(mode);
  const binanceFapiProxyTarget = binanceFapiProxyTargetFromEnv(mode);
  const qtssApiProxyTarget = qtssApiProxyTargetFromEnv(mode);
  const qtssApiProxy = {
    "/api": { target: qtssApiProxyTarget, changeOrigin: true },
    "/oauth": { target: qtssApiProxyTarget, changeOrigin: true },
    "/health": { target: qtssApiProxyTarget, changeOrigin: true },
  } as const;
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
        ...qtssApiProxy,
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
      port: 4173,
      proxy: {
        ...qtssApiProxy,
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
