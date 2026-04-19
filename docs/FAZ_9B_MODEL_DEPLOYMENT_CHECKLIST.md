# Model Deployment Checklist

**Faz 9B** — `active=true` flip öncesi zorunlu QA adımları.

Bir modeli canlıya almak (active=true → inference sidecar yüklüyor → setup engine skor kullanıyor) geri döndürülmesi hızlı ama sonucu etkili bir karardır. Her adım tick atılmadan aktivasyon yapılmaz.

---

## Aşama 1 — Training artifact sağlığı

- [ ] `qtss_ml_training_runs.status = 'success'` (training hata almadan bitmiş)
- [ ] `qtss_models.artifact_path` dosyası diskte var (`ls -lh $PATH`)
- [ ] `qtss_models.artifact_sha256` dosya SHA-256 ile eşleşiyor (`sha256sum $PATH`)
- [ ] `$PATH.meta.json` yanında duruyor; `feature_names`, `training_distributions`, `metrics` dolu
- [ ] `metrics_json->>'auc'` **≥ 0.55** (random'dan istatistiksel anlamlı iyi; AUC=0.50 coin flip)

---

## Aşama 2 — Eğitim seti temsil gücü

- [ ] `n_train` ≥ `trainer.min_rows` (default 500)
- [ ] `n_valid` ≥ `n_train × validation_fraction × 0.8` (time-split sızıntısı olmadı)
- [ ] `feature_coverage_pct` ≥ `trainer.min_feature_coverage_pct` (default 0.60)
- [ ] `label_balance` ≥ `trainer.min_label_balance` (default 0.15; minority class yeterli)
- [ ] Training set sembol çeşitliliği: minimum **10** farklı symbol
  ```sql
  SELECT COUNT(DISTINCT symbol) FROM v_qtss_training_set_closed;
  ```
- [ ] Training set timeframe çeşitliliği: canlıda hangi TF'ler varsa hepsi temsilde
- [ ] `mode` kırılımı (live + backtest): eğer sadece backtest varsa "out-of-distribution" riski not edilmeli

---

## Aşama 3 — Metrik karşılaştırma (varsa önceki model)

- [ ] Yeni AUC ≥ eski AUC + `trainer.auto_activate_min_auc_lift` (default 0.01)
  - Değilse: **shadow mode** olarak kayda al, manuel review
- [ ] Yeni PR-AUC ≥ eski PR-AUC × 0.95 (regresyon toleransı %5)
- [ ] `log_loss` azalmış veya eşit
- [ ] **Calibration check**: `predicted_prob` dilimleri gerçekleşen win-rate ile uyumlu
  ```sql
  -- Yeni modelin predict'leri ile gerçek outcome'lar arasındaki ilişki
  SELECT FLOOR(score * 10) / 10 AS bucket,
         COUNT(*),
         AVG(CASE WHEN label='win' THEN 1.0 ELSE 0.0 END) AS realized_win_rate
  FROM qtss_ml_predictions p JOIN v_qtss_training_set_closed s USING (setup_id)
  WHERE p.model_version = '<NEW>' GROUP BY 1 ORDER BY 1;
  ```
  Her bucket'ta `realized_win_rate ≈ bucket` olmalı (iyi calibrate model diagonal).

---

## Aşama 4 — Feature leakage kontrolü

- [ ] Feature names listesi incelendi; hiçbir feature `outcome_*`, `realized_*`, `closed_at` gibi future-dependent sinyal taşımıyor
  ```bash
  # qtss_models.feature_names'den görece kısa listeyi gözden geçir:
  psql -c "SELECT jsonb_pretty(feature_names) FROM qtss_models WHERE id='<NEW>';"
  ```
- [ ] SHAP top-10 kontrolü: en etkili feature'lar iş mantığıyla tutarlı mı?
  ```bash
  # Örnek bir setup üzerinde:
  curl -X POST http://127.0.0.1:8790/explain -d @sample_features.json
  ```
  Eğer `candle.spinning_top` %80 contribution veriyorsa şüphelen (muhtemelen leakage veya imbalance).

---

## Aşama 5 — Shadow canlı doğrulama (opsiyonel ama şiddetle tavsiye)

Model `active=false` ile yüklenir, setup engine kararı **etkilemez** ama skor `qtss_ml_predictions`'a yazılır:

- [ ] En az **24 saat** shadow trafiği gözlendi
- [ ] `n_predictions` ≥ 100
- [ ] `decision_distribution` sağlıklı (çoğu "shadow_pass"/"shadow_block" olmalı, tümü tek kola çakılmamalı)
- [ ] Shadow predictions'ın gerçekleşen outcome'larla AUC'si training AUC'den max **%10** düşük
  (daha fazla düşüş = overfit veya concept drift)

---

## Aşama 6 — Drift guards canlı

- [ ] `qtss_ml_drift_snapshots` son 24 saatte yazılmış (scheduler çalışıyor)
- [ ] Aktif herhangi bir `qtss_ml_breaker_events.resolved_at IS NULL` kaydı yok
- [ ] `drift.breaker_action = 'deactivate'` (production default)
- [ ] `inference.fail_open = false` (production default)

---

## Aşama 7 — Rollback hazır

- [ ] Önceki aktif model versiyonu biliniyor, not edildi
- [ ] Rollback komutu denendi (canary symbolda test):
  ```bash
  qtss-trainer activate setup_meta <PREV>
  curl -X POST http://127.0.0.1:8790/reload
  # setup üretimi 1-2 dk içinde önceki davranışa dönmeli
  ```
- [ ] Monitoring alert'leri canlı (AUC düşüşü, breaker trip, ingestion fail)

---

## Aşama 8 — Activation

Hepsi yeşilse:

```bash
# Aktivasyon
qtss-trainer activate setup_meta 2026.04.20-103000

# Sidecar yeni booster'ı yüklesin
curl -X POST http://127.0.0.1:8790/reload

# Health check
curl http://127.0.0.1:8790/active | jq .
# → n_features, metrics, model_version doğru dönüyor olmalı

# Audit kaydı
psql -c "UPDATE qtss_ml_training_runs SET notes = COALESCE(notes,'') || ' | activated <YYYY-MM-DD> by <operator>' WHERE model_id='<NEW>';"
```

---

## Aşama 9 — Post-activation monitor (ilk 48 saat)

- [ ] **T+1h**: `/v2/ml/predictions/summary` — prediction rate beklendiği gibi mi?
- [ ] **T+6h**: Setup üretim hızı regresyon yok mu? (eski modelin %80'inin altına düşmedi)
- [ ] **T+24h**: PSI warning/critical var mı? Drift runbook'a geçilmesi gerekiyor mu?
- [ ] **T+48h**: Gerçekleşen outcome'larla live precision@0.5 hesapla; training AUC ile karşılaştır

Regresyon varsa → **rollback** (Aşama 7).

---

## 10. Hızlı referans — nerede hangi sorgu?

| Ne | Sorgu |
|---|---|
| Training başarılı mı? | `SELECT status, notes FROM qtss_ml_training_runs WHERE model_id='<ID>';` |
| Model metrikleri | `SELECT metrics_json FROM qtss_models WHERE id='<ID>';` |
| Aktif model | `SELECT model_family, model_version, metrics_json->>'auc' FROM qtss_models WHERE active=true;` |
| Son 24h prediction sayısı | `SELECT COUNT(*) FROM qtss_ml_predictions WHERE created_at > now() - '24h'::interval;` |
| Live calibration | Yukarıdaki Aşama 3 calibration sorgusu |
| Open breaker events | `SELECT * FROM qtss_ml_breaker_events WHERE resolved_at IS NULL;` |

---

## 11. İlişkili dokümanlar

- [Retraining playbook](FAZ_9B_RETRAINING_PLAYBOOK.md)
- [Drift response runbook](FAZ_9B_DRIFT_RUNBOOK.md)
- [Historical backfill spec](FAZ_9B_HISTORICAL_BACKFILL.md)
