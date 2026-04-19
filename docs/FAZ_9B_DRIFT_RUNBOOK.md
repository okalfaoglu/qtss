# Drift Response Runbook

**Faz 9B** — PSI (Population Stability Index) kritik olduğunda operator akışı.

Tüm eşikler `system_config.ai.drift.*` altında (migration 0169).

---

## 1. PSI ne ölçer?

Her feature için **live distribution** (son `drift.psi_lookback_hours` saatlik inference çağrıları) vs **training distribution** (aktif modelin `.meta.json`'u) Population Stability Index:

```
PSI = Σᵢ (live_pctᵢ − train_pctᵢ) × ln(live_pctᵢ / train_pctᵢ)
```

| PSI | Anlam | Otomatik aksiyon |
|---|---|---|
| < 0.10 | Stable — feature dağılımı eğitim set'iyle uyumlu | yok |
| 0.10 – 0.25 | **Warning** — ılımlı drift | GUI banner + `qtss_ml_drift_snapshots` log |
| ≥ 0.25 | **Critical** — distribution farkı eğitim geçerliliğini tehdit ediyor | breaker tetikleyici adayı |

Breaker tetiklenir: **`drift.critical_features_for_trip`** adet feature aynı anda critical'e düştüğünde (default **3**, tek outlier noise değil).

Aksiyon (`drift.breaker_action`):
- `deactivate` (default) → `qtss_models.active = false`, inference sidecar /reload → 503
- `alert_only` → sadece `qtss_ml_breaker_events` + outbox bildirimi
- `throttle` → setup engine AI skoru shadow olarak alır, fail_open davranışı

---

## 2. Tetik akışı (otomatik)

```
drift_check_task (every drift.check_interval_secs, default 30 dk)
  ├─ active_model = qtss_models WHERE active=true
  ├─ train_dist = active_model.meta_json.training_distributions
  ├─ live_dist = qtss_ml_predictions WHERE computed_at > now() - lookback_hours
  ├─ FOR EACH feature IN feature_names:
  │    psi = compute_psi(train_dist[f], live_dist[f])
  │    INSERT qtss_ml_drift_snapshots (feature, psi, status, ...)
  ├─ critical = [f FOR f IF psi ≥ psi_critical_threshold]
  ├─ IF len(critical) ≥ critical_features_for_trip:
  │    INSERT qtss_ml_breaker_events (model_id, action, reason, critical_features)
  │    IF action = 'deactivate': UPDATE qtss_models SET active=false WHERE id=active_model.id
  │    OUTBOX: operator alert ("PSI breaker tripped on N features")
  └─ DONE
```

---

## 3. Operator akışı (manuel adımlar)

### Adım 1 — Alert geldi
GUI banner veya outbox: **"PSI breaker tripped on 3 features: wyckoff.phase, derivatives.oi_delta, classical.atr_norm"**

### Adım 2 — Doğrula
```sql
SELECT * FROM qtss_ml_breaker_events WHERE resolved_at IS NULL ORDER BY fired_at DESC;
```

Açık event varsa `critical_features` JSONB'sini incele.

### Adım 3 — Kaynağı belirle (dört olasılık)

| Kaynak | Belirti | Tanı |
|---|---|---|
| **Gerçek rejim değişikliği** | Piyasa regime_snapshot'unda dramatik dönüş (ranging → trending) | `SELECT kind, COUNT(*) FROM qtss_regime_snapshots WHERE at > now()-'7 days' GROUP BY 1;` |
| **Feature ingestion bozuk** | Belirli bir source (wyckoff, derivatives) tamamen eksik | `SELECT feature_source, COUNT(*) FROM qtss_features_snapshot WHERE created_at > now()-'1 day' GROUP BY 1;` — eksik source |
| **Upstream API değişikliği** | Derivatives/onchain verisi şema değiştirdi | source adapter loglarında parse hatası |
| **Sembol evreni değişti** | Yeni venue/segment eklendi, eski eğitim bunu görmedi | `SELECT DISTINCT symbol FROM qtss_setups WHERE created_at > active_model.trained_at EXCEPT SELECT DISTINCT symbol FROM qtss_setups WHERE created_at <= active_model.trained_at;` |

### Adım 4 — Aksiyon (kaynağa göre)

**Gerçek rejim değişikliği** → retrain gerek:
```bash
qtss-trainer train --activate-if-better --notes "post-drift: regime shift detected"
```
(scheduler zaten tetikleyecek — T3. Bu manuel acil yol.)

**Feature ingestion bozuk** → önce data akışını tamir:
```bash
# Worker log'larında hangi source'un ingestion'ı patlamış?
journalctl -u qtss-worker --since="2 hours ago" | grep -i "feature.*error\|ingestion.*fail"
# Upstream düzelince 1 saat feature akışı normale dönmesini bekle, sonra retrain.
```
**RETRAIN'İ ERKEN YAPMA** — bozuk feature'larla eğitirsen problemi modelde gömersin.

**Upstream API değişikliği** → source adapter'ı fix, retrain:
- İlgili crate'i güncelle, `cargo check`, deploy.
- `feature_store.spec_version` bump et (migration).
- Önceki modeller `feature_spec_version` mismatch ile invalid — automatically shadow'a düşer (registry kontrolü).

**Sembol evreni değişti** → backfill:
```bash
# Yeni sembollerde historical replay ile veri hızla üret:
qtss-worker backfill-training --symbols=NEW1,NEW2 --from=2024-01-01
qtss-trainer train --activate-if-better --notes "post-drift: universe expansion"
```

### Adım 5 — Breaker'ı kapat
Retrain başarılı ve yeni model aktif olduğunda:
```sql
UPDATE qtss_ml_breaker_events
SET resolved_at = now(),
    resolved_by = 'oguz',
    resolution_note = 'Retrained on 2026-04-20; AUC 0.68→0.71. Regime shift absorbed.'
WHERE resolved_at IS NULL;
```

Sidecar reload:
```bash
curl -X POST http://127.0.0.1:8790/reload
```

### Adım 6 — Post-mortem

`resolution_note` alanı zorunlu. Tekrarlayan event'lerde pattern ara:
```sql
SELECT resolution_note, COUNT(*) FROM qtss_ml_breaker_events
WHERE fired_at > now() - '90 days'
GROUP BY 1 ORDER BY 2 DESC;
```

Sık tekrarlayan kaynak → playbook'a yeni adım ekle veya `drift.psi_critical_threshold` tuning yap.

---

## 4. Fail-safe davranış

Breaker tripped + action=deactivate:
- Sidecar /score → 503 ("no active model loaded")
- Rust worker `inference.fail_open=false` → setup **reject** edilir (`qtss_v2_setup_rejections` satırı: reason="ai_unavailable_breaker")
- `inference.fail_open=true` ayarı sadece acil durumlar için (kötü model > hiç model değil).

Recovery sonrası `qtss_v2_setup_rejections` üzerinden kaç setup'ın engellendiği görülebilir.

---

## 5. Eşik ayarlama

Breaker çok sık tetikliyorsa (haftada 3+), muhtemelen:
- `psi_critical_threshold` çok düşük → 0.30-0.35'e çıkar
- `critical_features_for_trip` çok düşük → 5'e çıkar
- `psi_lookback_hours` çok kısa → 48h yap (gürültü azalır)

Hiç tetiklemiyor ama model zamanla kötüye gidiyorsa:
- `psi_warning_threshold` 0.08'e düşür (erken sinyal)
- `check_interval_secs` 1800 → 900 (15 dk)

---

## 6. İlişkili dokümanlar

- [Retraining playbook](FAZ_9B_RETRAINING_PLAYBOOK.md)
- [Deployment checklist](FAZ_9B_MODEL_DEPLOYMENT_CHECKLIST.md)
