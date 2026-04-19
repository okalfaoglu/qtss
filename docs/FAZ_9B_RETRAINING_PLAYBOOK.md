# Retraining Playbook

**Faz 9B** — Ne zaman, ne tetikler, kim karar verir?

Bu runbook `qtss-worker`'ın içindeki `ml_retraining_task` scheduler'ı ile manuel müdahaleler arasındaki iş bölümünü tanımlar. Tüm eşikler `system_config.ai.retraining.*` altında (migration 0169); değişiklik deploy gerektirmez.

---

## 1. Tetikleyiciler

| # | Tetik | Tür | Eşik | Action |
|---|---|---|---|---|
| T1 | Yeni veri birikmesi | **otomatik** | `retraining.trigger_min_new_closed` (default **50** yeni closed setup) | scheduler `qtss-trainer train --activate-if-better` spawn eder |
| T2 | Model yaşı | **otomatik** | `retraining.trigger_age_hours` (default **168h = 7 gün**) | scheduler retrain tetikler (veri azsa skip) |
| T3 | PSI drift critical | **yarı-otomatik** | drift breaker `deactivate` action'ı sonrası | breaker kapandıktan sonra retrain zorunlu (drift runbook step 5) |
| T4 | Outcome milestone | **otomatik** | her 1000 yeni closed setup'ta bir checkpoint | T1 ile aynı flow, farklı trigger_source |
| T5 | Manuel | **operator** | `qtss-trainer train --notes "…"` | CLI doğrudan, audit `qtss_ml_training_runs.trigger_source='manual'` |
| T6 | Historical backfill bitince | **otomatik** | `backfill.enabled=false` → true → false cycle sonunda | yeni veri = ilk eğitim (bootstrap) |

---

## 2. Karar akışı (scheduler her `cron_interval_secs` saniyede bir kontrol)

```
cron tick
  ├─ active_model = SELECT active=true FROM qtss_models
  ├─ n_new = closed_setups WHERE closed_at > active_model.trained_at
  ├─ age_h = NOW() - active_model.trained_at
  ├─ IF no active_model AND closed_setups ≥ min_rows → trigger="bootstrap" T6
  ├─ IF n_new ≥ trigger_min_new_closed → trigger="outcome_milestone" T4
  ├─ IF age_h ≥ trigger_age_hours      → trigger="cron" T2
  ├─ ELSE skip (log debug)
  └─ spawn trainer, insert qtss_ml_training_runs row (status='running')
```

---

## 3. Aktivasyon politikası

Yeni model eğitildiğinde:

| Koşul | Action |
|---|---|
| `auto_activate_if_better=true` **ve** new.AUC > old.AUC + `auto_activate_min_auc_lift` | `active=true` otomatik flip; eski model `active=false` |
| yukarıdaki değilse | `active=false` (shadow mode); manuel review gerek |
| ilk model (hiç active yok) ve AUC ≥ 0.55 | `active=true` bootstrap |
| AUC < 0.55 | `active=false` + `status='failed'` (random'dan iyi değil) |

**Shadow mode**: model kayıtlı ama inference sidecar yüklemiyor; GUI'de "shadow" etiketiyle gösterilir. Operator `qtss-trainer activate FAMILY VERSION` ile manuel aktif eder.

---

## 4. Operator kararları (ne zaman müdahale)

| Senaryo | Otomatik akış | Operator ne yapmalı |
|---|---|---|
| AUC düşerek aktif kalıyor | auto_activate_min_auc_lift pozitif olduğu için düşme aktive etmez | Eğer eski model 7 gün sonra hâlâ aktifse, **drift runbook**'a geç |
| 3 retrain üst üste `skipped_low_coverage` | scheduler denemeye devam eder | `qtss_features_snapshot` ingestion'ı bozuk — feature store sağlığını kontrol et (`/v2/config?limit=500` → `feature_store.enabled`) |
| AUC > 0.75 ama live precision düşük | active kalır | **Deployment checklist**'e geri dön; feature leakage var mı? |
| Backfill yarıda kaldı | scheduler backfill'den bağımsız | `qtss-worker backfill-training --resume` (bkz. historical backfill spec) |

---

## 5. SLO'lar

- **Retraining gecikmesi**: tetikleyici → `qtss_ml_training_runs.status='success'` arası **≤ 15 dk** (500 örnek için LightGBM eğitimi)
- **Aktivasyon gecikmesi**: `status='success'` → inference sidecar reload **≤ 60 sn** (`POST /reload`)
- **Rollback süresi**: kötü model tespit → eski model active **≤ 5 dk** (`qtss-trainer activate setup_meta <prev-version>`)

---

## 6. Manuel operasyon (CLI)

```bash
# Bootstrap training (ilk model)
qtss-trainer stats                         # veri yeterli mi?
qtss-trainer train --activate --notes "bootstrap Faz 9B"

# Model listesi + aktif durum
qtss-trainer list

# Belirli bir versiyona dön (rollback)
qtss-trainer activate setup_meta 2026.04.20-103000

# Shadow model'i canlıya al
qtss-trainer activate setup_meta 2026.04.25-071500
curl -X POST http://127.0.0.1:8790/reload   # sidecar'ı yenile
```

---

## 7. Audit

Her training job bir satır `qtss_ml_training_runs`:

```sql
SELECT started_at, finished_at - started_at AS duration,
       trigger_source, status, n_closed_setups, feature_coverage_pct,
       label_balance, notes
FROM qtss_ml_training_runs
ORDER BY started_at DESC LIMIT 20;
```

GUI: **Training Set Monitor** sayfası bu tabloyu paginate eder (Faz 9B ikinci dalga).

---

## 8. İlişkili dokümanlar

- [Drift response runbook](FAZ_9B_DRIFT_RUNBOOK.md)
- [Deployment checklist](FAZ_9B_MODEL_DEPLOYMENT_CHECKLIST.md)
- [Historical backfill spec](FAZ_9B_HISTORICAL_BACKFILL.md)
- [FAZ_9_AI_CONFLUENCE.md](FAZ_9_AI_CONFLUENCE.md) — üst mimari
