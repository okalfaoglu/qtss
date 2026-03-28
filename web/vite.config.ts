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

export default defineConfig(({ mode }) => {
  const binanceProxyTarget = binanceProxyTargetFromEnv(mode);
  const binanceFapiProxyTarget = binanceFapiProxyTargetFromEnv(mode);
  return {
    plugins: [react()],
    build: {
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
    server: {
      port: 5173,
      proxy: {
        "/api": { target: "http://127.0.0.1:8080", changeOrigin: true },
        "/oauth": { target: "http://127.0.0.1:8080", changeOrigin: true },
        "/health": { target: "http://127.0.0.1:8080", changeOrigin: true },
        "/__binance": {
          target: binanceProxyTarget,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/__binance/, ""),
        },
        "/__binance_fapi": {
          target: binanceFapiProxyTarget,
          changeOrigin: true,
          rewrite: (p) => p.replace(/^\/__binance_fapi/, ""),
        },
      },
    },
  };
});
