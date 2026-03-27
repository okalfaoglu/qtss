import type { ElliottTezSection } from "./elliottTezTypes";

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
