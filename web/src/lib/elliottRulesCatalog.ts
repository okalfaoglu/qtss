// Single catalog: tez sections + panel strings + types. Engine: elliottEngineV2/
export type ElliottTezRuleLine = { id: string; text: string };

export type ElliottTezSection = {
  id: string;
  heading: string;
  paragraphs?: readonly string[];
  rulesHeading?: string;
  rules?: readonly ElliottTezRuleLine[];
  guidelinesHeading?: string;
  guidelines?: readonly ElliottTezRuleLine[];
  /** Madde işaretli kısa liste (ör. dört düzeltme tipi) */
  bullets?: readonly string[];
};

/*
 * Elliott dalga prensipleri — public/elliott_dalga_prensipleri.txt ile hizalı.
 * §2.5.3.4–§2.5.5: bu dosyada ELLIOTT_TEZ_EXTENDED_SECTIONS.
 * Otomatik çizim: elliottEngineV2.
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

/**
 * `elliottEngineV2` ile örtüşme derecesi (panel etiketi).
 * - `full`: zigzag pivot itkisinde ilgili kontrol geçerli sayılır.
 * - `partial`: bir alt kümesi motorda; metindeki diğer cümleler henüz yok / bağlam bağımlı (ör. diyagonal).
 * - `none`: yalnızca kaynak metin; otomatik skorda doğrudan yok.
 */
export type ElliottImpulseRuleEngineScope = "full" | "partial" | "none";

export const ELLIOTT_IMPULSE_RULE_ENGINE_SCOPE: Record<
  (typeof ELLIOTT_IMPULSE_RULES)[number]["id"],
  ElliottImpulseRuleEngineScope
> = {
  /** Motor: w2 başlangıç + \|2\|≤\|1\|. Metin: dalga 4↔3 uzunluk eşitsizliği motorda yok. */
  tez_hard_wave2_wave4_length: "partial",
  tez_hard_wave3_shortest: "full",
  tez_hard_wave3_vs_wave1_end: "full",
  /** Standart itkide w4–w1 bindirme yok; diyagonal modda kasıtlı istisna. */
  tez_hard_wave4_wave1_area: "partial",
};

export function elliottImpulseRuleScopeLabelTr(scope: ElliottImpulseRuleEngineScope): string {
  if (scope === "full") return "Motor · tam";
  if (scope === "partial") return "Motor · kısmi";
  return "Yalnız metin";
}

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

