# QTSS Dev → Prod Deploy Checklist

Bu dosya her deploy öncesi yukarıdan aşağı işaretlenerek takip edilir. `deploy/preflight.sh` çoğu adımı otomatik doğrular; manuel adımlar açıkça işaretlidir.

## 0. Ön hazırlık (dev makine)

- [ ] `git status` temiz, tüm değişiklikler commit edildi.
- [ ] `git push origin main` — tüm commit'ler GitHub'da.
- [ ] Son commit'ler `main` üzerinde (feature branch'teysek merge edildi).

## 1. Lokal preflight (zorunlu)

```bash
./deploy/preflight.sh
# veya prod DB ile tam karşılaştırma:
PROD_DATABASE_URL='postgres://...' ./deploy/preflight.sh
# + testler:
RUN_TESTS=1 PROD_DATABASE_URL='...' ./deploy/preflight.sh
```

Script şunları kontrol eder:
1. **Migration dosya tekrarı yok** — aynı `00NN_` önekli iki dosya sqlx'i kilitler.
2. **Git tree temiz + origin ile senkron**.
3. **Yeni migration'larda sır sızıntısı yok** — `postgres://...:pw@`, `sk-...`, AWS anahtarları, vb.
4. **`INSERT INTO system_config` satırları `ON CONFLICT` içeriyor** — yoksa redeploy'da prod çöker.
5. **`cargo build --release` yeşil**.
6. **`cargo clippy -- -D warnings` yeşil** (uyarı = hata).
7. **`cargo test --workspace` yeşil** (opsiyonel).
8. **PROD `_sqlx_migrations` ↔ disk `migrations/` diff'i** — prod'da var ama diskte yok **→ DEPLOY DURDUR**.

Kırmızı herhangi bir `FAIL` varsa **deploy etme**; düzelt → commit → push → tekrar çalıştır.

## 2. Prod DB audit (opsiyonel ama önerilir)

```bash
psql "$PROD_DATABASE_URL" -f deploy/migration_status.sql
```

Şunları gözden geçir:
- [ ] `success = false` satırı yok (yarıda kalmış migration).
- [ ] Uygulanan son migration dev'deki ile tutarlı (ileride değil).
- [ ] `qtss_v2_detections` üzerindeki `idx_v2_detections_fst_outcome` gibi hot-path index'leri mevcut.
- [ ] `tbm` modül `system_config` anahtarları (P22..P26 → `anchor.*`, `confirm.*`, `checklist.*`, `effort_result.*`, `mtf.htf_parents`) seed edilecek/yenilerse migration içinde var.

## 3. Prod deploy

```bash
ssh prod
cd /app/qtss
./deploy/pull-build-restart.sh
```

Varsayılan davranış: `git pull --ff-only main` → `cargo build --release -p qtss-api -p qtss-worker` → `npm build` (web) → `systemctl restart qtss-api qtss-worker qtss-web qtss-web`.

Worker/API başlarken `run_migrations` otomatik çalışır; yeni `.sql` dosyaları sıraya göre uygulanır. İlk başlangıç log'unu izle:

```bash
sudo journalctl -u qtss-worker -f --since '1 min ago'
sudo journalctl -u qtss-api    -f --since '1 min ago'
```

Beklenen:
- `sqlx::migrate` satırları hatasız (checksum drift yok, duplicate prefix yok).
- `tbm` worker `tick_interval` log'larını emit ediyor.
- API health endpoint 200 dönüyor: `curl -fsS http://127.0.0.1:<PORT>/healthz`.

## 4. Post-deploy smoke (5 dk)

- [ ] Worker log'unda `ERROR` satırı yok.
- [ ] GUI açılıyor, son TBM detection listesi dolu.
- [ ] En az bir yeni `qtss_v2_detections` satırı eklendi:
  ```sql
  SELECT COUNT(*) FROM qtss_v2_detections
   WHERE detected_at > now() - interval '5 minutes';
  ```
- [ ] Yeni migration'ların eklediği `system_config` anahtarları görünüyor:
  ```sql
  SELECT config_key FROM system_config WHERE module='tbm' ORDER BY config_key;
  ```

## 5. Rollback planı (sorun çıkarsa)

**Kod rollback** (migration henüz uygulanmadıysa — idempotent, güvenli):
```bash
cd /app/qtss
git reset --hard <ONCEKI_COMMIT>
./deploy/pull-build-restart.sh   # eski binary'yi kurar
```

**Migration rollback** (uygulandıysa — dikkat!):
- `_sqlx_migrations` satırını **silme**. Yerine ters işlemi yapan bir yeni `00NN+1_revert_*.sql` yaz ve ileriye doğru uygula.
- Schema değişikliği geri alınamıyorsa: önce **DB snapshot restore**, sonra kod rollback.
- Tablonun boşaldığı/değiştiği durumda prod'a bakım penceresi açmadan ellemeyin.

## 6. Sık karşılaşılan hatalar

| Belirti | Sebep | Çözüm |
|---|---|---|
| `migration NNNN has been modified` | Uygulanmış bir `.sql` dosyası düzenlenmiş. | `cargo run -p qtss-storage --bin qtss-sync-sqlx-checksums` (yalnız dosya mantığı değişmediyse) ya da yeni bir migration yaz. |
| `previously applied but is missing` | Prod'da kayıtlı bir version diskte yok. | Dosyayı geri koy veya `QTSS_MIGRATIONS_DIR` yanlış. |
| `duplicate key value violates unique constraint "system_config_pkey"` | Yeni migration `INSERT INTO system_config` yapıp `ON CONFLICT` koymamış. | Migration'a `ON CONFLICT (module, config_key) DO NOTHING` ekle → yeni dosya olarak push et. |
| Worker tick atmıyor | `tbm.enabled` config'i kapalı veya `tick_interval_s` çok büyük. | `system_config`'ten kontrol et. |
| `slow statement` log'u | Yeni sorgu için index eksik veya `VACUUM ANALYZE` gerekli. | `EXPLAIN (ANALYZE)` → index ekle veya `VACUUM ANALYZE <table>`. |

## 7. Ortam değişkenleri (prod)

Prod `/etc/default/qtss-*` veya systemd `Environment=` ile set edilir. Asgari:

- `DATABASE_URL` — prod Postgres.
- `QTSS_RUNTIME_MODE=live` (live/dry/backtest).
- `QTSS_MIGRATIONS_DIR=/app/qtss/migrations` — systemd `WorkingDirectory` repo root'u değilse.
- Diğer tüm iş mantığı değişkenleri → `system_config` (CLAUDE.md #2).
