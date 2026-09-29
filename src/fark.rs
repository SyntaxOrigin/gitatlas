//! Ağaç karşılaştırması ve satır bazlı unified diff.
//!
//! İki katman vardır:
//!
//! 1. **Ağaç farkı:** iki commit'in ağaçlarını özyinelemeli karşılaştırıp değişen yolları
//!    listeler. Alt modüller (gitlink) içeriği okunmaz, yalnızca adıyla listelenir.
//! 2. **Satır farkı:** iki blob içeriği arasında satır bazlı unified diff üretilir.
//!    Algoritma **Myers'in O(ND) en kısa düzenleme yolu** yöntemidir; ortak ön/son
//!    ekleri ayıklayıp kalan orta blok üzerinde çalışır.
//!
//! **Dürüstlük notu:** Myers yalnızca `D` (toplam düzenleme uzaklığı) sınırına kadar
//! ilerler. Aşırı farklı iki büyük metin bloğunda `D` tavanı aşılır ve araç **sessizce**
//! yanlış bir minimal fark üretmez; bunun yerine tüm bloğu "sil + ekle" olarak işaretler
//! ve `kaba: true` bildirir. Bu, E-004'ün "sessizce yanlış veri" yasağına uyar.

use crate::agac::agac_girdileri;
use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::oid::Oid;

/// Myers yürüyüşünün azami düzenleme uzaklığı.
///
/// Bu tavanın bellek maliyeti `D × (2D+3) × 8` bayttır (≈ 5,8 MB). Üst sınır
/// bilinçlidir: 260 MB tepe RSS bütçesinde iz izi büyükse daha büyük `D` tavanı
/// tüm bütçeyi tek bir diff çağrısına harcardı.
const EN_COK_D: usize = 600;

/// İkili (metin olmayan) dosya sayılma eşiği: ilk N baytta NUL aranır.
///
/// Git'in kendi eşiği de 8000 bayttır. Uyarı metinlerinin ilk 8000 baytta NUL
/// içermesi neredeyse imkânsızdır, bu yüzden eşik seçicinin işini görür.
const IKILI_ESIK: usize = 8000;

/// Bir yolun iki ağaç arasındaki durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegisimTuru {
    /// Yalnızca yeni ağaçta var.
    Eklendi,
    /// Yalnızca eski ağaçta var.
    Silindi,
    /// İki ağaçta da var ama nesne adı farklı.
    Degisti,
    /// Aynı nesne adı ama dosya modu (ör. izin biti) değişmiş.
    ModDegisti,
}

impl DegisimTuru {
    /// Unified/JSON çıktısında kullanılan tek harfli gösterge.
    pub fn isaret(&self) -> char {
        match self {
            DegisimTuru::Eklendi => 'A',
            DegisimTuru::Silindi => 'D',
            DegisimTuru::Degisti => 'M',
            DegisimTuru::ModDegisti => 'T',
        }
    }

    /// Türkçe açıklama.
    pub fn ad(&self) -> &'static str {
        match self {
            DegisimTuru::Eklendi => "eklendi",
            DegisimTuru::Silindi => "silindi",
            DegisimTuru::Degisti => "degisti",
            DegisimTuru::ModDegisti => "mod degisti",
        }
    }
}

/// Ağaç karşılaştırmasından çıkan tek bir yol farkı.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct YolFarki {
    /// Depo içi tam yol (`src/lib.rs`).
    pub yol: String,
    /// Değişim türü.
    pub tur: DegisimTuru,
    /// Eski taraftaki nesne (yoksa `None`).
    pub eski: Option<Oid>,
    /// Yeni taraftaki nesne (yoksa `None`).
    pub yeni: Option<Oid>,
    /// Eski taraftaki mod.
    pub eski_mod: Option<String>,
    /// Yeni taraftaki mod.
    pub yeni_mod: Option<String>,
    /// Değişen yol bir alt modül mü?
    pub modul: bool,
}

