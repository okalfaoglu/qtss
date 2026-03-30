#!/usr/bin/env node
/**
 * Wrapper: run from repo root (`node scripts/check-i18n-keys.mjs`).
 * Canonical implementation: `web/scripts/check-i18n-keys.mjs`.
 */
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = dirname(scriptsDir);
const target = join(repoRoot, "web", "scripts", "check-i18n-keys.mjs");
const r = spawnSync(process.execPath, [target], { stdio: "inherit" });
process.exit(r.status === null ? 1 : r.status);
