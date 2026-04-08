import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev proxy: forwards /api and /oauth to the local qtss-api on :8080
// so the browser can call the v2 endpoints without CORS gymnastics.
// Override the target via VITE_API_URL when running against a remote API.
const apiTarget = process.env.VITE_API_URL ?? "http://localhost:8080";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    proxy: {
      "/api": { target: apiTarget, changeOrigin: true },
      "/oauth": { target: apiTarget, changeOrigin: true },
    },
  },
  build: {
    outDir: "dist",
    sourcemap: true,
  },
});