/// İki ağacı karşılaştırıp değişen yolları yol sırasına göre döndürür.
///
/// `eski` verilmezse (kök commit) boş ağaç karşılaştırılır, yani her şey "eklendi"
/// olarak listelenir.
pub fn agac_karsilastir(
    magaza: &Magaza,
    eski: Option<&Oid>,
    yeni: &Oid,
) -> Result<Vec<YolFarki>, Hata> {
    let mut cikti = Vec::new();
    match eski {
        Some(eski) => karsilastir(magaza, eski, yeni, "", &mut cikti)?,
        None => topla_hepsini(magaza, yeni, "", &mut cikti)?,
    }
    cikti.sort_by(|a, b| a.yol.cmp(&b.yol));
    Ok(cikti)
}

fn karsilastir(
    magaza: &Magaza,
    eski_agac: &Oid,
    yeni_agac: &Oid,
    on: &str,
    cikti: &mut Vec<YolFarki>,
) -> Result<(), Hata> {
    if eski_agac == yeni_agac {
        return Ok(());
    }
    let eski_girdiler = agac_girdileri(magaza, eski_agac)?;
    let yeni_girdiler = agac_girdileri(magaza, yeni_agac)?;
    let eski_harita = crate::agac::harita(&eski_girdiler);
    let yeni_harita = crate::agac::harita(&yeni_girdiler);

    for (ad, yeni_giris) in &yeni_harita {
        let tam = if on.is_empty() {
            ad.to_string()
        } else {
            format!("{on}/{ad}")
        };
        match eski_harita.get(ad) {
            None => topla(magaza, yeni_giris, &tam, cikti)?,
            Some(eski_giris)
                if eski_giris.oid == yeni_giris.oid
                    && eski_giris.mod_bilgi == yeni_giris.mod_bilgi => {}
            Some(eski_giris) if eski_giris.agac_mi() && yeni_giris.agac_mi() => {
                karsilastir(magaza, &eski_giris.oid, &yeni_giris.oid, &tam, cikti)?;
            }
            Some(eski_giris) => cikti.push(YolFarki {
                yol: tam,
                tur: if eski_giris.oid == yeni_giris.oid {
                    DegisimTuru::ModDegisti
                } else {
                    DegisimTuru::Degisti
                },
                eski: Some(eski_giris.oid),
                yeni: Some(yeni_giris.oid),
                eski_mod: Some(eski_giris.mod_bilgi.clone()),
                yeni_mod: Some(yeni_giris.mod_bilgi.clone()),
                modul: eski_giris.modul_mi() || yeni_giris.modul_mi(),
            }),
        }
    }

    for (ad, eski_giris) in &eski_harita {
        if yeni_harita.contains_key(ad) {
            continue;
        }
        let tam = if on.is_empty() {
            ad.to_string()
        } else {
            format!("{on}/{ad}")
        };
        if eski_giris.modul_mi() {
            cikti.push(YolFarki {
                yol: tam,
                tur: DegisimTuru::Silindi,
                eski: Some(eski_giris.oid),
                yeni: None,
                eski_mod: Some(eski_giris.mod_bilgi.clone()),
                yeni_mod: None,
                modul: true,
            });
            continue;
        }
        if eski_giris.agac_mi() {
            topla_hepsini(magaza, &eski_giris.oid, &tam, cikti)?;
        } else {
            cikti.push(YolFarki {
                yol: tam,
                tur: DegisimTuru::Silindi,
                eski: Some(eski_giris.oid),
                yeni: None,
                eski_mod: Some(eski_giris.mod_bilgi.clone()),
                yeni_mod: None,
                modul: false,
            });
        }
    }
    Ok(())
}

fn topla_hepsini(
    magaza: &Magaza,
    agac: &Oid,
    on: &str,
    cikti: &mut Vec<YolFarki>,
) -> Result<(), Hata> {
    for (_, giris) in crate::agac::harita(&agac_girdileri(magaza, agac)?) {
        let tam = if on.is_empty() {
            giris.ad.clone()
        } else {
            format!("{on}/{}", giris.ad)
        };
        topla(magaza, giris, &tam, cikti)?;
    }
    Ok(())
}

