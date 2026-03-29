# QTSS dağıtım notları

## `qtss-worker` (systemd, Linux)

Kline WebSocket ile kapanan mumları PostgreSQL `market_bars` tablosuna yazar. **Aynı** `DATABASE_URL` API ile uyumlu olmalı; kök `.env` içinde `QTSS_KLINE_*` tanımlı olmalı.

### 1. Release derlemesi

```bash
cd /app/qtss
cargo build --release -p qtss-worker
```

İkili: `target/release/qtss-worker`.

### 2. Ortam

Kök `.env` örneği: repoda `.env.example`. Worker için **mutlaka** (yorum satırı değil, düz `KEY=value`):

```env
DATABASE_URL=postgres://...
QTSS_KLINE_SYMBOL=BTCUSDT
QTSS_KLINE_INTERVAL=1m
QTSS_KLINE_SEGMENT=spot
```

**systemd `EnvironmentFile`:** `#` ile başlayan satırlar yüklenmez. `.env.example` içindeki `# QTSS_KLINE_SYMBOL=...` satırını kopyalayıp kullanırsanız değişken hiç gelmez — `#` kaldırın veya yeni satır ekleyin.

`load_dotenv()` çalışma dizininde `.env` arar; systemd biriminde `WorkingDirectory` repoya işaret etmeli. Servis kullanıcısı `.env` dosyasını okuyabilmeli (`EnvironmentFile=` yolu doğru ve izinler uygun olmalı).

**Doğrulama:** süreç çalışırken

```bash
pid=$(pidof qtss-worker)
tr '\0' '\n' < /proc/$pid/environ | grep QTSS_KLINE
```

boşsa değişken yok demektir. Gerekirse birim dosyasında doğrudan:

```ini
Environment=QTSS_KLINE_SYMBOL=BTCUSDT
```

kullanın (`deploy/systemd/qtss-worker.service.example` içinde yorumlu örnek var).

### 3. systemd birimi

```bash
sudo cp deploy/systemd/qtss-worker.service.example /etc/systemd/system/qtss-worker.service
sudo nano /etc/systemd/system/qtss-worker.service
# WorkingDirectory, EnvironmentFile, ExecStart yollarını kendi sunucuna göre düzenle
sudo systemctl daemon-reload
sudo systemctl enable --now qtss-worker
```

Durum ve log:

```bash
sudo systemctl status qtss-worker
journalctl -u qtss-worker -f
```

### 4. WSL / geliştirme

WSL’de systemd kullanımı dağıtıma göre değişir; yoksa aynı `.env` ile:

```bash
cd /app/qtss && set -a && source .env && set +a && ./target/release/qtss-worker
```

veya `tmux` / `screen` içinde sürekli çalıştırın.

### 5. API ile birlikte

API (`qtss-api`) ayrı bir süreç olarak çalışır; worker yalnızca veri yazar. İkisi de PostgreSQL’e bağlanır; API’yi durdurmak worker’ı etkilemez (tersi de geçerli).

### 6. `qtss-api` sağlık ve metrik uçları

Kubernetes veya benzeri düzenleyiciler için:

- **`GET /live`** — liveness; süreç cevap veriyorsa 200 (dış bağımlılık yok).
- **`GET /ready`** — readiness; PostgreSQL `SELECT 1` başarılıysa 200, aksi 503 (pod trafiği kesilir).
- **`GET /health`** — özet JSON (`status`, `service`).
- **`GET /metrics`** — Prometheus metin çıktısı; üretimde `QTSS_METRICS_TOKEN` ile koruyun (`Authorization: Bearer …` veya `?token=`).

Örnek probe (Ingress / Deployment yorumu olarak):

```yaml
livenessProbe:
  httpGet:
    path: /live
    port: 8080
readinessProbe:
  httpGet:
    path: /ready
    port: 8080
```

### 7. `qtss-worker` sağlık uçları

`QTSS_WORKER_HTTP_BIND` doluysa (ör. `127.0.0.1:9090`) ayrı bir HTTP dinleyicisi açılır:

- **`GET /live`** — liveness (`qtss-worker` süreç cevabı).
- **`GET /ready`** — `DATABASE_URL` varsa PostgreSQL `SELECT 1` (başarısızsa 503); yoksa 200 ve `database: none`.

API ile aynı pod’da değilseniz probe `port` değerini bu bind ile eşleştirin.
