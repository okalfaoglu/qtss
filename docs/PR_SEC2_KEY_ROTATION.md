# PR-SEC2 — API Key Rotation Runbook

End-to-end procedure for migrating a plaintext `system_config` API key
into the encrypted `secrets_vault`. After the migration, the old
plaintext row should be wiped from `system_config` so a `pg_dump`
cannot leak it.

## 0. One-time bootstrap (first rotation only)

```bash
# 1. Generate a fresh 32-byte KEK (master key).
KEK_HEX=$(openssl rand -hex 32)
echo "QTSS_SECRET_KEK_V1=$KEK_HEX"

# 2. Add it to the worker + api systemd environments. This survives
#    reboots and is NEVER committed to git or the DB.
sudo systemctl edit qtss-worker   # add [Service]\nEnvironment="QTSS_SECRET_KEK_V1=..."
sudo systemctl edit qtss-api      # same
sudo systemctl daemon-reload
```

The KEK lives outside Postgres. A DB dump alone is useless without it.
For production, swap the `StaticKek` provider for a KMS-backed one
(see `qtss-secrets/src/kek.rs`) so rotation can happen without redeploy.

## 1. Migrate one secret at a time

Example: rotate the Anthropic API key.

```bash
# Export DB + KEK for the CLI.
export DATABASE_URL="postgres://qtss:PASS@127.0.0.1:5432/qtss"
export QTSS_SECRET_KEK_V1="$KEK_HEX"
export QTSS_SECRET_KEK_VERSION=1

# Put the new key into the vault (stdin avoids shell history).
echo "sk-ant-api03-REAL-NEW-KEY..." | \
  /app/qtss/target/release/qtss-secret-cli \
    put anthropic_api_key --from-stdin \
    --description "Rotated 2026-04-24"

# Confirm it landed (metadata only — plaintext never printed).
/app/qtss/target/release/qtss-secret-cli list
```

## 2. Restart the worker so `qtss-ai::provider_secrets` picks it up

```bash
sudo systemctl restart qtss-worker
# Watch for vault hits in the audit log:
psql -c "SELECT actor, secret_name, outcome, reason, occurred_at
           FROM secret_access_log
          ORDER BY occurred_at DESC LIMIT 10;"
# Expected: outcome='hit', actor='qtss-ai', reason='ai.provider_secrets.load'
```

A successful vault read logs `outcome='hit'`. A fallback read logs
`outcome='miss_fallback_config'` — that means the vault didn't have
the key and the loader fell back to `system_config`. Keep rotating
until all audit rows show `hit`.

## 3. Wipe the plaintext from `system_config`

Only after step 2 shows `outcome='hit'` for this secret.

```sql
BEGIN;
DELETE FROM system_config
 WHERE module = 'ai' AND config_key = 'anthropic_api_key';
-- Repeat verification: the next worker tick should continue serving
-- traffic (vault still has the key); a fallback now means something
-- is wrong, check secret_access_log.
COMMIT;
```

## 4. Lock fallback off (optional, recommended after bulk rotation)

```sql
UPDATE system_config
   SET value = '{"enabled": false}'::jsonb
 WHERE module = 'secrets'
   AND config_key = 'allow_config_fallback';
```

With fallback off, any vault miss returns `SecretError::NotFound`. This
is the "vault-only" mode — safer once every consumer has been migrated,
but leave it on during the rotation window so a mis-named secret does
not take the worker down.

## 5. KEK rotation (periodic, e.g. quarterly)

1. Generate `QTSS_SECRET_KEK_V2` and add it alongside V1 in systemd env.
2. Bump `system_config.secrets.kek_version` to 2.
3. For each row: read with V1, re-wrap with V2 (`PgSecretStore::put` re-
   encrypts under the new KEK — a dedicated `rotate` subcommand is the
   next CLI addition).
4. Remove `QTSS_SECRET_KEK_V1` from systemd env once all rows show
   `kek_version = 2` in the audit log.

## Migrated consumers (PR-SEC2 scope)

| Secret | Consumer | Status |
|---|---|---|
| `anthropic_api_key` | `qtss-ai::AiProviderSecrets::load` | ✅ vault-first |
| `gemini_api_key` (ai module) | `qtss-ai::AiProviderSecrets::load` | ✅ vault-first |
| `onprem_api_key` | `qtss-ai::AiProviderSecrets::load` | ✅ vault-first |
| `gemini_api_key` (telegram module) | `qtss-telegram-setup-analysis` | ⚠️ still plaintext |
| `telegram_bot_token` | `qtss-notify` | ⚠️ still plaintext |
| `webhook_secret` | telegram webhook | ⚠️ still plaintext |
| `binance_*_api_key` / `_api_secret` | `exchange_accounts` table | ⚠️ separate store |
| `jwt_secret`, `metrics_token` | API bootstrap | ⚠️ read at startup |

The remaining consumers will land in follow-up PRs — the vault + reader
contract stays the same, only new call sites to migrate.

## Panic buttons

* **Lost KEK**: every vault row is unreadable. Either restore the KEK
  from backup (keep it offline, e.g. in a vaulted password manager) or
  truncate `secrets_vault` and re-put every secret. The system_config
  fallback rows are the safety net — do *not* delete them until the
  rotation is complete.
* **Leaked plaintext detected**: rotate at the provider, put the new
  key via the CLI, restart the worker, wipe the old `system_config`
  row. Total downtime: ~30 seconds.
* **Cannot read vault (audit shows `outcome='error'`)**: corruption or
  wrong KEK version. Check the worker logs for the inner crypto error
  message; until resolved the fallback keeps the system alive as long
  as `allow_config_fallback = true`.
