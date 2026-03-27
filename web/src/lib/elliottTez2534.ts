import type { ElliottTezSection } from "./elliottTezTypes";

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
