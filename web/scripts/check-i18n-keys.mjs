#!/usr/bin/env node
/**
 * Ensures en.json and tr.json define the same nested key paths (FAZ 9.6).
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const localesDir = join(__dirname, "..", "src", "locales");

function collectKeys(obj, prefix = "") {
  /** @type {string[]} */
  const out = [];
  if (obj === null || typeof obj !== "object" || Array.isArray(obj)) {
    return out;
  }
  for (const k of Object.keys(obj).sort()) {
    const p = prefix ? `${prefix}.${k}` : k;
    const v = obj[k];
    if (v !== null && typeof v === "object" && !Array.isArray(v)) {
      out.push(...collectKeys(v, p));
    } else {
      out.push(p);
    }
  }
  return out;
}

const en = JSON.parse(readFileSync(join(localesDir, "en.json"), "utf8"));
const tr = JSON.parse(readFileSync(join(localesDir, "tr.json"), "utf8"));
const enKeys = new Set(collectKeys(en));
const trKeys = new Set(collectKeys(tr));

const onlyEn = [...enKeys].filter((k) => !trKeys.has(k)).sort();
const onlyTr = [...trKeys].filter((k) => !enKeys.has(k)).sort();

if (onlyEn.length || onlyTr.length) {
  console.error("i18n key mismatch:");
  if (onlyEn.length) console.error("  only in en:", onlyEn.join(", "));
  if (onlyTr.length) console.error("  only in tr:", onlyTr.join(", "));
  process.exit(1);
}
console.log("i18n keys OK:", enKeys.size, "paths");