fn topla(
    magaza: &Magaza,
    giris: &crate::agac::AgacGirdi,
    tam: &str,
    cikti: &mut Vec<YolFarki>,
) -> Result<(), Hata> {
    if giris.agac_mi() {
        return topla_hepsini(magaza, &giris.oid, tam, cikti);
    }
    cikti.push(YolFarki {
        yol: tam.to_string(),
        tur: DegisimTuru::Eklendi,
        eski: None,
        yeni: Some(giris.oid),
        eski_mod: None,
        yeni_mod: Some(giris.mod_bilgi.clone()),
        modul: giris.modul_mi(),
    });
    Ok(())
}

/// Bir blob içeriğinin ikili (metin olmayan) sayılıp sayılmayacağını bildirir.
pub fn ikili_mi(veri: &[u8]) -> bool {
    let kesit = &veri[..veri.len().min(IKILI_ESIK)];
    kesit.contains(&0)
}

/// İçeriği satırlara böler. Sondaki `\n` bir satır sayılmaz (git'in davranışı).
pub fn satirlara_bol(veri: &[u8]) -> Vec<String> {
    let metin = String::from_utf8_lossy(veri);
    let mut satirlar: Vec<String> = metin.split('\n').map(|s| s.to_string()).collect();
    if satirlar.last().map(|s| s.is_empty()).unwrap_or(false) {
        satirlar.pop();
    }
    satirlar
}

/// Tek bir satırın diff gösterimi.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SatirIslem {
    /// Değişmeyen satır.
    Esit,
    /// Silinen satır.
    Silinen,
    /// Eklenen satır.
    Eklenen,
}

impl SatirIslem {
    /// Unified diff ön eki (` `, `-`, `+`).
    pub fn onek(&self) -> char {
        match self {
            SatirIslem::Esit => ' ',
            SatirIslem::Silinen => '-',
            SatirIslem::Eklenen => '+',
        }
    }
}

/// Unified diff çıktısındaki tek bir hunk.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    /// Eski dosyada hunk'un başladığı satır numarası (1 tabanlı).
    pub eski_baslangic: usize,
    /// Yeni dosyada hunk'un başladığı satır numarası (1 tabanlı).
    pub yeni_baslangic: usize,
    /// Satır içerikleri.
    pub satirlar: Vec<(SatirIslem, String)>,
}

impl Hunk {
    /// Hunk içindeki eklenen satır sayısı.
    pub fn eklenen(&self) -> usize {
        self.satirlar
            .iter()
            .filter(|(islem, _)| *islem == SatirIslem::Eklenen)
            .count()
    }

    /// Hunk içindeki silinen satır sayısı.
    pub fn silinen(&self) -> usize {
        self.satirlar
            .iter()
            .filter(|(islem, _)| *islem == SatirIslem::Silinen)
            .count()
    }

    /// Unified diff başlığında kullanılan **toplam** eski satır sayısı (bağlam dâhil).
    pub fn eski_satir(&self) -> usize {
        self.satirlar
            .iter()
            .filter(|(islem, _)| *islem != SatirIslem::Eklenen)
            .count()
    }

    /// Unified diff başlığında kullanılan **toplam** yeni satır sayısı (bağlam dâhil).
    pub fn yeni_satir(&self) -> usize {
        self.satirlar
            .iter()
            .filter(|(islem, _)| *islem != SatirIslem::Silinen)
            .count()
    }
}

/// İki metin arasındaki satır farkı.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SatirFarki {
    /// Hunk listesi (yoksa boş).
    pub hunks: Vec<Hunk>,
    /// Eklenen toplam satır sayısı.
    pub eklenen: usize,
    /// Silinen toplam satır sayısı.
    pub silinen: usize,
    /// Myers tavanı aşıldığı için "tümünü sil + tümünü ekle" kaba farkı üretildiyse `true`.
    ///
    /// Bu alan çıktıda görünür; "minimal fark" iddiası sadece `false` olduğunda geçerlidir.
    pub kaba: bool,
}

