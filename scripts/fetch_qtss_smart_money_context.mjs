#!/usr/bin/env node
/**
 * Fetches analysis endpoints useful for Nansen / smart-money LLM playbooks.
 *
 * Env:
 *   QTSS_API_BASE      default http://127.0.0.1:8080
 *   QTSS_BEARER_TOKEN  required (JWT)
 */
const base = (process.env.QTSS_API_BASE || "http://127.0.0.1:8080").replace(/\/$/, "");
const token = process.env.QTSS_BEARER_TOKEN || "";
if (!token.trim()) {
  console.error("QTSS_BEARER_TOKEN is required");
  process.exit(1);
}

const headers = {
  Accept: "application/json",
  Authorization: `Bearer ${token.trim()}`,
};

async function getJson(path) {
  const url = `${base}${path}`;
  const res = await fetch(url, { headers });
  const text = await res.text();
  let body;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = { _parse_error: true, raw: text.slice(0, 2000) };
  }
  return {
    path,
    status: res.status,
    ok: res.ok,
    body,
  };
}

const paths = [
  "/api/v1/analysis/data-snapshots",
  "/api/v1/analysis/engine/confluence/latest",
  "/api/v1/analysis/nansen/snapshot",
  "/api/v1/analysis/nansen/setups/latest",
  "/api/v1/analysis/market-context/summary?limit=200&enabled_only=true",
  "/api/v1/analysis/intake-playbook/recent?limit=30",
  "/api/v1/analysis/intake-playbook/latest?playbook_id=market_mode",
];

const results = await Promise.all(paths.map((p) => getJson(p)));

const out = {
  fetched_at: new Date().toISOString(),
  qtss_api_base: base,
  endpoints: results,
};

process.stdout.write(`${JSON.stringify(out, null, 2)}\n`);
