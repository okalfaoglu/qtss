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

export default defineConfig(({ mode }) => {
  const binanceProxyTarget = binanceProxyTargetFromEnv(mode);
  return {
    plugins: [react()],
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
      },
    },
  };
});
