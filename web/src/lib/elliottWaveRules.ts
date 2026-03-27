/**
 * Elliott dalga prensipleri — `public/elliott_dalga_prensipleri.txt` (§2.5.3) ile hizalı.
 * §2.5.3.4–§2.5.5 tam metin: `elliottTezExtended.ts` + `elliottTez2534|254|255.ts`.
 * Otomatik çizim: `elliottEngineV2` (MTF ZigZag + itki/düzeltme).
 */

export const ELLIOTT_REFERENCE_TXT_URL = "/elliott_dalga_prensipleri.txt";

/** Dış Türkçe özet (Rankia) — yatırım tavsiyesi değildir; üçüncü taraf içerik. */
export const ELLIOTT_REFERENCE_RANKIA_URL = "https://rankia.com.tr/elliott-dalgalari/";

export const ELLIOTT_TECHNICAL_ANALYSIS_SUMMARY = [
  "Teknik analiz: geçmiş fiyat ve hacim davranışını grafik ve göstergelerle inceler; gelecek için olasılık/tahmin üretir, kesin sonuç iddia etmez.",
  "Elliott yaklaşımı: piyasa hareketleri tekrar eden dalga kalıplarında fraktal (iç içe) yapı gösterebilir.",
] as const;

/** Dalga dereceleri (üstten alta örnek hiyerarşi). */
export const ELLIOTT_DEGREE_LEVELS = [
  "Grand Supercycle",
  "Supercycle",
  "Cycle",
  "Primary",
  "Intermediate",
  "Minor",
  "Minute",
  "Minuette",
  "Subminuette",
] as const;

/** 2.5.3.1 — giriş paragrafları (kaynak metin). */
export const ELLIOTT_TEZ_2531_INTRO_PARAGRAPHS = [
  "İtki yani itici dalga olarak Türkçe karşılık bulan bu kavram, yeni bir dalganın hareketine başlamasını sağlayan, ona itki veren anlamını taşımaktadır. İtki dalgaları fiyatın yeni bir tepe ya da yeni bir dip gerçekleştirmesini sağlayan ve toplamda beş dalgadan oluşan yapılardır. İtki dalgalarının ana özellikleri fiyat trendinin ana yönünü belirliyor olmasıdır. İtki dalgaları 1,2,3,4,5 rakamları ile etiketlenir ve beş dalganın bir araya gelmesiyle döngüsünü tamamlamış olurlar.",
  "Burada 1,3 ve 5. dalgalar itki dalgalarıdır ve ana hareketin yönünü belirlerler. 2 ve 4 numaralı dalgalar ise ana hareketin tam tersi yönünde oluşan düzeltme dalgalarıdır (Şengöz, 2002:41-42).",
  "İtki dalgalarının ana yapıları 5-3-5-3-5 şeklindedir ve bu biçimde ilerlerler. 5 dalgalık bir trend oluştuktan sonra 3 dalgalık bir düzeltme gerçekleşir. İdeal bir itki dalgası klasik şema ile uyumlu biçimde görülmelidir.",
  "Bir itki dalgası kendisinden sonra daha da büyük bir itki oluşturamıyorsa düzeltme dalgası olarak kalır ve bir itki dalgasından sonra meydana gelen bir düzeltme dalgası olarak ilerleyişini sürdürür. Büyük dereceye sahip bir itki dalgası, küçük dereceli üç itki dalgasının birleşiminden oluşur.",
  "Elliott Dalga Prensipleri’nde uygulamanın doğru bir biçimde yapılabilmesi için her bir kalıbın (pattern) doğru bir biçimde öğrenilmesi büyük bir önem arz etmektedir. Fiyat analizlerinde dalga kalıpları doğru bir biçimde belirlenebilirse fiyatların sadece olası hedefleri değil, aynı zamanda hangi yolu izleyerek hangi şekilde oluşabileceği hakkında tahminleri ön görmek mümkün olur.",
  "Elliott Dalga Prensipleri teorisine göre kurallar ve rehber ilkeler yol gösterici görevi üstlenmektedir (Baysal, 2011:97-99):",
] as const;