/// İki metin arasında satır bazlı unified diff üretir.
pub fn satir_farki(eski: &[u8], yeni: &[u8], baglam: usize) -> SatirFarki {
    let eski_satirlar = satirlara_bol(eski);
    let yeni_satirlar = satirlara_bol(yeni);

    // Ortak ön ek / son ek, Myers'in çalışacağı bloğu küçültür.
    let mut on = 0usize;
    while on < eski_satirlar.len()
        && on < yeni_satirlar.len()
        && eski_satirlar[on] == yeni_satirlar[on]
    {
        on += 1;
    }
    let mut son_eski = eski_satirlar.len();
    let mut son_yeni = yeni_satirlar.len();
    while son_eski > on
        && son_yeni > on
        && eski_satirlar[son_eski - 1] == yeni_satirlar[son_yeni - 1]
    {
        son_eski -= 1;
        son_yeni -= 1;
    }

    let orta_eski = &eski_satirlar[on..son_eski];
    let orta_yeni = &yeni_satirlar[on..son_yeni];

    let (islemler, kaba) = match myers(orta_eski, orta_yeni) {
        Some(islemler) => (islemler, false),
        None => (kaba_degisimler(orta_eski.len(), orta_yeni.len()), true),
    };

    let mutasyon = mutasyon_olustur(
        &eski_satirlar,
        on,
        son_eski,
        orta_eski,
        orta_yeni,
        &islemler,
    );
    let hunks = hunks_olustur(&mutasyon, baglam);
    let eklenen = hunks.iter().map(Hunk::eklenen).sum();
    let silinen = hunks.iter().map(Hunk::silinen).sum();
    SatirFarki {
        hunks,
        eklenen,
        silinen,
        kaba,
    }
}

/// Tam dosyanın satır dizisini `(işlem, içerik)` çiftlerine çevirir.
fn mutasyon_olustur(
    eski_satirlar: &[String],
    on: usize,
    son_eski: usize,
    orta_eski: &[String],
    orta_yeni: &[String],
    islemler: &[SatirIslem],
) -> Vec<(SatirIslem, String)> {
    let mut mutasyon: Vec<(SatirIslem, String)> = Vec::new();
    for satir in &eski_satirlar[..on] {
        mutasyon.push((SatirIslem::Esit, satir.clone()));
    }
    let mut eski_imlec = 0usize;
    let mut yeni_imlec = 0usize;
    for islem in islemler {
        match islem {
            SatirIslem::Esit => {
                mutasyon.push((SatirIslem::Esit, orta_eski[eski_imlec].clone()));
                eski_imlec += 1;
                yeni_imlec += 1;
            }
            SatirIslem::Silinen => {
                mutasyon.push((SatirIslem::Silinen, orta_eski[eski_imlec].clone()));
                eski_imlec += 1;
            }
            SatirIslem::Eklenen => {
                mutasyon.push((SatirIslem::Eklenen, orta_yeni[yeni_imlec].clone()));
                yeni_imlec += 1;
            }
        }
    }
    for satir in &eski_satirlar[son_eski..] {
        mutasyon.push((SatirIslem::Esit, satir.clone()));
    }
    mutasyon
}

/// İki metin bloğu arasındaki düzenleme sırasını üretir.
///
/// `D` tavanı aşılırsa `None` döner; çağıran kaba "sil + ekle" farkına düşer.
fn myers(eski: &[String], yeni: &[String]) -> Option<Vec<SatirIslem>> {
    let n = eski.len();
    let m = yeni.len();
    if n == 0 && m == 0 {
        return Some(Vec::new());
    }
    if n == 0 {
        return Some(vec![SatirIslem::Eklenen; m]);
    }
    if m == 0 {
        return Some(vec![SatirIslem::Silinen; n]);
    }

    let azami = EN_COK_D.min(n + m);
    let ofset = azami as isize + 1;
    let indeks = |k: isize| (k + ofset) as usize;
    let mut v = vec![0isize; 2 * azami + 3];
    let mut izlem: Vec<Vec<isize>> = Vec::new();

    for d in 0..=azami {
        let mut k = -(d as isize);
        while k <= d as isize {
            let mut x =
                if k == -(d as isize) || (k != d as isize && v[indeks(k - 1)] < v[indeks(k + 1)]) {
                    v[indeks(k + 1)]
                } else {
                    v[indeks(k - 1)] + 1
                };
            let mut y = x - k;
            while (x as usize) < n && (y as usize) < m && eski[x as usize] == yeni[y as usize] {
                x += 1;
                y += 1;
            }
            v[indeks(k)] = x;
            if x as usize >= n && y as usize >= m {
                izlem.push(v.clone());
                return Some(geri_yur(&izlem, ofset, n, m));
            }
            k += 2;
        }
        izlem.push(v.clone());
    }
    None
}

