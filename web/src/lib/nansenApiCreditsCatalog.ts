/** Rows for the Nansen drawer: official per-call credits (docs.nansen.ai overview) + worker `system_config` keys. */

export type NansenApiCatalogKind = "worker_default_on" | "worker_opt_in" | "reference_only";

export type NansenApiCatalogRow = {
  id: string;
  label: string;
  path: string;
  creditsLabel: string;
  /** Why this endpoint exists and how QTSS / Nansen docs use it (shown when user clicks ⓘ). */
  purposeDetail: string;
  /** `worker` module `system_config.config_key`; unset = not controllable from UI. */
  configKey?: string;
  envKey?: string;
  kind: NansenApiCatalogKind;
};

/** Loops spawned by `qtss-worker` (toggle via admin checkboxes → `system_config`). */
export const NANSEN_WORKER_LOOP_ROWS: NansenApiCatalogRow[] = [
  {
    id: "token_screener",
    label: "Token screener",
    path: "POST /api/v1/token-screener",
    creditsLabel: "1",
    purposeDetail:
      "Çoklu zincirde token tarama: hacim, akıllı para, borsa akışı gibi filtrelerle aday listesi üretir. QTSS worker bu sonucu `nansen_snapshots` ve `data_snapshots` (kaynak: `nansen_token_screener`) altına yazar; setup taraması ve confluence gibi üst katmanlar buradan beslenebilir.",
    configKey: "nansen_token_screener_loop_enabled",
    envKey: "NANSEN_TOKEN_SCREENER_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "netflows",
    label: "Smart money netflows",
    path: "POST /api/v1/smart-money/netflow",
    creditsLabel: "5",
    purposeDetail:
      "Seçilen zincirlerde akıllı para cüzdanlarının token bazında net alım/satım eğilimini gösterir. QTSS bu paketi periyodik olarak çekip `data_snapshots` (`nansen_netflows`) içinde saklar; piyasa bağlamı ve sinyal motoru gecikme/latency kontrollerinde kullanılabilir.",
    configKey: "nansen_loop_netflows_enabled",
    envKey: "NANSEN_NETFLOWS_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "holdings",
    label: "Smart money holdings",
    path: "POST /api/v1/smart-money/holdings",
    creditsLabel: "5",
    purposeDetail:
      "Akıllı para adreslerinin güncel token pozisyonlarını toplu görüntüler. Worker çıktısı `nansen_holdings` anahtarıyla `data_snapshots`’a yazılır; hangi tokenlarda kurumsal/akıllı para birikimi olduğunu izlemek için uygundur.",
    configKey: "nansen_loop_holdings_enabled",
    envKey: "NANSEN_HOLDINGS_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "sm_perp_trades",
    label: "Smart money perp trades (HL)",
    path: "POST /api/v1/smart-money/perp-trades",
    creditsLabel: "5",
    purposeDetail:
      "Hyperliquid üzerinde akıllı para etiketli cüzdanların perp işlemlerini listeler. QTSS `nansen_perp_trades` anlık görüntüsünü günceller; copy-trade veya HL odaklı stratejiler için ham işlem akışı sağlar.",
    configKey: "nansen_loop_smart_money_perp_trades_enabled",
    envKey: "NANSEN_PERP_TRADES_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "sm_dex_trades",
    label: "Smart money DEX trades (24h)",
    path: "POST /api/v1/smart-money/dex-trades",
    creditsLabel: "5",
    purposeDetail:
      "Son 24 saatte akıllı para cüzdanlarının DEX üzerindeki işlemlerini döner. Varsayılan olarak worker’da opt-in: açıkken `nansen_smart_money_dex_trades` anlık görüntüsü dolar; DEX akışı ve kısa vadeli smart money davranışı için kullanılır.",
    configKey: "nansen_loop_smart_money_dex_trades_enabled",
    envKey: "NANSEN_SM_DEX_TRADES_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "who_bought",
    label: "TGM who bought / sold",
    path: "POST /api/v1/tgm/who-bought-sold",
    creditsLabel: "1",
    purposeDetail:
      "Belirli bir token için son dönemde alım/satım yapan adreslerin özetini verir. Worker’da `NANSEN_WHO_BOUGHT_BODY_JSON` veya token+chain env ile gövde tanımlanmalıdır; çıktı `nansen_who_bought_sold` olarak saklanır, likidite/ilgi analizi için uygundur.",
    configKey: "nansen_loop_who_bought_sold_enabled",
    envKey: "NANSEN_WHO_BOUGHT_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "flow_intel",
    label: "TGM flow intelligence",
    path: "POST /api/v1/tgm/flow-intelligence",
    creditsLabel: "1",
    purposeDetail:
      "Tek bir token etrafında akıllı para, borsa, balina, halka açık figür gibi kategorilere göre akış özetini sunar. `app_config` (`nansen_flow_intel_by_symbol`) ile sembol başına gövde verilir; `nansen_flow_intelligence` anlık görüntüsü confluence ve bağlam panellerinde kullanılabilir.",
    configKey: "nansen_loop_flow_intelligence_enabled",
    envKey: "NANSEN_FLOW_INTEL_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "perp_pnl_lb",
    label: "TGM perp PnL leaderboard",
    path: "POST /api/v1/tgm/perp-pnl-leaderboard",
    creditsLabel: "5",
    purposeDetail:
      "Belirli bir perp sembolü ve tarih aralığında en kârlı adresleri sıralar. Worker sonuçları `nansen_perp_leaderboard` olarak yazar ve başarılı yanıtta cüzdan listesini `nansen_whale_watchlist` `app_config` kaydına işleyebilir; whale takibi ve HL leaderboard stratejileri için temel veridir.",
    configKey: "nansen_loop_perp_pnl_leaderboard_enabled",
    envKey: "NANSEN_PERP_LEADERBOARD_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "whale_perp_agg",
    label: "Whale perp aggregate (watchlist)",
    path: "POST /api/v1/profiler/perp-positions",
    creditsLabel: "1",
    purposeDetail:
      "İzleme listesindeki (watchlist) adreslerin Hyperliquid perp pozisyonlarını okur ve tek bir birleşik anlık görüntüde toplar. Çıktı `nansen_whale_perp_aggregate`; büyük yön pozisyonlarını ve sağlık metriklerini tek yerden izlemek içindir.",
    configKey: "nansen_loop_whale_perp_aggregate_enabled",
    envKey: "NANSEN_WHALE_PERP_AGGREGATE_ENABLED",
    kind: "worker_default_on",
  },
  {
    id: "tgm_flows",
    label: "TGM flows",
    path: "POST /api/v1/tgm/flows",
    creditsLabel: "1",
    purposeDetail:
      "Token bazında zaman içinde kategori akışlarını (ör. smart money giriş-çıkış) verir. Opt-in döngü; `nansen_tgm_flows_by_symbol` ile motor sembollerine göre gövde eşlenir. `nansen_tgm_flows` anlık görüntüsü trend ve birikim analizine yardımcı olur.",
    configKey: "nansen_loop_tgm_flows_enabled",
    envKey: "NANSEN_TGM_FLOWS_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "tgm_perp_trades",
    label: "TGM perp trades",
    path: "POST /api/v1/tgm/perp-trades",
    creditsLabel: "1",
    purposeDetail:
      "Belirli bir perp sembolü için işlem geçmişini döner. Sembol başına yapılandırma `nansen_tgm_perp_trades_by_symbol` üzerinden; çıktı `nansen_tgm_perp_trades`. HL token derinliği ve işlem yoğunluğu analizi içindir.",
    configKey: "nansen_loop_tgm_perp_trades_enabled",
    envKey: "NANSEN_TGM_PERP_TRADES_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "tgm_dex",
    label: "TGM DEX trades",
    path: "POST /api/v1/tgm/dex-trades",
    creditsLabel: "1",
    purposeDetail:
      "Seçilen token için DEX üzerindeki işlemleri listeler. `nansen_tgm_dex_trades_by_symbol` ile eşleme; `nansen_tgm_dex_trades` anlık görüntüsü spot/EVM tarafında işlem akışı izlemek içindir.",
    configKey: "nansen_loop_tgm_dex_trades_enabled",
    envKey: "NANSEN_TGM_DEX_TRADES_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "tgm_token_info",
    label: "TGM token information",
    path: "POST /api/v1/tgm/token-information",
    creditsLabel: "1",
    purposeDetail:
      "Token istatistikleri: piyasa değeri, hacim, holder sayısı, işlemci sayısı vb. `nansen_tgm_token_information_by_symbol` ile beslenir; `nansen_tgm_token_information` üzerinden üst katmana özet metrik sağlar.",
    configKey: "nansen_loop_tgm_token_information_enabled",
    envKey: "NANSEN_TGM_TOKEN_INFORMATION_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "tgm_indicators",
    label: "TGM indicators",
    path: "POST /api/v1/tgm/indicators",
    creditsLabel: "5",
    purposeDetail:
      "Nansen risk/ödül göstergeleri. Sembol eşlemesi `nansen_tgm_indicators_by_symbol`; sonuç `nansen_tgm_indicators`. Token seçiminde ek skor katmanı ve uyarı sinyalleri için kullanılır.",
    configKey: "nansen_loop_tgm_indicators_enabled",
    envKey: "NANSEN_TGM_INDICATORS_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "tgm_perp_pos",
    label: "TGM perp positions",
    path: "POST /api/v1/tgm/perp-positions",
    creditsLabel: "5",
    purposeDetail:
      "Belirli bir perp sembolünde açık pozisyonları (kaldıraç, PnL, likidasyon fiyatı vb.) gösterir. `nansen_tgm_perp_positions_by_symbol`; çıktı `nansen_tgm_perp_positions`. Piyasa tarafı yoğunluk ve yön dağılımı için uygundur.",
    configKey: "nansen_loop_tgm_perp_positions_enabled",
    envKey: "NANSEN_TGM_PERP_POSITIONS_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "tgm_holders",
    label: "TGM holders",
    path: "POST /api/v1/tgm/holders",
    creditsLabel: "5",
    purposeDetail:
      "Üst holder’lar, akıllı para, borsa, balina gibi kırılımlarla bakiye dağılımı. `nansen_tgm_holders_by_symbol`; `nansen_tgm_holders`. Token sahiplik yapısı ve konsantrasyon analizi içindir.",
    configKey: "nansen_loop_tgm_holders_enabled",
    envKey: "NANSEN_TGM_HOLDERS_ENABLED",
    kind: "worker_opt_in",
  },
  {
    id: "perp_screener",
    label: "Perp screener (HL)",
    path: "POST /api/v1/perp-screener",
    creditsLabel: "1",
    purposeDetail:
      "Hyperliquid’de hacim ve akıllı para aktivitesi yüksek perp sembollerini tarar. Opt-in döngü; çıktı `nansen_perp_screener`. Yeni HL fırsatları ve tarama listesi üretmek için kullanılır.",
    configKey: "nansen_loop_perp_screener_enabled",
    envKey: "NANSEN_PERP_SCREENER_ENABLED",
    kind: "worker_opt_in",
  },
];