/**
 * Baysal (2011) — “Kurallar” bölümü (zorunlu).
 * Otomatik tespitte kısmen: dalga 2 başlangıç, dalga 3 en kısa değil, dalga 4/1 alanı (sıkı mod), yapı.
 */
export const ELLIOTT_IMPULSE_RULES = [
  {
    id: "tez_hard_wave2_wave4_length",
    title: "Kural — Dalga 2 ve 4 (uzunluk ve başlangıç)",
    detail:
      "Kalıpta fiyat olarak 2. Dalga asla 1. İtki dalgasından daha uzun olamaz. Ayrıca, 2. dalga 1. dalganın başlangıç noktasını asla aşamaz. Bu kuralın aynısı 4. Dalga içinde geçerlidir. Kalıpta 4. dalga asla 3. dalgadan daha uzun olamaz ve 3. dalganın başlangıç noktasını aşamaz.",
  },
  {
    id: "tez_hard_wave3_shortest",
    title: "Kural — Dalga 3 en kısa olamaz",
    detail: "Kalıpta 3. dalga asla tüm dalgalar arasında en kısa dalga olamaz.",
  },
  {
    id: "tez_hard_wave3_vs_wave1_end",
    title: "Kural — Dalga 3 ve dalga 1 bitişi",
    detail: "Kalıpta 3. Dalga hiçbir zaman 1. dalganın bitiş fiyatının altında bir fiyatta bulunamaz.",
  },
  {
    id: "tez_hard_wave4_wave1_area",
    title: "Kural — Dalga 4 ve dalga 1 fiyat alanı",
    detail: "Kalıpta 4. dalga hiçbir zaman 1. dalganın oluşturduğu fiyat alanına giremez.",
  },
] as const;

