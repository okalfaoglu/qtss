# Makina sinyalleri — grup / operasyon kuralları (not)

Bu belge, paylaşılan ekran görüntüsündeki **ürün ve iletişim kurallarının** özetidir; QTSS koduna bire bir bağlı değildir. Uygulama veya skor mantığı değişince burası güncellenmelidir.

---

## 1. Genel amaç

Makina tarafından üretilen **kısa vadeli** pozisyon önerileri bu grupta paylaşılır.

---

## 2. Süreç

| Aşama | İçerik |
|--------|--------|
| **Kurulum (setup)** | Giriş seviyesi, stop ve hedef (target) içerir. |
| **Güncelleme** | Fiyat **%3 veya daha fazla** hareket ettiğinde veya **stop seviyesi değiştiğinde** tetiklenir. |
| **Kapanış** | Sinyalin tamamlanması. |

---

## 3. Ortalama pozisyon süresi

**Birkaç gün — birkaç hafta** aralığında değerlendirilir.

---

## 4. Poz koruma (position protection)

- Sistem (makina), stop seviyesinin altında **ek bir tampon** önererek ani fitil ve piyasa gürültüsüne karşı koruma hedefler.
- Amaç: pozisyona **nefes alanı** bırakmak.

---

## 5. Puanlama (güç / TSK benzeri skor)

| Skor | Anlam |
|------|--------|
| **10 — 7** | Güçlü; paylaşılır. |
| **6** | Orta; değerlendirilir. |
| **5 — 4** | Zayıf. |
| **3 ve altı** | Kritik seviye; pozisyondan **uzak durulmalıdır**. |

---

## 6. Risk-off dönemleri

- Risk-off ortamında makina **T-Analiz** üretimini durdurur veya **çok daha seçici** çalışır.
- Bu dönemlerde yalnızca **yüksek güçlü yapılar** değerlendirmeye alınır.

---

## İlgili teknik not

Motor tarafındaki anlık skor üretimi (`signal_dashboard`, `pozisyon_gucu_10` vb.) bu tabloyla **bire bir aynı olmak zorunda değildir**; bu dosya ürün/operasyon sözleşmesi niteliğindedir. Hizalama isteniyorsa `crates/qtss-chart-patterns/src/dashboard_v1.rs` içindeki puanlama ile karşılaştırılmalıdır.