/** Documented APIs without a worker loop in this repo (reference / future). */
export const NANSEN_REFERENCE_ONLY_ROWS: NansenApiCatalogRow[] = [
  {
    id: "ref_sm_hist_holdings",
    label: "Smart money historical holdings",
    path: "POST /api/v1/smart-money/historical-holdings",
    creditsLabel: "5",
    purposeDetail:
      "Akıllı para cüzdanlarının geçmişteki token bakiyelerini zaman içinde inceler. Geçmiş birikim/dağıtım paternleri ve backtest benzeri analizler için Nansen dokümantasyonunda anlatıldığı gibi kullanılır; QTSS worker döngüsü henüz yok.",
    kind: "reference_only",
  },
  {
    id: "ref_sm_dcas",
    label: "Smart money Jupiter DCAs",
    path: "POST /api/v1/smart-money/dcas",
    creditsLabel: "5",
    purposeDetail:
      "Solana’da akıllı para tarafından başlatılan Jupiter DCA emirlerini listeler. Otomatik birikim stratejilerini ve akıllı para Solana akışını anlamak için; şu an QTSS’de ayrı döngü tanımlı değil.",
    kind: "reference_only",
  },
  {
    id: "ref_tgm_transfers",
    label: "TGM transfers",
    path: "POST /api/v1/tgm/transfers",
    creditsLabel: "1",
    purposeDetail:
      "Token için büyük transferleri ve hareketleri öne çıkarır. Balina hareketleri ve şüpheli akışları izlemek için; worker entegrasyonu yok.",
    kind: "reference_only",
  },
  {
    id: "ref_tgm_pnl_lb",
    label: "TGM PnL leaderboard",
    path: "POST /api/v1/tgm/pnl-leaderboard",
    creditsLabel: "5",
    purposeDetail:
      "Adreslerin genel gerçekleşmiş/gerçekleşmemiş PnL sıralaması (Hyperliquid perp PnL leaderboard’dan farklı olabilir). Kopya ticaret adayı aramak için dokümanda anlatılır; QTSS şu an bu uç için döngü içermiyor.",
    kind: "reference_only",
  },
  {
    id: "ref_tgm_ohlcv",
    label: "TGM token OHLCV",
    path: "POST /api/v1/tgm/token-ohlcv",
    creditsLabel: "1",
    purposeDetail:
      "Token için birleşik OHLCV mum verisi. Grafik ve teknik analiz ile on-chain metrikleri birleştirmek için; worker’da henüz yok.",
    kind: "reference_only",
  },
  {
    id: "ref_tgm_jup_dca",
    label: "TGM Jupiter DCA",
    path: "POST /api/v1/tgm/jup-dca",
    creditsLabel: "1",
    purposeDetail:
      "Belirli token için Jupiter DCA emirlerinin listesi. Token bazlı DCA ilgisini ölçmek için; QTSS worker’da yok.",
    kind: "reference_only",
  },
  {
    id: "ref_perp_lb",
    label: "Hyperliquid address leaderboard",
    path: "POST /api/v1/perp-leaderboard",
    creditsLabel: "5",
    purposeDetail:
      "Tarih aralığında en kârlı Hyperliquid adreslerini (TGM perp PnL leaderboard’a paralel ürün) listeler. Adres bazlı HL performans karşılaştırması için; qtss-nansen istemcisinde sarmalayıcı mevcut, worker döngüsü ayrı tanımlı değil.",
    kind: "reference_only",
  },
  {
    id: "ref_portfolio",
    label: "Portfolio DeFi holdings",
    path: "POST /api/v1/portfolio/defi-holdings",
    creditsLabel: "1",
    purposeDetail:
      "Adreslerin DeFi protokollerindeki pozisyonlarını izler. Portföy ve protokol riski görünümü için; QTSS worker entegrasyonu yok.",
    kind: "reference_only",
  },
  {
    id: "ref_agent_fast",
    label: "Agent fast (SSE)",
    path: "POST /api/v1/agent/fast",
    creditsLabel: "200",
    purposeDetail:
      "Nansen AI ajanına doğal dilde soru; yanıt sunucu gönderimli etkinlik akışı (SSE) ile gelir. Hızlı araştırma ve özet için; yüksek kredi maliyeti ve akış işleme gerektirir, QTSS worker’da kullanılmıyor.",
    kind: "reference_only",
  },
  {
    id: "ref_agent_expert",
    label: "Agent expert (SSE)",
    path: "POST /api/v1/agent/expert",
    creditsLabel: "750",
    purposeDetail:
      "Daha güçlü model ile çok adımlı analiz; yine SSE. Derin sentez ve örüntü açıklaması için; en yüksek kredi dilimlerinden biridir, worker entegrasyonu yok.",
    kind: "reference_only",
  },
  {
    id: "ref_search_general",
    label: "Search general",
    path: "POST /api/v1/search/general",
    creditsLabel: "—",
    purposeDetail:
      "İsim, sembol veya kontrat adresiyle token ve kurum araması. Adres çözümleme ve keşif için; qtss-nansen’de çağrı mevcut, worker döngüsü yok.",
    kind: "reference_only",
  },
  {
    id: "ref_search_entity",
    label: "Search entity name",
    path: "POST /api/v1/search/entity-name",
    creditsLabel: "0",
    purposeDetail:
      "Profiler uçlarında kullanılacak tam `entity_name` metnini bulmak için (çoğu zaman 0 kredi). Borsa/figür adlarını doğru yazmak için; worker’da otomatik döngü yok.",
    kind: "reference_only",
  },
  {
    id: "ref_profiler_labels",
    label: "Profiler labels (common / premium)",
    path: "POST /api/v1/profiler/address/labels",
    creditsLabel: "100 / 500",
    purposeDetail:
      "Adres etiketleri: standart uç ücretsiz/premium olmayan etiketleri, premium uç tüm etiketleri döner (kredi farkı dokümana göre). Cüzdan sınıflandırması ve risk etiketleri için; QTSS worker döngüsü tanımlı değil.",
    kind: "reference_only",
  },
];