fn kaba_degisimler(n: usize, m: usize) -> Vec<SatirIslem> {
    let mut islemler = vec![SatirIslem::Silinen; n];
    islemler.extend(vec![SatirIslem::Eklenen; m]);
    islemler
}

fn geri_yur(izlem: &[Vec<isize>], ofset: isize, n: usize, m: usize) -> Vec<SatirIslem> {
    let indeks = |k: isize| (k + ofset) as usize;
    let mut sonuc: Vec<SatirIslem> = Vec::new();
    let mut x = n as isize;
    let mut y = m as isize;

    for d in (1..izlem.len()).rev() {
        let v = &izlem[d];
        let k = x - y;
        let onceki_k =
            if k == -(d as isize) || (k != d as isize && v[indeks(k - 1)] < v[indeks(k + 1)]) {
                k + 1
            } else {
                k - 1
            };
        let onceki_x = v[indeks(onceki_k)];
        let onceki_y = onceki_x - onceki_k;
        while x > onceki_x && y > onceki_y {
            sonuc.push(SatirIslem::Esit);
            x -= 1;
            y -= 1;
        }
        if x == onceki_x {
            sonuc.push(SatirIslem::Eklenen);
            y -= 1;
        } else {
            sonuc.push(SatirIslem::Silinen);
            x -= 1;
        }
    }
    sonuc.reverse();
    sonuc
}

/// Değişiklik satırlarının ±`baglam` kadarını birleştirerek hunk listesi üretir.
fn hunks_olustur(mutasyon: &[(SatirIslem, String)], baglam: usize) -> Vec<Hunk> {
    let degisenler: Vec<usize> = mutasyon
        .iter()
        .enumerate()
        .filter(|(_, (islem, _))| *islem != SatirIslem::Esit)
        .map(|(i, _)| i)
        .collect();
    if degisenler.is_empty() {
        return Vec::new();
    }

    let mut araliklar: Vec<(usize, usize)> = Vec::new();
    for &i in &degisenler {
        let bas = i.saturating_sub(baglam);
        let bit = (i + baglam + 1).min(mutasyon.len());
        match araliklar.last_mut() {
            Some((_, son)) if bas <= *son => *son = (*son).max(bit),
            _ => araliklar.push((bas, bit)),
        }
    }

    araliklar
        .into_iter()
        .map(|(bas, bit)| Hunk {
            eski_baslangic: eski_satir_sayisi(&mutasyon[..bas]) + 1,
            yeni_baslangic: yeni_satir_sayisi(&mutasyon[..bas]) + 1,
            satirlar: mutasyon[bas..bit].to_vec(),
        })
        .collect()
}

fn eski_satir_sayisi(bolum: &[(SatirIslem, String)]) -> usize {
    bolum
        .iter()
        .filter(|(islem, _)| *islem != SatirIslem::Eklenen)
        .count()
}

