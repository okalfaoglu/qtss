# qtss-trainer — Faz 9.3 LightGBM pipeline

Tek komutla `v_qtss_training_set_closed` view'ını okur, feature snapshot
JSONB'ini düz tablolaştırır, LightGBM binary classifier eğitir ve modeli
`qtss_models` tablosuna kaydeder. Her parametre `config_schema`'da
(`ai.trainer.*`) yaşar — tuning için deploy gerekmez.

## Kurulum

```bash
cd /app/qtss/trainer
python3 -m venv .venv
. .venv/bin/activate
pip install -e .
```

## Kullanım

```bash
# .env'den DATABASE_URL okuyarak tek tur training:
qtss-trainer train

# veri birikimi kontrolü (eğitmeden önce):
qtss-trainer stats

# aktif modeli değiştir:
qtss-trainer activate <model-version>
```

## Mimari

- `db.py` — psycopg bağlantı + config_schema okuyucu (Rust tarafıyla aynı
  iki-katmanlı `system_config` > `config_schema` önceliği).
- `loader.py` — `v_qtss_training_set_closed`'den DataFrame.
- `features.py` — `features_by_source` JSONB'ını `source.feature` kolon
  adlarına flatten eder; eksik kolonlar NaN.
- `model.py` — LightGBM train + time-ordered holdout + metrics.
- `registry.py` — artifact'i diske yazar, `qtss_models` satırı insert eder.
- `__main__.py` — CLI dispatch tablosu (CLAUDE.md #1).

## Label kodlaması

`outcome.label` stringleri ikili sınıfa indirgenir:
- `win` → 1
- diğer her şey (`loss` / `timeout` / `breakeven` / null) → 0

Bu, Faz 9.0.1 `qtss_setup_outcomes` spec'iyle uyumludur.