/** “Rehber İlkeler” — yorum / hedef; otomatik modülde çoğu uygulanmaz. */
export const ELLIOTT_TEZ_IMPULSE_GUIDELINES = [
  {
    id: "guide_tepe_dip",
    title: "Önceki itki tepe/dip",
    detail:
      "İtki dalgaları kendilerinden daha önce oluşan itki dalgalarının eğer fiyat yükseliş hareketinde ise tepe, eğer fiyat düşüş hareketinde ise dip noktalarını geçer. Eğer geçemiyorsa bu dalganın daha küçük bir dereceye sahip olma olasılığı gözden geçirilmelidir.",
  },
  {
    id: "guide_wave2_retrace",
    title: "2. dalga geri alım kapsamı",
    detail:
      "2. dalga bir önceki dalganın alt dereceli 4. dalgasına kadar ilerler ve 5. dalgasının tamamını geri almalıdır.",
  },
  {
    id: "guide_fib_retrace",
    title: "2. ve 4. dalga geri alım oranları",
    detail:
      "2. ve 4. dalgaların düzeltme yani fiyatı geri alış seviyeleri genellikle %38.2, %50.0 ya da %61.8’dir. Uzunluk olarak daha kısa süren bir düzeltme dalgasının ardından daha güçlü bir itici dalga oluşması beklenir.",
  },
  {
    id: "guide_sharp_vs_flat",
    title: "2. keskin — 4. yatay",
    detail:
      "2. ve 4. dalgalar arasından 2. dalganın genellikle daha büyük, daha keskin bir düzeltme gerçekleştirmesi, 4. dalganın ise daha yatay bir düzeltme gerçekleştirmesi beklenmektedir.",
  },
  {
    id: "guide_alternation_long",
    title: "Almaşıklık (detay)",
    detail:
      "2. ve 4. dalgalar birbirlerinin almaşığıdır. 2. dalga zigzag modelinde bir düzeltme gerçekleştiriyorsa, 4. dalganın daha yatay bir düzeltme gerçekleştirmesi beklenir. Aynı durum tersi ihtimal için de geçerlidir. 2. Dalga %38 bir düzeltme gerçekleştirdiyse, 4. Dalganın %62’lik bir geri alım yapması beklenir. Eğer 2. Dalga karmaşık bir düzeltme modeli oluşturuyorsa 4. Dalganın basit bir yapıda olması beklenmektedir. Süre bakımından da 2. Dalga kısa sürede düzeltme yapmış ise, 4. Dalga düzeltmesinin daha uzun bir süre alması beklenir.",
  },
  {
    id: "guide_wave3_projection",
    title: "3. dalga uzunluk projeksiyonu",
    detail:
      "Kalıpta 3. Dalganın fiyat uzunluğu 1. Dalganın %161.8’i ya da %261.8’i kadar devam eder. Eğer ki bu koşul kalıpta sağlanıyorsa, 1. ve 5. dalgaların eşit uzunlukta olması beklenir. Eğer 1. ve 3. dalgalar yaklaşık olarak birbirilerine eşit uzunlukta oluştuysa, 5. dalganın uzatma dalgası olması beklenir. 5. dalga uzatmasının da 1. dalganın başlangıç noktasından ve 3. dalganın bitiş noktasına kadar olan uzunluğunun %162’si kadar olması beklenir.",
  },
  {
    id: "guide_extension_one_of_three",
    title: "Uzatma (1, 3 veya 5)",
    detail:
      "Kalıpta bir itki döngüsünde trend yönünde ilerleyen dalgalar, yani 1, 3, veya 5 numaralı dalgalardan birinin çoğunlukla uzatma yapması beklenir. Bir dalga uzatma yaptığında diğer iki dalganın birbirlerine eşit ya da yakın eşitlikte olması beklenir.",
  },
  {
    id: "guide_wave5_vs_wave1",
    title: "5. dalga bitiş seviyeleri",
    detail: "5. dalganın bitiş seviyesi, 1. dalganın %61.8, %100 veya %161.8 olabilir.",
  },
  {
    id: "guide_subcycle_failed_fifth",
    title: "Alt derece ve başarısız 5",
    detail: "3. dalgayı oluşturan bir alt dereceli döngüde 5. dalga başarısız 5. dalga olamaz.",
  },
  {
    id: "guide_wave5_vs_wave4",
    title: "5. dalga ve 4. dalga oranı",
    detail:
      "Kalıpta 5. dalga 4. dalganın minimum %61.8’i kadar hareketinin yönünde yükselir ya da düşer.",
  },
] as const;

/** İç indirgeme — txt ile uyumlu. */
export const ELLIOTT_SUBDIVISION_RULE = {
  id: "subdivision",
  title: "İç indirgeme (subdivision / fraktal)",
  detail:
    "Her itki dalgası (1,3,5) kendi içinde beş daha küçük itki alt dalgasına bölünebilir. Her düzeltme dalgası (2,4) genelde üç alt dalgaya (a-b-c veya iç küçük 5–3 yapılar) bölünür. Pratikte otomatik sayım, pivot seçimi ve veri uzunluğuna bağlı belirsizlik taşır.",
} as const;

