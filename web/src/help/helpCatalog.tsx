import type { ReactNode } from "react";

export type HelpTopic = {
  id: string;
  title: string;
  /** Drawer araması + id eşlemesi için düz metin (TR küçük harf). */
  searchBlob: string;
  body: ReactNode;
};

function sb(parts: string[]): string {
  return parts.join(" ").toLocaleLowerCase("tr-TR");
}

export const HELP_TOPICS: HelpTopic[] = [
  {
    id: "market-context-overview",
    title: "Piyasa bağlamı — genel (F7 / PLAN Phase E)",
    searchBlob: sb([
      "piyasa",
      "bağlam",
      "baglam",
      "f7",
      "plan",
      "phase",
      "confluence",
      "api",
      "worker",
      "env",
      "source_key",
      "btcusdt",
      "binance",
      "market-context",
      "latest",
      "summary",
      "onchain",
      "yardım",
      "yardim",
      "sss",
      "faq",
      "help",
      "dokümantasyon",
    ]),
    body: (
      <>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <strong>Bağlam</strong> sekmesi üst çubuktaki borsa / segment / sembol / zaman dilimini API çağrılarına bağlar. Worker tarafında
          confluence, Nansen ve harici çekimler için repo kökündeki <code className="mono">.env.example</code> kullanılır.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          Uç örnekleri: <code className="mono">market-context/latest</code>, <code className="mono">market-context/summary</code>,{" "}
          <code className="mono">engine/confluence/latest</code>, <code className="mono">onchain-signals/breakdown</code>,{" "}
          <code className="mono">data-snapshots</code>.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <code className="mono">source_key</code> listesi: <code className="mono">docs/DATA_SOURCES_AND_SOURCE_KEYS.md</code>. Mimari:{" "}
          <code className="mono">docs/PLAN_CONFLUENCE_AND_MARKET_DATA.md</code>, arayüz hedefi:{" "}
          <code className="mono">docs/SPEC_EXECUTION_RANGE_SIGNALS_UI.md</code> (F7), zincir sinyalleri:{" "}
          <code className="mono">docs/SPEC_ONCHAIN_SIGNALS.md</code>.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          Confluence motoru ortamda <code className="mono">QTSS_CONFLUENCE_ENGINE</code> ile açılır; <code className="mono">0</code> veya
          kapalı ile devre dışı bırakılır.
        </p>
      </>
    ),
  },
  {
    id: "market-context-summary",
    title: "Motor hedefleri özeti (market-context/summary)",
    searchBlob: sb([
      "özet",
      "summary",
      "motor",
      "hedef",
      "engine_symbols",
      "filtre",
      "sembol",
      "exchange",
      "segment",
    ]),
    body: (
      <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
        <code className="mono">GET …/market-context/summary</code> çağrısında üst çubukta <strong>sembol</strong> seçiliyken aynı{" "}
        <code className="mono">exchange</code>, <code className="mono">segment</code> ve <code className="mono">symbol</code> ile süzülür.
        Sembol boşken yanıtta tüm <strong>aktif</strong> motor hedefleri (limit ile) listelenir.
      </p>
    ),
  },
  {
    id: "engine-data-snapshots",
    title: "Motor paneli — birleşik data_snapshots",
    searchBlob: sb(["data_snapshots", "snapshot", "nansen", "taker", "confluence", "motor", "worker"]),
    body: (
      <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
        Nansen ve harici çekimler tek satır/kaynak olarak saklanır; confluence bu birleşik listeden okur (ör.{" "}
        <code className="mono">binance_taker_btcusdt</code>). Satır yoksa worker yazımı veya migration (ör.{" "}
        <code className="mono">0022</code>) kontrol edilir.
      </p>
    ),
  },
  {
    id: "engine-market-context-latest",
    title: "Motor paneli — üst çubuk için market-context/latest",
    searchBlob: sb([
      "latest",
      "signal_dashboard",
      "trading_range",
      "engine_symbols",
      "404",
      "motor",
    ]),
    body: (
      <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
        <code className="mono">GET …/analysis/market-context/latest</code> tek bir <code className="mono">engine_symbols</code> hedefi
        için <code className="mono">signal_dashboard</code>, <code className="mono">trading_range</code>, <code className="mono">confluence</code>{" "}
        ve Nansen + taker <code className="mono">data_snapshots</code> döner. Üst çubukta sembol yoksa veya hedef yoksa 404 / boş yanıt
        alınabilir — <strong>Motor</strong> sekmesinden hedef ekleyin.
      </p>
    ),
  },
  {
    id: "engine-range-signals",
    title: "Range sinyal olayları (DB)",
    searchBlob: sb([
      "range",
      "sinyal",
      "long_entry",
      "short_entry",
      "notr",
      "durum",
      "f2",
      "worker",
      "market_bars",
      "komisyon",
      "ücret",
      "fee",
      "net",
      "brüt",
      "gross",
      "setup",
      "trading_range",
      "referans",
      "stop",
      "tp",
      "kar_al",
      "payload",
    ]),
    body: (
      <>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          Worker, <code className="mono">signal_dashboard.durum</code> (LONG / SHORT / NOTR) değeri{" "}
          <strong>önceki geçerli snapshot’a göre değişince</strong> veya ilk kez yönlü bir <code className="mono">durum</code> oluşunca{" "}
          <code className="mono">long_entry</code>, <code className="mono">long_exit</code>, <code className="mono">short_entry</code>,{" "}
          <code className="mono">short_exit</code> olaylarını yazar. Yalnız NOTR kalıyorsa olay düşmez. Grafikte mum üstü işaret:{" "}
          <strong>F2</strong>.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <strong>İşlem Özeti</strong> kartı: Aynı grafik hedefi için olaylar zamana göre sıralanır; giriş→çıkış{" "}
          <strong>LIFO</strong> ile eşlenir. Satırlardaki fiyatlar <code className="mono">reference_price</code> ile hesaplanır (gerçekleşen
          dolum değildir).
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <strong>Trading Range Setup</strong> tablosu: Tüm <code className="mono">range_signal_events</code> satırlarını ayrı ayrı listeler
          (borsa, segment, sembol, aralık, giriş fiyatı + bar zamanı). <strong>Stop / TP</strong> sütunları olay yazılırken aynı turdaki{" "}
          <code className="mono">signal_dashboard</code> anlık görüntüsünden <code className="mono">stop_ilk</code> /{" "}
          <code className="mono">kar_al_ilk</code> alanlarıyla doldurulur; worker güncellemesinden önce oluşmuş kayıtlarda tire görünebilir.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <strong>Δ % (ref):</strong> Yalnızca bu referans giriş/çıkış fiyatlarından ham yüzde hareket (long/short yönüne göre).
          Açık setup’larda çıkış yoksa sütun boş kalır.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <strong>Δ % (net tahm.):</strong> Aynı referans fiyatlar üzerinden, iki bacak için de{" "}
          <strong>taker</strong> komisyon kesirinin (ondalık, ör. 0.0004) giriş ve çıkış nominaline uygulanmasıyla{" "}
          brüt hareketten düşülür: ücret = <code className="mono">entry_px × taker + exit_px × taker</code>. Oran sırası: motor
          paneli yenilenirken yüklenen Binance <strong>hesap</strong> taker oranı (JWT ile{" "}
          <code className="mono">commission-account</code>) — bunu kalıcı görmek için{" "}
          <strong>Komisyon</strong> sekmesinde aynı sembol için “hesabı yükle” kullanın — yoksa{" "}
          <code className="mono">commission-defaults</code> taker bps (tier0 / exchangeInfo ipucu). Borsa çubuğu Binance değilse
          bile bu uçlar Binance modeli içindir; karşılaştırma amaçlı tahmindir.
        </p>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          Maker, kademeli ücret veya finansman ücreti dahil değildir. Net sütun yine gerçekleşmiş işlem değil,{" "}
          <strong>tek yönlü sinyal kenarı + tek taker modeli</strong> ile düzeltilmiş referans performans özetidir.
        </p>
      </>
    ),
  },
  {
    id: "nansen-token-screener",
    title: "Nansen — Token Screener ve kurulum",
    searchBlob: sb([
      "nansen",
      "token",
      "screener",
      "api",
      "kredi",
      "403",
      "404",
      "snapshot",
      "setup",
      "worker",
    ]),
    body: (
      <>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5 }}>
          <code className="mono">qtss-worker</code> sunucuda <code className="mono">NANSEN_API_KEY</code> ile{" "}
          <code className="mono">POST …/api/v1/token-screener</code> çağrılır; sonuç <code className="mono">nansen_snapshots</code> tablosuna
          yazılır. Anahtar yalnızca worker ortamında tutulur. Resmi doküman:{" "}
          <a href="https://docs.nansen.ai/" target="_blank" rel="noreferrer">
            docs.nansen.ai
          </a>
          .
        </p>
        <ul className="muted" style={{ fontSize: "0.8rem", lineHeight: 1.45, margin: "0.35rem 0 0 1rem" }}>
          <li>
            <code className="mono">NANSEN_TICK_SECS</code> — çağrı aralığı (varsayılan 1800 sn); kredi için yüksek tutun.{" "}
            <code className="mono">NANSEN_INSUFFICIENT_CREDITS_SLEEP_SECS</code> (varsayılan 3600).
          </li>
          <li>
            <code className="mono">QTSS_SETUP_SNAPSHOT_ONLY</code> — varsayılan <code className="mono">1</code>: setup ikinci Nansen isteği
            yapmaz; yalnız snapshot okur. Canlı yedek: <code className="mono">0</code>.
          </li>
          <li>
            <code className="mono">NANSEN_TOKEN_SCREENER_REQUEST_JSON</code> — isteğe bağlı tam JSON; yoksa kod varsayılanı.
          </li>
          <li>
            <code className="mono">NANSEN_API_BASE</code> — varsayılan <code className="mono">https://api.nansen.ai</code>.
          </li>
          <li>
            API: <code className="mono">GET …/analysis/nansen/snapshot</code> ve{" "}
            <code className="mono">GET …/analysis/nansen/setups/latest</code> (JWT).
          </li>
          <li>
            <code className="mono">QTSS_SETUP_SCAN_SECS</code> — setup tarama aralığı (varsayılan 900 sn).{" "}
            <code className="mono">QTSS_SETUP_MAX_SNAPSHOT_AGE_SECS</code> yalnız <code className="mono">QTSS_SETUP_SNAPSHOT_ONLY=0</code> iken
            anlamlıdır.
          </li>
          <li>
            <strong>403 Insufficient credits</strong>: aralığı artırın veya planı güncelleyin; snapshot <code className="mono">hata</code> alanı
            API yanıtını taşır.
          </li>
          <li>
            <strong>404</strong> on <code className="mono">…/nansen/setups/latest</code>: sunucudaki <code className="mono">qtss-api</code> sürümü
            veya <code className="mono">VITE_API_BASE</code> yapılandırması (yolu çiftlemeyin).
          </li>
        </ul>
        <p className="muted" style={{ fontSize: "0.82rem", lineHeight: 1.5, marginTop: "0.5rem" }}>
          Setup çıktısı: <code className="mono">nansen_setup_runs</code> / <code className="mono">nansen_setup_rows</code> (migration{" "}
          <code className="mono">0020</code>). En iyi <strong>5 LONG</strong> + <strong>5 SHORT</strong> satır gösterimi hedeflenir.
        </p>
      </>
    ),
  },
];

export function filterHelpTopics(query: string): HelpTopic[] {
  const q = query.trim().toLocaleLowerCase("tr-TR");
  if (!q) return HELP_TOPICS;
  return HELP_TOPICS.filter(
    (t) =>
      t.id.includes(q) ||
      t.title.toLocaleLowerCase("tr-TR").includes(q) ||
      t.searchBlob.includes(q),
  );
}
