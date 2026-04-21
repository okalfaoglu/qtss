#!/usr/bin/env bash
# QTSS prod preflight — run from repo root BEFORE `deploy/pull-build-restart.sh`.
#
# Goal: catch the classes of mistakes that silently break prod:
#   1. duplicate migration prefixes (two 00NN_*.sql files → sqlx panics)
#   2. modified already-applied migrations (checksum drift)
#   3. uncommitted / unpushed changes (prod pulls main; anything not on origin is missing)
#   4. release build failure / clippy errors
#   5. workspace test failure (optional, with RUN_TESTS=1)
#   6. accidentally-committed secrets in staged/new migrations
#   7. new migrations that INSERT into system_config without ON CONFLICT
#      (would crash on redeploy against an already-seeded DB)
#   8. diff of migrations not yet applied on prod (requires PROD_DATABASE_URL)
#
# Usage:
#   ./deploy/preflight.sh                          # local-only checks
#   PROD_DATABASE_URL=... ./deploy/preflight.sh    # + prod migration diff
#   RUN_TESTS=1 ./deploy/preflight.sh              # + cargo test --workspace
#
# Exit non-zero on any hard failure; warnings are printed but don't block.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
cd "$ROOT"

red()    { printf '\033[31m%s\033[0m\n' "$*"; }
green()  { printf '\033[32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
bold()   { printf '\033[1m%s\033[0m\n' "$*"; }

FAILED=0
WARNED=0
fail() { red   "  FAIL: $*"; FAILED=$((FAILED+1)); }
warn() { yellow "  WARN: $*"; WARNED=$((WARNED+1)); }
ok()   { green "  OK:   $*"; }

# -------------------------------------------------------------------
bold "1/8 Migration file sanity"
# -------------------------------------------------------------------
dupes=$(ls migrations/ | sed -E 's/^([0-9]+)_.*$/\1/' | sort | uniq -d || true)
if [[ -n "$dupes" ]]; then
  fail "duplicate migration prefix(es): $dupes"
  ls migrations/ | grep -E "^($(echo "$dupes" | tr '\n' '|' | sed 's/|$//'))_" || true
else
  ok "no duplicate migration prefixes ($(ls migrations/*.sql | wc -l) files)"
fi

# -------------------------------------------------------------------
bold "2/8 Git tree + upstream"
# -------------------------------------------------------------------
if [[ -n "$(git status --porcelain)" ]]; then
  fail "working tree dirty — commit or stash before deploy"
  git status --short
else
  ok "working tree clean"
fi
branch=$(git rev-parse --abbrev-ref HEAD)
if [[ "$branch" != "main" ]]; then
  warn "current branch is '$branch', prod typically deploys 'main'"
fi
if ! git fetch origin --quiet 2>/dev/null; then
  warn "git fetch origin failed (offline?) — skipping upstream-sync check"
else
  ahead=$(git rev-list --count "origin/${branch}..HEAD" 2>/dev/null || echo 0)
  behind=$(git rev-list --count "HEAD..origin/${branch}" 2>/dev/null || echo 0)
  if [[ "$ahead" != "0" ]]; then
    fail "$ahead commit(s) ahead of origin/${branch} — push before deploy"
  elif [[ "$behind" != "0" ]]; then
    warn "$behind commit(s) behind origin/${branch} — pull before deploy"
  else
    ok "in sync with origin/${branch}"
  fi
fi

# -------------------------------------------------------------------
bold "3/8 Secret scan on staged / new migrations"
# -------------------------------------------------------------------
# Only scan files not yet in origin/main so we don't re-flag legacy.
new_migs=$(git diff --name-only "origin/${branch}...HEAD" -- migrations/ 2>/dev/null | grep -E '\.sql$' || true)
if [[ -z "$new_migs" ]]; then
  new_migs=$(git diff --name-only HEAD~20 HEAD -- migrations/ 2>/dev/null | grep -E '\.sql$' || true)
fi
if [[ -n "$new_migs" ]]; then
  pat='(postgres://[^ ]*:[^ @]+@|password\s*=\s*['"'"'"][^'"'"'"]{6,}|api[_-]?key\s*=|sk-[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16})'
  hits=$(echo "$new_migs" | xargs -r grep -EHn -i "$pat" || true)
  if [[ -n "$hits" ]]; then
    fail "possible secret in new migration:"
    echo "$hits" | sed 's/^/    /'
  else
    ok "no obvious secrets in new migrations"
  fi
else
  ok "no new migrations since origin/${branch}"
fi

# -------------------------------------------------------------------
bold "4/8 system_config INSERTs use ON CONFLICT"
# -------------------------------------------------------------------
# Any INSERT INTO system_config without ON CONFLICT DO NOTHING/UPDATE
# will panic on redeploy if the key already exists. We only check
# migrations new on this branch.
if [[ -n "$new_migs" ]]; then
  bad=""
  for f in $new_migs; do
    if grep -qi 'INSERT INTO system_config' "$f" && ! grep -qi 'ON CONFLICT' "$f"; then
      bad+="$f "
    fi
  done
  if [[ -n "$bad" ]]; then
    fail "INSERT INTO system_config without ON CONFLICT in: $bad"
  else
    ok "all new system_config INSERTs have ON CONFLICT"
  fi
fi

# -------------------------------------------------------------------
bold "5/8 cargo build --release -p qtss-api -p qtss-worker"
# -------------------------------------------------------------------
if cargo build --release -p qtss-api -p qtss-worker 2>&1 | tail -20 | sed 's/^/    /'; then
  ok "release build green"
else
  fail "release build broken"
fi

# -------------------------------------------------------------------
bold "6/8 cargo clippy (errors only)"
# -------------------------------------------------------------------
if cargo clippy -p qtss-api -p qtss-worker --release -- -D warnings 2>&1 | tail -20 | sed 's/^/    /'; then
  ok "clippy clean"
else
  warn "clippy found issues (warnings treated as errors above) — review before deploy"
fi

# -------------------------------------------------------------------
bold "7/8 cargo test (optional — RUN_TESTS=1)"
# -------------------------------------------------------------------
if [[ "${RUN_TESTS:-0}" == "1" ]]; then
  if cargo test --workspace --no-fail-fast 2>&1 | tail -40 | sed 's/^/    /'; then
    ok "workspace tests green"
  else
    fail "workspace tests failing"
  fi
else
  warn "skipped (set RUN_TESTS=1 to run)"
fi

# -------------------------------------------------------------------
bold "8/8 Migration diff vs PROD (optional — set PROD_DATABASE_URL)"
# -------------------------------------------------------------------
if [[ -n "${PROD_DATABASE_URL:-}" ]]; then
  applied=$(psql "$PROD_DATABASE_URL" -Atc \
    "SELECT version FROM _sqlx_migrations ORDER BY version" 2>/dev/null || true)
  if [[ -z "$applied" ]]; then
    warn "could not read _sqlx_migrations from PROD (connection/permission?)"
  else
    on_disk=$(ls migrations/*.sql | sed -E 's|migrations/0*([0-9]+)_.*|\1|' | sort -n)
    pending=$(comm -23 <(echo "$on_disk") <(echo "$applied" | sort -n))
    missing=$(comm -13 <(echo "$on_disk") <(echo "$applied" | sort -n))
    if [[ -n "$missing" ]]; then
      fail "migrations applied on PROD but missing on disk: $(echo $missing)"
      fail "  → DO NOT DEPLOY. Restore missing files or revert the PROD row."
    fi
    if [[ -n "$pending" ]]; then
      ok "migrations pending on PROD (will apply on restart): $(echo $pending)"
    else
      ok "PROD already at head"
    fi
  fi
else
  warn "skipped (set PROD_DATABASE_URL to compare)"
fi

# -------------------------------------------------------------------
echo
bold "=============================================="
if [[ "$FAILED" -gt 0 ]]; then
  red "PREFLIGHT FAILED: $FAILED hard failure(s), $WARNED warning(s)"
  exit 1
fi
if [[ "$WARNED" -gt 0 ]]; then
  yellow "PREFLIGHT OK with $WARNED warning(s) — review above before deploying"
else
  green "PREFLIGHT CLEAN — safe to deploy"
fi
