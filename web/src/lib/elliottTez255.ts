import type { ElliottTezSection } from "./elliottTezTypes";

export const ELLIOTT_TEZ_255_SECTIONS: readonly ElliottTezSection[] = [
  {
    id: "tez_255_app_formations",
    heading: "QTSS — Otomatik formasyon kapsamı (özet)",
    paragraphs: [
      "Bu paneldeki sayım motoru, tez §2.5.3–2.5.4 ile uyumlu olacak şekilde genişletilmiştir: ana beşlik itkide klasik kurallar; dalga 2 ve 4 (ve mümkünse itki sonrası ABC) için hem zigzag (5-3-5) hem yassı (3-3-5) adayları aranır ve skora göre biri seçilir. Mor tonlu a–b–c çizgisi yassı, turuncu zigzag seçimini gösterir.",
      "Bağlam satırları: itkiler 1-3-5 göreli uzunluğa göre “hangi dalga uzatma adayı” rehberi; §2.5.3.3 başarısız beşinci (5’in 3’ün ekstremini geçememesi); dalga 4 aralığında çok sayıda salınım varsa §2.5.4.3 üçgen düzeltme olasılığı uyarısı.",
      "Otomatik motor (V2): dalga 2/4 ve itki sonrası ABC için zigzag/yassı §2.5.4.1–2 sayısal kontrolleri (`elliottEngineV2/tezWaveChecks.ts`); üçgen için tri_r5 (§2.5.4.3); W–X–Y ve W–X–Y–X–Z kombinasyon adayları mevcut. Henüz tam otomatik sayım yapılmayan tez başlıkları (manuel okuma): sonlanan/ilerleyen diyagonal üçgenler (§2.5.3.4), genişleyen/hareketli yassı, ikili-üçlü zigzag W-X-Y alt bölünmesi. Tez metinleri `elliottTez2534.ts`, `elliottTez254.ts`, `elliottTez255.ts` dosyalarında korunur.",
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