/** 2.5.3.2 Uzatma — kaynak paragraflar. */
export const ELLIOTT_TEZ_2532_EXTENSION = {
  title: "2.5.3.2 Uzatma",
  paragraphs: [
    "Elliott Dalga Prensipleri’ne göre uzatma kavramı, ana trend yönünde hareket eden itici dalgalardan birinin kendi içinde belirgin bir şekilde bölünerek hareket etmesi ve diğerlerinden ana trend yönünde daha uzun olmasıdır. Bir dalga kalıbı içinde 1, 3 ya da 5 numaralı dalgalardan biri genellikle uzatma hareketini yapmaktadırlar (Frost ve Prechter, 2000:31).",
    "İtki dalga kalıplarında daha önce de bahsedildiği üzere 3. dalga hiçbir zaman en kısa dalga olamaz ve 4. dalga hiçbir zaman 1. dalganın fiyat bölgesine giremez. Bu sebeplerden dolayı itki dalgaları içinde uzatma yapma ihtimali en yüksek olan dalga 3. dalgadır. Tabii bu konudan yola çıkarak, uzatmayı yapan dalganın 1. dalga olduğu görüldüğünde trendin uzun süreceği kanısına da varılmaktadır. Eğer uzatmayı yapan dalga 5. dalga ise döngü tamamlandıktan sonra oluşacak düzeltme dalgasının uzun süreceğine işaret edebilmektedir. 3. dalganın uzatma yapması durumunda ise, 5. dalga uzamamakta ve 1. dalgaya benzeme eğiliminde hareket etmektedir.",
    "Uzatma yapan dalganın alt dereceli dalgaları, itkinin diğer 4 büyük dalgası ile neredeyse aynı büyüklük ve süreye sahiptir. Bundan dolayı birbirine yakın 9 dalgalık bir hareketmiş gibi görünürler. Birbirinden ayrılması pek de kolay olmayan bu 9 dalgalık hareketi incelendiğinde, ilk bakışta hangi dalganın uzatma yaptığını söyleyebilmek cidden çok zordur. Burada önemli olan Ralph Nelson Elliott’a göre 5 dalgalık hareketin 9 dalgalık hareketten teknik olarak pek de farklı olmadığı gerçeğidir (Baysal, 2011:101).",
  ],
} as const;

/** 2.5.3.3 Başarısız beşinci dalga. */
export const ELLIOTT_TEZ_2533_FAILED_FIFTH = {
  title: "2.5.3.3 Başarısız Beşinci Dalga",
  paragraphs: [
    "Elliott Dalga Prensipleri’ne göre başarısız olan 5. dalga, 5. dalganın 3. dalganın yaptığı tepe veya dibi geçememesi durumunda oluşmaktadır. Bu duruma genel olarak 3. dalganın sert ve keskin bir biçimde ilerlediği dönemlerde rast gelinmektedir. Eğer 5. dalga başarısız olduysa genellikle uzun süren derin düzeltme hareketleri görülmektedir. Eğer başarısızlıkla sonuçlanan bir 5. dalga şüphesi var ise markete çok dikkatli yaklaşılmalı ve derin bir düzeltme için belirli şartların oluşabileceği düşünülmelidir (Baştaş, 2014:66).",
  ],
} as const;

export const ELLIOTT_CORRECTIVE_RULES = [
  {
    id: "cycle_abc",
    title: "Tam döngü: 5–3 (1–5 + A–B–C)",
    detail:
      "Beşlik itki tamamlandıktan sonra karşı yönde üç dalgalı düzeltme (zigzag, flat, üçgen vb.) gelir.",
  },
  {
    id: "zigzag_abc",
    title: "Zigzag düzeltme (özet)",
    detail:
      "A karşı trend, B kısmi geri alım, C genelde A sonunu aşar. B’nin A’yı tam geri almaması beklenir.",
  },
  {
    id: "triangle_note",
    title: "Üçgen düzeltme",
    detail:
      "Beş dalgalı (A–E) yapı olabilir; sıkça 4. dalga veya B dalgası bağlamında tartışılır.",
  },
] as const;

export const ELLIOTT_FIBONACCI_NOTES = [
  "Geri çekilmelerde 38,2% – 50% – 61,8% sık kullanılır; zorunlu kilit değildir.",
  "Hedef projeksiyonlarda 1,618 ve türev oranlar yorum amaçlıdır.",
] as const;

export type ElliottRuleDefId = (typeof ELLIOTT_IMPULSE_RULES)[number]["id"];