export const ELLIOTT_TEZ_2534_SECTIONS: readonly ElliottTezSection[] = [
  {
    id: "tez_2534_intro",
    heading: "2.5.3.4 Diyagonal Üçgenler",
    paragraphs: [
      "Diyagonal üçgenler fiyatı yeni tepelere ve yeni diplere taşımalarına rağmen birer itki dalgası özelliği taşımazlar. Diyagonal üçgenler trend dalgaları özelliği taşırlar. Çünkü diyagonal üçgenlerin birtakım düzeltici özellikleri vardır. Diyagonal üçgenler genel olarak son yani 5. dalgalarda oluşurlar ve aynı itki dalgalarında olduğu gibi diyagonal üçgenlerde de düzeltme dalgaları kendinden önce oluşan dalgayı tam boyu kadar geri alamaz ve 3. dalga 1. ve 5. dalgaya oranla en kısa dalga olamaz. Bir diyagonal üçgen ile bir itki dalgası arasındaki en temel fark, diyagonal üçgenlerde 4. dalganın birçok durumda 1. dalganın bölgesine sarkma gerçekleştirmesidir.",
      "İki farklı diyagonal üçgen türü vardır: Sonlanan diyagonal üçgenler ve İlerleyen diyagonal üçgenler (Frost ve Prechter, 2000:35-36).",
    ],
  },
  {
    id: "tez_2534_ending_heading",
    heading: "Sonlanan Diyagonal (3-3-3-3-3)",
    paragraphs: [
      "Ralph Nelson Elliot’a göre sonlanan diyagonal üçgenler, önceki dalganın çok büyük bir fiyat mesafesini çok hızlı bir biçimde katetmesi durumunda 5. dalga içerisinde görülürler. Eğer 3. dalga sert ve uzun bir dalga ise, 5. dalgada sonlanan diyagonal üçgenlerin ortaya çıkma ihtimali daha da yükselir. Sonlanan diyagonaller her zaman mevcut dalgada son dalganın oluşturduğu formasyonun son aşamasında oluşarak piyasa trendinin yakın zamanda yön değiştireceğinin sinyalini verirler.",
      "Sonlanan diyagonaller genel olarak alçalan ya da yükselen bir takoz (kama) formasyonu görünümü oluştururlar. Buna ek olarak, beş dalgayı oluşturan küçük dereceli dalgaların her biri üç farklı dalgadan oluşur. Tam da bu sebeple, 3-3-3-3-3 sayımıyla ifade edilirler (Frost ve Prechter, 2000:36).",
      "Eğer fiyat yükselen trend hareketini devam ettirirken sonlanan diyagonaller oluştuysa, piyasa fiyatı üzerinde ayıların giderek hakim olduğuna dair bir sinyal üretilmiş olur. Bu durumda fiyat trendini aşağı doğru kırdığında fiyatlar formasyon başlangıcına kadar geri dönebilir. Tam tersi olarak, eğer fiyat düşen trend hareketini devam ettirirken bir sonlanan diyagonal oluştuysa, piyasaya boğaların hakim olmaya başladığı sinyalini alırız. Bu durumda düşen trend yukarı doğru kırıldığında kuvvetli bir yön değişimi beklenir. Fiyat formasyon başlangıcına kadar yükselbilir. Kuralları ve rehber ilkeleri inceleyelim (Frost ve Prechter, 2000:36):",
    ],
    rulesHeading: "Kurallar",
    rules: [
      {
        id: "ed_r1",
        text: "1 ve 3 numaralı dalgalar üçgen dışında bulunan birer düzeltme dalgalarıdır.",
      },
      {
        id: "ed_r2",
        text: "2, 4 ve 5 numaralı dalgalar birer düzeltme dalgalarıdır.",
      },
      {
        id: "ed_r3",
        text: "2 numaralı dalga 1 numaralı dalganın, 4 numaralı dalga da 3 numaralı dalganın altına sarkmaz.",
      },
      {
        id: "ed_r4",
        text: "3 numaralı dalganın oluşturduğu fiyat alanı daima 2 numaralı dalganın oluşturduğu fiyat alanından fazladır. Fiyat alanı bakımından bir kıyaslama yapıldığında 3 numaralı dalga, 1 ve 5 numaralı dalgalara kıyasla en kısa dalga asla olamaz. Aynı zamanda 5 numaralı dalga 1 ve 3 numaralı dalgalara oranla en uzun dalga asla olamaz.",
      },
      {
        id: "ed_r5",
        text: "4 numaralı dalga 1 numaralı dalgaya sarkma yapabilir.",
      },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [
      {
        id: "ed_g1",
        text: "2 ve 4 numaralı dalgalar birbirilerinin almaşıdır. Eğer 2. dalga zigzag bir modelde düzeltme gerçekleştiriyorsa, 4. dalganın daha yatay biçimde bir düzeltme gerçekleştirmesi beklenir. Aynı durum tam tersi ihtimal için de geçerlidir. Genel olarak, 2. dalga daha keskin, 4. dalga ise daha yatay bir düzeltme yapar.",
      },
      {
        id: "ed_g2",
        text: "2 numaralı dalga 1 numaralı dalgayı en az %23.6 oranında geri alır. Bu durum 4 numaralı dalga içinde geçerlidir.",
      },
      {
        id: "ed_g3",
        text: "Fiyat paralel bir kanal içinde ilerlemez. Genel olarak daralan bir kanal içinde ilerleme kaydedilir. Çok nadir de olsa, genişleyen kanal yapısı görülebilir.",
      },
      {
        id: "ed_g4",
        text: "Formasyonun bitişi kanal bantlarının birbiriyle kesişmemesi öncesi olmalıdır.",
      },
      {
        id: "ed_g5",
        text: "5 numaralı dalga 4 numaralı dalganın en az %61.8 oranı kadardır ve genel olarak kanal içinde ilerler. Bazı durumlarda ise kanal bandından %15 oranına yakın bir oranda sarkma meydana getirebilir.",
      },
      {
        id: "ed_g6",
        text: "2 numaralı dalganın bir üçgen oluşturması çok az görülen bir durumdur. Aynı şekilde, 4 ve 5 numaraları dalgalar da nadir bir biçimde üçgen olabilirler.",
      },
    ],
  },
  {
    id: "tez_2534_leading_heading",
    heading: "İlerleyen Diyagonaller (5-3-5-3-5)",
    paragraphs: [
      "İlerleyen diyagonallerin en kritik özelliği ve sonlanan diyagonallerden ayıran özelliği, trend ile aynı yönde bulunan dalgaların, yani 1 numaralı, 3 numaralı ve 5 numaralı dalgaların 5 ayrı dalgadan oluşmasıdır. Bu sebepten dolayı (5-3-5-3-5) sayımıyla ifade edilirler. Sonlanan diyagonallerden farklı olarak ilerleyen diyagonaller daha fazla 1 numaralı dalgada ya da A-B-C formasyon görünümünün A dalgasında oluşurlar. Bu diyagonallerde formasyon hareketini ilerlettikçe genel olarak trend daha zayıflar. Ayrıca, 5. dalganın fiyat değişim oranı 3. dalgaya oranla çok daha yavaş gerçekleşir. Kurallar ve rehber ilkeleri incelendiğinde (Baysal, 2011:113-115):",
    ],
    rulesHeading: "Kurallar",
    rules: [
      {
        id: "ld_r1",
        text: "3 numaralı dalga 1 ve 5 numaralı dalgalara oranla en kısa dalga olamaz.",
      },
      {
        id: "ld_r2",
        text: "4 numaralı dalga 2 numaralı dalganın fiyat bölgesine girer.",
      },
      {
        id: "ld_r3",
        text: "5 numaralı dalganın uzunluğu 4 numaralı dalganın en az 1,38 katı olmalıdır.",
      },
      {
        id: "ld_r4",
        text: "3 numaralı dalganın fiyat uzunluğu 2 numaralı dalganın fiyat uzunluğundan daha fazla olmalıdır.",
      },
      {
        id: "ld_r5",
        text: "2 ve 4 numaralı dalgalar kendilerinden önce gelen dalgaların tamamını asla örtemezler.",
      },
      {
        id: "ld_r6",
        text: "5 numaralı dalga olan itki dalgası ilerleyen veya sonlanan diyagonal bir yapı oluşturabilir. Buna benzer olarak, 2 ve 5 numaralı dalgalarda herhangi bir düzeltme dalgası görevi üstlenebilir. Benzer durum 1 ve 3 numaralı dalgalar için geçerli değildir. 1 ve 3 numaralı dalgalar bariz itki dalgaları olmalıdır (Baştaş, 2014: s.69-70).",
      },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [
      {
        id: "ld_g1",
        text: "2 ve 4 numaralı dalgalar birbirilerinin almaşığıdır. Eğer 2. dalga zigzag modelinde bir düzeltme gerçekleştiriyorsa 4 numaralı dalganın daha yatay bir biçimde düzeltme gerçekleştirmesi beklenir. Benzer durum tam tersi için de geçerlidir. Genellikle 2 numaralı dalga çok keskin, 4 numaralı dalga ise daha yatay bir düzeltmeye yatkınlık gösterir.",
      },
      {
        id: "ld_g2",
        text: "5 numaralı dalga çok fazla sık olmasa da bazı durumlarda kanalın üst bandından dışarı doğru sarkabilir. Bu sarkma oranı en fazla %15 olmalıdır.",
      },
      {
        id: "ld_g3",
        text: "5 numaralı dalganın boyu kendinden önce oluşan 4 numaralı dalganın en az 1.618 katı kadar olmalıdır.",
      },
      {
        id: "ld_g4",
        text: "2 numaralı dalga 1 numaralı dalganın çoğunlukla en az %23.6 oranını geri alır. Aynı durum 4 numaralı dalga için de geçerlidir.",
      },
      {
        id: "ld_g5",
        text: "Çizilen kanal bantları paralel olamaz. İki bandında aynı eğimde ilerlemesi gerekmektedir.",
      },
      {
        id: "ld_g6",
        text: "Formasyonun kanal bantları birbirileriyle kesişim gerçekleştirmeden bitmelidir (Baştaş, 2014: s.69-70).",
      },
    ],
  },
];

export const ELLIOTT_TEZ_254_SECTIONS: readonly ElliottTezSection[] = [
  {
    id: "tez_254_intro",
    heading: "2.5.4 Düzeltme Dalgaları",
    paragraphs: [
      "Düzeltme dalgaları fiyat trend hareketinin tam tersi yönünde gelişen dalga hareketleridir. Piyasada boğa ve ayı tüccarlarının gerçekleştirdiği mücadele sebebiyle, piyasa trend yönünde devam etme isteğinde bulunurken trendin aksi yönünde bir baskı oluşması ve hareketin trendin aksi yönünde gerçekleşmesi nedeniyle çeşitlilik arz ederler. Bu sebepten dolayı bir düzeltme dalgasını tespit etmek bir itki dalgasını tespit etmekten çok daha zordur.",
      "Düzeltme dalgalarının en kritik özelliklerinden biri hiçbir zaman bir itki dalgası gibi 5 dalgalı yapıda olmamalarıdır. 5 dalgalı bir yapı düzeltmenin kendisi değil ancak bir parçası olabilir. Düzeltme dalgaları her zaman 3 dalgalı yapılardan oluşurlar.",
    ],
    bullets: [
      "Yassı Düzeltmeler (3-3-5)",
      "Zigzag Düzeltmeler (5-3-5)",
      "Üçgen Düzeltmeler (3-3-3-3-3)",
      "Kombinasyonlu Düzeltmeler",
    ],
  },
  {
    id: "tez_2541_flat",
    heading: "2.5.4.1 Yassı Düzeltmeler",
    paragraphs: [
      "Yassı düzeltmeler farklı bir değişle yatay düzeltmeler olarak da adlandırılırlar. Bir yassı düzeltme ile bir sonraki alt başlıkta yer verilecek Zigzag düzeltmeleri birbirilerinden ayıran en önemli özellik, yassı düzeltmelerin (3-3-5) yapısında bulunmasıdır. Fiyat üzerinde trendi değiştirebilecek itki dalgasının güçsüz olması nedeniyle A düzeltme dalgası toplam 3 dalgadan oluşmaktadır. B dalgası ise A dalgasının tamamını geri almaktadır.",
      "Ana trendin tersi yönünde gerçekleşen bir dip ve tepe arasında ilerleyen yassı düzeltme dalgaları iki düzeltme ve bir itki dalgasından oluşmaktadır. Bir yassı düzeltme dalgasının oluşması genellikle güçlü bir itki dalgası ardından olur. Bir itki dalgasında genel olarak 3. dalga uzatma gerçekleştirdiği için bu düzeltmenin 4. dalgada görülebilme olasılığı 2. dalgaya oranla daha fazladır.",
      "Yassı düzeltmelerde B dalgasının bitiş noktası A dalgasının bitiş noktasına eşit ya da yakındır. C dalgasının bitiş noktası ise A dalgasının bitiş noktasına eşit ya da biraz miktar yukarıdadır (Frost ve Prechter, 2000:45).",
      "Yassı düzeltmeler hızlı yassı veya genişleyen yassı düzeltmeler olarak da ilerleme kaydedebilirler.",
      "Genişleyen yassı düzeltmelerin normal yassı düzeltmelerden farkı, B dalgasının A dalgasının başlangıç noktası üzerine çıkmasıdır. Diğer bir fark ise C dalgasının A dalgasının bitiş noktasının biraz altında bitmiş olmasıdır. Aşağıdaki şekillerde daha basit olarak görülebilmektedir (Baştaş, 2014:77).",
      "Hareketli yassı düzeltme dalgalarının diğer yassı düzeltmelerden farkı, B dalgasının bitiş noktasının A dalgasının başlangıç noktasının üzerinde olmasına karşın, C dalgasının bitiş noktasının A dalgasının bittiği noktaya gelemeden dalganın sona ermesidir. Nadir oluşan bir düzeltme dalgasıdır. Kurallar ve rehber ilkeleri incelendiğinde (Baştaş, 2014:77-78):",
    ],
    rulesHeading: "Kurallar",
    rules: [
      { id: "flat_r1", text: "A dalgası zigzag ya da yassı düzeltme dalgası olmalıdır." },
      { id: "flat_r2", text: "C dalgası ve A dalgası ortak bir fiyat alanına sahip olmalıdır." },
      { id: "flat_r3", text: "B dalgası yapı olarak herhangi bir düzeltme dalgası olmalıdır." },
      { id: "flat_r4", text: "B dalgası A dalgasının en az 0.382, en fazla 2.618 katı olabilir." },
      { id: "flat_r5", text: "C dalgası yapı bakımından her hangi bir itki dalgası görevini üstlenebilir." },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [
      {
        id: "flat_g1",
        text: "B dalgası yüksek ihtimal ile zigzag ya da üçgen düzeltme dalgasıdır. Nadir olarak da yassı düzeltme dalgası olabilir.",
      },
      {
        id: "flat_g2",
        text: "C dalgası genel olarak itki veya ilerleyen diyagonal olabilir. Nadir olarak da sonlanan diyagonal olur.",
      },
      { id: "flat_g3", text: "A dalgası genel olarak zigzag düzeltme dalgası olur." },
      {
        id: "flat_g4",
        text: "C dalgasının bitiş noktasının A dalgasının bittiği noktaya kadar ilerlemesi beklenir.",
      },
      {
        id: "flat_g5",
        text: "B dalgasının A dalgasının en az olarak 0.618, en fazla olarak 1.618 katı olması beklenir.",
      },
      {
        id: "flat_g6",
        text: "A, B ve C dalgalarının birbirlerine eşit olması gerekir ya da C dalgasının A dalgasının 1.618 katı olması gereklidir.",
      },
      { id: "flat_g7", text: "C dalgası en az A dalgasının 0.382 katı kadar olmalıdır." },
    ],
  },
  {
    id: "tez_2542_zigzag",
    heading: "2.5.4.2 Zigzag Düzeltmeler (5-3-5)",
    paragraphs: [
      "Zigzag düzeltme dalgaları analizi ve gözlemlenebilmesi en kolay olan temel düzeltme dalgalarıdır. Düzeltme dalga tiplerinin bir çoğunda zigzag düzeltme dalgaları gözlemlenebilir. Zigzag düzeltme dalgaları için en kritik özelliklerin başında genel olarak sert ya da hareketli piyasa koşullarında ortaya çıkmalarıdır.",
      "Zigzag düzeltmeler A B C dalgaları ile oluşan ana trendin tam tersi yönünde ilerleyen üç dalgalı yapılardır. Zigzag düzeltmeler iki itki dalgasından (A, C) ve bir düzeltme dalgasından (B) meydana gelirler. Basit bir zigzag düzeltme için dikkat edilmesi gereken en önemli husus, B dalgasının bitiş seviyesinin A dalgasının başlangıç seviyesinden daha aşağıda bulunmasıdır. Özetle, B dalgasının bitiş seviyesi A dalgasının başlangıç seviyesini geçemez. Kurallar ve rehber ilkeleri incelendiğinde (Frost ve Prechter, 2000:41-42):",
    ],
    rulesHeading: "Kurallar",
    rules: [
      { id: "zz_r1", text: "Zigzag düzeltmeler için C dalgası B dalgasından daha kısa olamaz." },
      {
        id: "zz_r2",
        text: "C dalgası asla ilerleyen bir diyagonal olamaz. İtki ya da sonlanan diyagonal olmalıdır.",
      },
      {
        id: "zz_r3",
        text: "A dalgası için, ayı marketi hakimse ilerleyen diyagonal olma istisnası dışında itki olmak zorundadır.",
      },
      { id: "zz_r4", text: "B dalgası herhangi bir düzeltme dalgası olabilir." },
      { id: "zz_r5", text: "B dalgası A dalgasının %61.8 oranından fazlasını asla geri alamaz." },
      {
        id: "zz_r6",
        text: "C dalgası bitiş noktası, A dalgasının bittiği noktayı geçmek zorundadır.",
      },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [
      {
        id: "zz_g1",
        text: "C dalgası ve A dalgası genel olarak birbirilerine eşit ilerlerler. Burada eşit olarak nitelendirilen ana durum tam olarak eşitlik değildir. Görmezden gelinebilecek bir oranda fark olsa dahi birlikte ilerlemektedir. Eşit olmadıkları durumda ise, C dalgası A dalgasının 0,618 ya da 1,618 katı olur.",
      },
      {
        id: "zz_g2",
        text: "C dalgası A dalgası ile karşılaştırıldığında daha kuvvetli ise, dalga yapısının basit zigzag değil, ikili ya da üçlü zigzag olarak devam etmesi beklenir.",
      },
      {
        id: "zz_g3",
        text: "C dalgası A dalgasının 1,618 katından daha uzun ise bu yapının büyük bir ihtimalle itki dalgası olması beklenir (Baştaş, 2014:73).",
      },
    ],
  },
  {
    id: "tez_2542_double_zigzag",
    heading: "İkili Zigzag Düzeltme",
    paragraphs: [
      "İlk zigzag düzeltme hareketinin başarılı olamaması durumunda ikinci bir zigzag düzeltme ortaya çıkabilir. Bu zigzag düzeltme dalgaları bir itki dalgasının gerçekleştirdiği uzatmaya benzer karakterde ilerleseler de onlara nazaran daha az görülürler. İkili zigzaglar W – X – Y harfleri ile tanımlanır ve gösterilirler. Kurallar ve rehber ilkeleri incelendiğinde (Baysal, 2011:121-122):",
    ],
    rulesHeading: "Kurallar",
    rules: [
      { id: "dzz_r1", text: "W dalgası asla üçgen olamaz." },
      {
        id: "dzz_r2",
        text: "X dalgası genişleyen üçgen haricinde bir düzeltme dalgası olmak zorundadır.",
      },
      {
        id: "dzz_r3",
        text: "X dalgası W ve Y dalgalarının her ikisinden de büyük ya da küçük olmak zorundadır. Birinden büyük diğerinden küçük ya da tam tersi olamaz.",
      },
      { id: "dzz_r4", text: "Ana trend dalgasının yönü W ile aynı olmalıdır." },
      {
        id: "dzz_r5",
        text: "Y dalgası genişleyen üçgen dışında bir düzeltme dalgası olmalıdır.",
      },
      {
        id: "dzz_r6",
        text: "X dalgası W dalgasının en az %23.6’sı, en fazla %261.8’i büyüklüğünde olmalıdır.",
      },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [{ id: "dzz_g1", text: "X dalgası her hangi bir zigzag düzeltmedir" }],
  },
  {
    id: "tez_2542_triple_zigzag",
    heading: "Üçlü Zigzag Düzeltme",
    paragraphs: [
      "Çok nadir görülen bir düzeltme tipi olmakla birlikte birbiri ardına ilerleyen üç zigzag dalgasından meydana gelirler. Üçlü zigzag düzeltmeler W, X, Y, X, Z harfleri ile gösterilirler.",
    ],
    rulesHeading: "Kurallar",
    rules: [
      {
        id: "tzz_r1",
        text: "Üçlü zigzag düzeltmelerin kuralları ikili zigzag düzeltmeler ile aynıdır. Farkı ise üçüncü bir zigzag düzeltme dalgasının oluşmasıdır.",
      },
    ],
  },
  {
    id: "tez_2543_triangle",
    heading: "2.5.4.3 Üçgen Düzeltmeler",
    paragraphs: [
      "Markette alıcı ve satıcılardan bir taraf diğeri üzerinde büyük bir üstünlük kuramadığı zaman fiyat genel olarak üçgen tipi formasyonlar şeklinde ilerler. Üçgen formasyonlarında hacim ve volatilite göreceli olarak düşük seyreder.",
      "Üçgen düzeltme dalgaları için diğer düzeltme dalgalarından en önemli farklılık 5 dalgalı yapıda ilerlemesidir. Her dalga bir alt dereceli olarak üç farklı düzeltme dalgasından oluşur. Bu sebepten dolayı, dalga yapısı (3-3-3-3-3) biçimini alır. Üçgen düzeltme dalgaları a – b – c – d – e harfleri ile tanımlanır ve gösterilirler.",
      "Üçgen düzeltmeler genel olarak itki dalgalarının bir alt dereceli 4. dalgalarında karşımıza çıkarlar. 2. dalganın bir üçgen düzeltme olarak ilerliyor olması çok az görülen bir durumdur.",
      "Üçgen dalgalar genişleyen ve daralan üçgenler olarak iki farklı başlıkta incelenir. Daralan üçgenleri ise kendi içinde simetrik üçgen, yükselen üçgen ve alçalan üçgen olarak üçe ayrılırlar.",
      "Genişleyen üçgenler Ralph Nelson Elliot tanımına göre “ters simetrik üçgen” olarak tanımlanmıştır ve çok daha az görülürler. Örnek şekillerle incelendiğinde (Frost ve Prechter, 2000:48-49):",
    ],
    rulesHeading: "Kurallar",
    rules: [
      {
        id: "tri_r1",
        text: "Üçgen görünümünü oluşturan dalgalardan ilk dört tanesi üçgenin dışında herhangi bir düzeltme formasyonu oluşturabilir. Son dalga ise üçgen de dahil olmak üzere herhangi bir düzeltme dalgası biçimindedir.",
      },
      {
        id: "tri_r2",
        text: "A ve C dalgalarının bitişi noktaları işe B ve D dalgalarının bitiş noktaları arasında çizilen çizgiler arasında hareket gerçekleşir. E dalgası ise bu çizgilerden maksimum %15 oranında bir sapma gerçekleştirebilir.",
      },
      {
        id: "tri_r3",
        text: "Düzeltme hareketinin tamamı alt ve üst bant çizgileri birbirilerini kesmeden önce sonlanmalıdır.",
      },
      {
        id: "tri_r4",
        text: "Çizgiler ya birbirilerine yakınsar ya da birbirilerinden uzaklaşır. Çizgiler asla birbirilerine paralel olmaz.",
      },
      {
        id: "tri_r5",
        text: "B dalgası A dalgasının en az %38.2, en fazla ise %161.8’i kadardır.",
      },
      {
        id: "tri_r6",
        text: "Genişleyen bir üçgenin E dalgası daralan üçgen olarak ilerleyebilir.",
      },
      {
        id: "tri_r7",
        text: "Genişleyen üçgen tiplerinde en kısa dalgalar A veya B dalgalarından biri olmalıdır.",
      },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [
      {
        id: "tri_g1",
        text: "A ve B dalgaları genel olarak birbirlerinin almaşıklarıdır. Özetle, A dalgası keskin bir düzeltme gerçekleştiriyorsa, B dalgasının daha yatay bir düzeltme gerçekleştirmesi beklenir.",
      },
      { id: "tri_g2", text: "Genişleyen üçgen tiplerine piyasalarda çok az rastlanmaktadır." },
      { id: "tri_g3", text: "Aynı trend ile ilerleyen dalgaların oranı genellikle %61.8’dir." },
      { id: "tri_g4", text: "B dalgası genel olarak zigzag düzeltmeler gerçekleştirir." },
      {
        id: "tri_g5",
        text: "Genişleyen bir üçgen tipiyle çok az karşı karşıya kalırız. Eğer bir genişleyen üçgen dalgası ile karşı karşıya kaldıysak oran genel olarak şöyledir: E dalgası C dalgasının, C dalgası A dalgasının, D dalgası da B dalgasının 1.618 katı kadardır (Baştaş, 2014:81-82).",
      },
    ],
  },
  {
    id: "tez_2544_combination",
    heading: "2.5.4.4 Kombinasyonlu Düzeltmeler",
    paragraphs: [
      "Ralph Nelson Elliott düzeltme olarak ilerleyen dalgaların yatay uzamalarını ikili ve üçlü üçlüler olarak isimlendirmiştir. Oldukça nadir karşılaşılan bu düzeltme tipleri iki ya da üç basit düzeltme formasyonun birleşmesiyle oluşurlar. Genellikle oluşan formasyonun ilk iki bölümü zigzag ya da yassı düzeltme, son bölümü ise üçgen düzeltme olarak görülür. Üçlü oluşturan ana dalgalar ise W – X – Y harfleri ile isimlendirilirler. (Baştaş, 2014: s.84)",
      "Formasyona ait olan düzeltme dalgası X herhangi bir düzeltme dalgası olabilir ancak genel olarak zigzag düzeltme dalgası olarak görev edinir (Baştaş, 2014: s.84).",
    ],
    rulesHeading: "Kurallar",
    rules: [
      {
        id: "comb_r1",
        text: "Kombinasyonlu düzeltmeler, iki ya da üç basit düzeltme formasyonunun birleşiminden oluşur; ilk parça(lar) genelde zigzag ya da yassı, son parça genelde üçgen şeklinde görülür (W–X–Y).",
      },
      {
        id: "comb_r2",
        text: "X dalgası herhangi bir düzeltme dalgası olabilir; pratikte çoğunlukla zigzag görevi üstlenir (W–X–Y).",
      },
      {
        id: "comb_r3",
        text: "Y dalgası, kombinasyonun son bölümü olarak genellikle üçgen düzeltme görevini üstlenir.",
      },
      {
        id: "comb_r4",
        text: "Almaşıklık/kontrast: W ve Y’nin yapısal karakteri (keskinlik/karmaşıklık) birbirinden farklı olmalıdır; bu sayede kombinasyon ayrışır.",
      },
      {
        id: "comb_r5",
        text: "Genişletilmiş kombinasyon (W–X–Y–X–Z / WXYXZ) adayları için W≈Y oran bandı aranır; Z bölümü uzatma (extension) bandında kalmalıdır.",
      },
      {
        id: "comb_r6",
        text: "WXYXZ adaylarında X’in retrace/bant kısıtları ile birlikte “post-B” bağlamı aranır; tüm koşullar sağlanırsa aday daha güvenli hale gelir.",
      },
    ],
    guidelinesHeading: "Rehber İlkeler",
    guidelines: [
      {
        id: "comb_g1",
        text: "X dalgasında genelde zigzag tercih edilse de bazı piyasa koşullarında flat/başka düzeltmeler de görülebilir; ancak son parça (Y) üçgen davranışına daha yakın olmalıdır.",
      },
      {
        id: "comb_g2",
        text: "Kombinasyon sayımında dalga ilerleyişi, ana trend yönünde “aşama aşama” ilerlemeyi bekler; bağlam (özellikle post-B bölgesi) önemli bir teyittir.",
      },
      {
        id: "comb_g3",
        text: "WXYXZ sınıflamasında candidate → confirmed ayrımı kural/teyit bileşenlerine göre yapılır; yalnızca yapısal benzerlik yeterli sayılmaz.",
      },
    ],
  },
];

export const ELLIOTT_TEZ_255_SECTIONS: readonly ElliottTezSection[] = [
  {
    id: "tez_255_app_formations",
    heading: "QTSS — Otomatik formasyon kapsamı (özet)",
    paragraphs: [
      "Bu paneldeki sayım motoru, tez §2.5.3–2.5.4 ile uyumlu olacak şekilde genişletilmiştir: ana beşlik itkide klasik kurallar; dalga 2 ve 4 (ve mümkünse itki sonrası ABC) için hem zigzag (5-3-5) hem yassı (3-3-5) adayları aranır ve skora göre biri seçilir. Mor tonlu a–b–c çizgisi yassı, turuncu zigzag seçimini gösterir.",
      "Bağlam satırları: itkiler 1-3-5 göreli uzunluğa göre “hangi dalga uzatma adayı” rehberi; §2.5.3.3 başarısız beşinci (5’in 3’ün ekstremini geçememesi); dalga 4 aralığında çok sayıda salınım varsa §2.5.4.3 üçgen düzeltme olasılığı uyarısı.",
      "Otomatik motor (V2): dalga 2/4 ve itki sonrası ABC için zigzag/yassı §2.5.4.1–2 sayısal kontrolleri (`elliottEngineV2/tezWaveChecks.ts`); üçgen için tri_r5 (§2.5.4.3); W–X–Y ve W–X–Y–X–Z kombinasyon adayları mevcut. Henüz tam otomatik sayım yapılmayan tez başlıkları (manuel okuma): sonlanan/ilerleyen diyagonal üçgenler (§2.5.3.4), genişleyen/hareketli yassı, ikili-üçlü zigzag W-X-Y alt bölünmesi. Tez §2.5.3.4–2.5.5 metinleri `elliottRulesCatalog.ts` içinde birleşiktir.",
    ],
  },
  {
    id: "tez_255_alternation",
    heading: "2.5.5 Almaşıklık İlkesi",
    paragraphs: [
      "Almaşıklık ilkesi varsayımına göre iki düzeltme dalgası birbirilerinden farklı yapılarda oluşmaktadırlar. Eğer düzeltme dalgalarından bir tanesi keskin bir düzeltme hareketi gerçekleştiriyorsa diğer düzeltme dalgasının daha basit bir düzeltme gerçekleştireceği varsayılmaktadır. Tersi olarak, bir dalga daha basit bir düzeltme gerçekleştiriyorsa diğeri daha keskin ya da karmaşık bir düzeltme gerçekleştirir. Almaşıklık varsayımı tam olarak buna dayanmaktadır.",
      "Almaşıklık ilkesi bir kural değil bir varsayımdır. Bir önceki dalga analiz edilerek bir sonraki dalganın nasıl oluşması gerektiği konusunda tahminde bulunulur. Doğru sonuç için bir garanti beklemek kesinlikle hata olacaktır. Glenn Neely “Mastering the Elliot Wave” isimli eserinde almaşıklık ilkesinin beş farklı biçimde gözlemlenebileceğini dile getirmiştir (Neely,1990:5):",
    ],
    bullets: [
      "Fiyat (Grafikte dikey olarak kat ettiği mesafe)",
      "Zaman (Grafikte yatay olarak kat ettiği mesafe)",
      "Şiddet (Yüzde olarak, kendinden önceki dalgayı geri alışı)",
      "Karmaşıklık (Bir kalıbın alt bölünme sayısı)",
      "Dalga Yapısı (Bir dalganın yassı, zigzag veya üçgen olması)",
    ],
  },
];

/** §2.5.3.4 – §2.5.5 — birleşik dizi (panel). */
export const ELLIOTT_TEZ_EXTENDED_SECTIONS: readonly ElliottTezSection[] = [
  ...ELLIOTT_TEZ_2534_SECTIONS,
  ...ELLIOTT_TEZ_254_SECTIONS,
  ...ELLIOTT_TEZ_255_SECTIONS,
];
