# Faz 9B Prod Deployment Guide

**Hedef:** AI training + inference sidecar + paper trading stack'inin süreklilik altına alınması. WSL/laptop session'ları yerine systemd + restart policy.

## Ön koşullar

- [ ] Migration 0169 + 0170 uygulanmış (`psql ... -f migrations/0169_*.sql 0170_*.sql`)
- [ ] Trainer venv hazır: `/app/qtss/trainer/.venv/bin/qtss-trainer` ve `qtss-inference` çalıştırılabilir
- [ ] `pipx ensurepath` (opsiyonel — global erişim için)
- [ ] `worker.release` binary: `cargo build --release -p qtss-worker`
- [ ] `.env` içinde `DATABASE_URL` dolu

## 1. Trainer binary_path config'i

Cron subprocess venv binary'sini bulabilsin:

```sql
UPDATE system_config
   SET value = '"/app/qtss/trainer/.venv/bin/qtss-trainer"'::jsonb
 WHERE module='ai' AND config_key='retraining.binary_path';
```

## 2. Inference sidecar systemd unit

`/etc/systemd/system/qtss-inference.service`:

```ini
[Unit]
Description=QTSS LightGBM Inference Sidecar (Faz 9B)
After=network.target postgresql.service

[Service]
Type=simple
User=root
WorkingDirectory=/app/qtss/trainer
Environment="QTSS_DATABASE_URL=postgres://qtss:PASS@127.0.0.1:5432/qtss"
ExecStart=/app/qtss/trainer/.venv/bin/qtss-inference
Restart=on-failure
RestartSec=5s
StandardOutput=append:/var/log/qtss/inference.log
StandardError=append:/var/log/qtss/inference.log

[Install]
WantedBy=multi-user.target
```

Aktivasyon:
```bash
mkdir -p /var/log/qtss
systemctl daemon-reload
systemctl enable --now qtss-inference
systemctl status qtss-inference
curl http://127.0.0.1:8790/health | jq .
```

## 3. Worker systemd unit

`/etc/systemd/system/qtss-worker.service`:

```ini
[Unit]
Description=QTSS Rust Worker (Faz 9B)
After=network.target postgresql.service qtss-inference.service
Requires=qtss-inference.service

[Service]
Type=simple
User=root
WorkingDirectory=/app/qtss
EnvironmentFile=/app/qtss/.env
ExecStart=/app/qtss/target/release/qtss-worker
Restart=on-failure
RestartSec=10s
StandardOutput=append:/var/log/qtss/worker.log
StandardError=append:/var/log/qtss/worker.log

[Install]
WantedBy=multi-user.target
```

Aktivasyon:
```bash
systemctl daemon-reload
systemctl enable --now qtss-worker
journalctl -u qtss-worker -f | grep -iE "trainer|drift|backfill|sidecar"
```

## 4. Paper trading açma checklist'i

Paper mode = gerçek market data + simüle emir + paper ledger'a PnL kaydı.

- [ ] Worker config'te paper UUID'leri dolu (`worker.paper_org_id`, `worker.paper_user_id`)
- [ ] `worker.paper_ledger_enabled=true`
- [ ] `worker.ai_tactical_executor_dry=true` (canlıda false)
- [ ] `worker.position_manager_dry_close_enabled=true`
- [ ] `worker.ai_tactical_executor_live=false` (prod hazırlığı tamamlanana dek)
- [ ] `worker.position_manager_live_close_enabled=false`

Üretilen dry setupları training setine otomatik düşüyor (`v_qtss_training_set_closed` mode-agnostik filtre).

## 5. Canlı geçiş öncesi son kontroller

| Kontrol | SQL / komut |
|---|---|
| Aktif model var | `SELECT model_version, (metrics_json->>'auc')::float FROM qtss_models WHERE active=true;` |
| Sidecar sağlıklı | `curl -s http://127.0.0.1:8790/active \| jq .` |
| Son eğitim başarılı | `SELECT status, finished_at FROM qtss_ml_training_runs ORDER BY started_at DESC LIMIT 1;` |
| PSI breaker kapalı | `SELECT COUNT(*) FROM qtss_ml_breaker_events WHERE resolved_at IS NULL;` (=0) |
| Drift snapshot son 24h | `SELECT COUNT(*) FROM qtss_ml_drift_snapshots WHERE computed_at > now()-'24h'::interval;` |
| Paper ledger kayıt yazıyor | `SELECT COUNT(*) FROM paper_ledger_fills WHERE created_at > now()-'1h'::interval;` |

Hepsi yeşilse `ai_tactical_executor_live=true` flip'i deployment checklist Aşama 8'e göre yapılır.

## 6. Izleme

```bash
# Worker sağlık:
journalctl -u qtss-worker --since="10 min ago" | grep -iE "ERROR|WARN"

# Sidecar sağlık:
journalctl -u qtss-inference --since="10 min ago" | grep -iE "ERROR|WARN|traceback"

# Pipeline sağlık özeti:
psql $DATABASE_URL <<SQL
SELECT
  (SELECT COUNT(*) FROM qtss_setups WHERE created_at > now()-'1h'::interval) AS setups_1h,
  (SELECT COUNT(*) FROM qtss_ml_predictions WHERE inference_ts > now()-'1h'::interval) AS predictions_1h,
  (SELECT COUNT(*) FROM qtss_ml_drift_snapshots WHERE computed_at > now()-'1h'::interval) AS drift_1h,
  (SELECT COUNT(*) FROM qtss_ml_breaker_events WHERE resolved_at IS NULL) AS open_breakers;
SQL
```

## 7. Rollback

AI katmanını tümden devre dışı bırakmak:

```sql
UPDATE system_config SET value='false'::jsonb
 WHERE module='ai' AND config_key IN (
   'trainer.enabled','inference.enabled','drift.enabled','trainer.cron.enabled'
 );
```

Worker veya sidecar'ı durdur:
```bash
systemctl stop qtss-worker qtss-inference
```

Setup engine AI olmadan devam eder çünkü `inference.fail_open=false` default'u setup üretmeyi bloklar — yerine `true` çekmek gerekirse `UPDATE ... 'true'::jsonb WHERE config_key='inference.fail_open';`.

## 8. İlişkili dokümanlar

- [Retraining playbook](FAZ_9B_RETRAINING_PLAYBOOK.md)
- [Drift runbook](FAZ_9B_DRIFT_RUNBOOK.md)
- [Deployment checklist (model flip)](FAZ_9B_MODEL_DEPLOYMENT_CHECKLIST.md)
- [Historical backfill](FAZ_9B_HISTORICAL_BACKFILL.md)