fn yeni_satir_sayisi(bolum: &[(SatirIslem, String)]) -> usize {
    bolum
        .iter()
        .filter(|(islem, _)| *islem != SatirIslem::Silinen)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metin(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    fn fark(eski: &str, yeni: &str) -> SatirFarki {
        satir_farki(&metin(eski), &metin(yeni), 3)
    }

    #[test]
    fn satirlara_bol_sondaki_satiri_atar() {
        assert_eq!(satirlara_bol(b"a\nb\n"), vec!["a", "b"]);
        assert_eq!(satirlara_bol(b"a\nb"), vec!["a", "b"]);
        assert_eq!(satirlara_bol(b""), Vec::<String>::new());
    }

    #[test]
    fn ikili_tespiti_nul_ve_utf8() {
        assert!(ikili_mi(&[0x41, 0x00, 0x42]));
        assert!(!ikili_mi(b"duz metin"));
    }

    #[test]
    fn birebir_ayni_metinde_hunk_yoktur() {
        let sonuc = fark("a\nb\nc\n", "a\nb\nc\n");
        assert!(sonuc.hunks.is_empty());
        assert_eq!(sonuc.eklenen, 0);
        assert_eq!(sonuc.silinen, 0);
    }

    #[test]
    fn tek_satirlik_degisiklik_hunk_uretir() {
        let sonuc = fark("a\nb\nc\n", "a\nX\nc\n");
        assert_eq!(sonuc.hunks.len(), 1);
        assert_eq!(sonuc.eklenen, 1);
        assert_eq!(sonuc.silinen, 1);
        assert!(!sonuc.kaba);
    }

    #[test]
    fn ekleme_hunk_uretir() {
        let sonuc = fark("a\nb\n", "a\nb\nc\n");
        assert_eq!(sonuc.eklenen, 1);
        assert_eq!(sonuc.silinen, 0);
    }

    #[test]
    fn silme_hunk_uretir() {
        let sonuc = fark("a\nb\nc\n", "a\nc\n");
        assert_eq!(sonuc.silinen, 1);
        assert_eq!(sonuc.eklenen, 0);
    }

    #[test]
    fn baglam_sayisi_hunk_boyutunu_etkiler() {
        let eski = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let yeni = "1\n2\n3\n4\n5\n6\n7\n8\n9\nX\n";
        let az = satir_farki(&metin(eski), &metin(yeni), 0);
        let cok = satir_farki(&metin(eski), &metin(yeni), 4);
        assert!(cok.hunks[0].satirlar.len() > az.hunks[0].satirlar.len());
    }

    #[test]
    fn ayni_satirlarin_sirasi_degisse_satir_tasiyinca_gorunur() {
        let sonuc = fark("a\nb\nc\n", "c\na\nb\n");
        assert_eq!(sonuc.eklenen, 1);
        assert_eq!(sonuc.silinen, 1);
    }

    #[test]
    fn kaba_isaret_alani_kalici_dogrudan_hazir_degildir() {
        let sonuc = fark("a\n", "b\n");
        assert!(!sonuc.kaba);
        assert_eq!(sonuc.eklenen, 1);
    }

    #[test]
    fn hunk_basliginda_baglam_satirlari_sayilir() {
        // Git'in `@@ -1,6 +1,6 @@` başlığı bağlam satırlarını da sayar; yalnızca
        // değişen satırları sayan bir başlık standart `patch` ile uyumsuzdur.
        let eski = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let yeni = "1\n2\nX\n4\n5\n6\n7\n8\n9\n10\n";
        let sonuc = fark(eski, yeni);
        assert_eq!(sonuc.hunks.len(), 1);
        let h = &sonuc.hunks[0];
        assert_eq!(h.eski_baslangic, 1);
        assert_eq!(h.yeni_baslangic, 1);
        assert_eq!(h.eklenen(), 1);
        assert_eq!(h.silinen(), 1);
        // 3 satır bağlam + 1 değişen = 6 satır; git de `@@ -1,6 +1,6 @@` yazar.
        assert_eq!(h.eski_satir(), 6);
        assert_eq!(h.yeni_satir(), 6);
    }

    #[test]
    fn degisim_turu_isaretleri() {
        assert_eq!(DegisimTuru::Eklendi.isaret(), 'A');
        assert_eq!(DegisimTuru::Silindi.isaret(), 'D');
        assert_eq!(DegisimTuru::Degisti.isaret(), 'M');
        assert_eq!(DegisimTuru::ModDegisti.isaret(), 'T');
    }
}
