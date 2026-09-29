//! `.idx` v2 dosyasının ayrıştırılması ve `sha1 → offset` sorgusu.
//!
//! Yalnızca **sürüm 2** okunur. Sürüm 1 ayrı bir biçimdir (başlıksız, girdiler
//! `ofs(4) + sha(20)` çiftlerinden oluşur) ve bilinçli olarak reddedilir; sessizce
//! yanlış offset üretmek, açık bir hatadan daha kötüdür.
//!
//! Dosya düzeni (`gitformat-pack(5)`):
//!
//! ```text
//! 4 bayt   sihirli  \377 t O c
//! 4 bayt   sürüm    = 2
//! 1024     fanout    256 × 4 bayt, büyük endian u32
//! 20·n     nesne adları (sıralı)
//! 4·n      CRC-32 değerleri
//! 4·n      ofsetler (MSB set ise büyük offset tablosuna işaret eder)
//! 8·m      büyük offset tablosu
//! 20       paket özeti
//! 20       indeks özeti (dosyanın geri kalanının SHA-1'i)
//! ```

use std::path::{Path, PathBuf};

use crate::hata::Hata;
use crate::oid::Oid;

/// `.idx` v2 sihirli değeri: `\xff t O c`.
const SIHRIRLI: [u8; 4] = [0xff, b't', b'O', b'c'];

/// `.idx` v2 başlık uzunluğu (sihirli + sürüm).
const BASLIK: usize = 8;

/// Fanout tablosunun bayt cinsinden uzunluğu (256 × 4 bayt).
const FANOUT: usize = 1024;

/// Bir paket dosyasının nesne dizini.
#[derive(Debug)]
pub struct PaketIndeksi {
    yol: PathBuf,
    shalar: Vec<[u8; 20]>,
    ofsetler: Vec<u64>,
    crc_ler: Vec<u32>,
    paket_ozeti: [u8; 20],
    indeks_ozeti: [u8; 20],
}

fn bozuk(yol: &Path, ayrinti: impl Into<String>) -> Hata {
    Hata::IndeksBozuk {
        yol: yol.to_path_buf(),
        ayrinti: ayrinti.into(),
    }
}

impl PaketIndeksi {
    /// `.idx` dosyasını okur ve yapısını doğrular.
    pub fn ac(yol: &Path) -> Result<Self, Hata> {
        let veri = std::fs::read(yol).map_err(|hata| Hata::io(yol, hata))?;
        Self::ayikla(yol, &veri)
    }

    /// Ham `.idx` baytlarından dizini üretir (dosya okumadan; test fikstürleri için).
    pub fn ayikla(yol: &Path, veri: &[u8]) -> Result<Self, Hata> {
        if veri.len() < BASLIK + FANOUT {
            return Err(bozuk(yol, format!("dosya çok kısa ({} bayt)", veri.len())));
        }

        if veri[..4] != SIHRIRLI {
            return Err(Hata::IndeksSurumuDesteklenmiyor {
                yol: yol.to_path_buf(),
                surum: "v1 veya bilinmeyen imza; yalnızca v2 okunur".to_string(),
            });
        }
        let surum = u32::from_be_bytes([veri[4], veri[5], veri[6], veri[7]]);
        if surum != 2 {
            return Err(Hata::IndeksSurumuDesteklenmiyor {
                yol: yol.to_path_buf(),
                surum: format!("v{surum}"),
            });
        }

        let mut fanout = [0u32; 256];
        for i in 0..256 {
            let t = BASLIK + i * 4;
            let deger = u32::from_be_bytes([veri[t], veri[t + 1], veri[t + 2], veri[t + 3]]);
            if i > 0 && deger < fanout[i - 1] {
                return Err(bozuk(
                    yol,
                    format!(
                        "fanout azalan: [{i}] = {deger}, [{}] = {}",
                        i - 1,
                        fanout[i - 1]
                    ),
                ));
            }
            fanout[i] = deger;
        }
        let sayi = fanout[255] as usize;

        let kisa_tablo = BASLIK + FANOUT;
        let gereken = kisa_tablo
            .checked_add(sayi * 20)
            .and_then(|v| v.checked_add(sayi * 4))
            .and_then(|v| v.checked_add(sayi * 4))
            .and_then(|v| v.checked_add(40))
            .ok_or_else(|| bozuk(yol, "nesne sayısı taşma yarattı"))?;
        if veri.len() < gereken {
            return Err(bozuk(
                yol,
                format!(
                    "{sayi} nesne için {gereken} bayt gerekir, dosyada {} bayt var",
                    veri.len()
                ),
            ));
        }

        let shalar_tab = kisa_tablo;
        let crc_tab = shalar_tab + sayi * 20;
        let ofset_tab = crc_tab + sayi * 4;
        let buyuk_tab = ofset_tab + sayi * 4;

        let mut shalar = Vec::with_capacity(sayi);
        for i in 0..sayi {
            let t = shalar_tab + i * 20;
            let mut sha = [0u8; 20];
            sha.copy_from_slice(&veri[t..t + 20]);
            shalar.push(sha);
        }
        let mut crc_ler = Vec::with_capacity(sayi);
        for i in 0..sayi {
            let t = crc_tab + i * 4;
            crc_ler.push(u32::from_be_bytes([
                veri[t],
                veri[t + 1],
                veri[t + 2],
                veri[t + 3],
            ]));
        }
        let mut ofset_kisa = Vec::with_capacity(sayi);
        for i in 0..sayi {
            let t = ofset_tab + i * 4;
            ofset_kisa.push(u32::from_be_bytes([
                veri[t],
                veri[t + 1],
                veri[t + 2],
                veri[t + 3],
            ]));
        }

        // Uzun offset tablosu yalnızca kısa tabloda MSB set olan kayıtlar için gerekir.
        let buyuk_basar = ofset_kisa.iter().filter(|o| **o & 0x8000_0000 != 0).count();
        let buyuk_son = buyuk_tab
            .checked_add(buyuk_basar * 8)
            .ok_or_else(|| bozuk(yol, "büyük offset tablosu taşma yarattı"))?;
        if veri.len() < buyuk_son {
            return Err(bozuk(
                yol,
                format!(
                    "{buyuk_basar} büyük offset kaydı için {buyuk_son} bayt gerekir, \
                     dosyada {} bayt var",
                    veri.len()
                ),
            ));
        }

        let mut ofsetler = Vec::with_capacity(sayi);
        for kisa in &ofset_kisa {
            if kisa & 0x8000_0000 == 0 {
                ofsetler.push(u64::from(*kisa));
            } else {
                let endeks = (kisa & 0x7FFF_FFFF) as usize;
                let t = buyuk_tab + endeks * 8;
                if t + 8 > buyuk_son {
                    return Err(bozuk(
                        yol,
                        format!("büyük offset endeksi {endeks} tablo dışında"),
                    ));
                }
                let mut parca = [0u8; 8];
                parca.copy_from_slice(&veri[t..t + 8]);
                ofsetler.push(u64::from_be_bytes(parca));
            }
        }

        // SHA-1 listesi kesin artan olmalı; ikili arama buna dayanır.
        for i in 1..sayi {
            if shalar[i - 1] >= shalar[i] {
                return Err(bozuk(
                    yol,
                    format!("nesne adları sıralı değil (indeks {i})"),
                ));
            }
        }

        if buyuk_son + 40 > veri.len() {
            return Err(bozuk(yol, "paket özeti / indeks özeti eksik"));
        }
        let mut paket_ozeti = [0u8; 20];
        paket_ozeti.copy_from_slice(&veri[buyuk_son..buyuk_son + 20]);
        let son = veri.len();
        let mut indeks_ozeti = [0u8; 20];
        indeks_ozeti.copy_from_slice(&veri[son - 20..son]);

        // İndeks özeti, dosyanın son 20 baytı hariç tüm içeriğinin SHA-1'idir.
        let hesaplanan = crate::sha1::sha1(&veri[..son - 20]);
        if hesaplanan != indeks_ozeti {
            return Err(bozuk(
                yol,
                format!(
                    "indeks özeti uyuşmuyor: dosya {}, hesaplanan {}",
                    Oid::baytlardan(indeks_ozeti).onaltilik(),
                    Oid::baytlardan(hesaplanan).onaltilik()
                ),
            ));
        }

        Ok(PaketIndeksi {
            yol: yol.to_path_buf(),
            shalar,
            ofsetler,
            crc_ler,
            paket_ozeti,
            indeks_ozeti,
        })
    }

    /// Dizindeki nesne sayısını döndürür.
    pub fn nesne_sayisi(&self) -> usize {
        self.shalar.len()
    }

    /// `.idx` dosyasının yolunu döndürür.
    pub fn yol(&self) -> &Path {
        &self.yol
    }

    /// İndeksin sakladığı paket özetini döndürür (pack dosyasının son 20 baytıyla eşleşmelidir).
    pub fn paket_ozeti(&self) -> [u8; 20] {
        self.paket_ozeti
    }

    /// İndeks dosyasının kendi özetini döndürür.
    pub fn indeks_ozeti(&self) -> [u8; 20] {
        self.indeks_ozeti
    }

    /// İndeksin sakladığı nesne adlarının pack içi offsetlerini sırayla döndürür.
    pub fn ofsetler(&self) -> impl Iterator<Item = u64> + '_ {
        self.ofsetler.iter().copied()
    }

    /// `oid` için pack içi ofseti bulur (ikili arama).
    pub fn ofset_bul(&self, oid: &Oid) -> Option<u64> {
        let hedef = oid.baytlar();
        let mut sol = 0usize;
        let mut sag = self.shalar.len();
        while sol < sag {
            let orta = sol + (sag - sol) / 2;
            match self.shalar[orta].cmp(hedef) {
                std::cmp::Ordering::Less => sol = orta + 1,
                std::cmp::Ordering::Greater => sag = orta,
                std::cmp::Ordering::Equal => return self.ofsetler.get(orta).copied(),
            }
        }
        None
    }

    /// `oid` için indekste saklanan CRC-32 değerini döndürür.
    pub fn crc_bul(&self, oid: &Oid) -> Option<u32> {
        let hedef = oid.baytlar();
        self.shalar
            .iter()
            .position(|sha| sha == hedef)
            .and_then(|i| self.crc_ler.get(i).copied())
    }

    /// İndekteki nesne adlarını sırayla verir.
    pub fn nesne_adlari(&self) -> impl Iterator<Item = Oid> + '_ {
        self.shalar.iter().map(|sha| Oid::baytlardan(*sha))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nesne::NesneTuru;
    use crate::paket::yazici::PaketYazici;

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("gitatlas-idx-{etiket}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&yol);
        std::fs::create_dir_all(&yol).expect("geçici dizin oluşturulmalı");
        yol
    }

    /// İki blob içeren geçerli bir pack + idx çifti yazar ve dizin yolunu döndürür.
    ///
    /// Her test **kendi etiketli** dizinini kullanır: testler paralel çalıştığı için
    /// ortak bir dizin paylaşılsaydı birbirlerinin dosyalarını silmeleri anlamına gelirdi.
    fn ornek_indeks(etiket: &str) -> (PathBuf, Oid, Oid) {
        let dizin = gecici(etiket);
        let mut yazici = PaketYazici::yeni();
        let (a, _) = yazici
            .duz(NesneTuru::Blob, b"birinci")
            .expect("blob eklenmeli");
        let (b, _) = yazici
            .duz(NesneTuru::Blob, b"ikinci")
            .expect("blob eklenmeli");
        yazici.yaz(&dizin, "p1").expect("pack yazılmalı");
        (dizin, a, b)
    }

    #[test]
    fn v2_indeks_ayristirilir() {
        let (dizin, a, b) = ornek_indeks("v2");
        let indeks = PaketIndeksi::ac(&dizin.join("p1.idx")).expect("v2 indeks okunmalı");
        assert_eq!(indeks.nesne_sayisi(), 2);
        assert!(indeks.ofset_bul(&a).is_some());
        assert!(indeks.ofset_bul(&b).is_some());
        assert!(indeks.crc_bul(&a).is_some());
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn idx_v1_reddedilir() {
        let (dizin, _, _) = ornek_indeks("v1");
        let yol = dizin.join("p1-v1.idx");
        // v1: sihirli ve sürüm alanı yok; doğrudan 1024 baytlık fanout gelir.
        let mut v1 = vec![0u8; 1024];
        v1[255 * 4 + 3] = 2;
        v1.extend_from_slice(&12u32.to_be_bytes());
        v1.extend_from_slice(&[0u8; 20]);
        v1.extend_from_slice(&12u32.to_be_bytes());
        v1.extend_from_slice(&[0u8; 20]);
        std::fs::write(&yol, &v1).expect("v1 yazılmalı");
        let hata = match PaketIndeksi::ac(&yol) {
            Ok(_) => panic!("v1 indeks reddedilmeliydi"),
            Err(h) => h,
        };
        assert!(matches!(hata, Hata::IndeksSurumuDesteklenmiyor { .. }));
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn fanout_tutarsizligi_tespit_edilir() {
        let (dizin, _, _) = ornek_indeks("fanout");
        let yol = dizin.join("p1.idx");
        let mut veri = std::fs::read(&yol).expect("okunmalı");
        veri[8 + 4 * 3] = 9; // fanout[3] = 9, fanout[4] = 1 → azalan
        let son = veri.len();
        let yeni = crate::sha1::sha1(&veri[..son - 20]);
        veri[son - 20..].copy_from_slice(&yeni);
        std::fs::write(&yol, &veri).expect("bozuk fanout yazılmalı");
        let hata = match PaketIndeksi::ac(&yol) {
            Ok(_) => panic!("fanout tutarsızlığı reddedilmeliydi"),
            Err(h) => h,
        };
        assert!(matches!(hata, Hata::IndeksBozuk { .. }));
        assert!(hata.to_string().contains("fanout"));
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn kesik_indeks_hata_verir() {
        let (dizin, _, _) = ornek_indeks("kesik");
        let yol = dizin.join("p1.idx");
        let veri = std::fs::read(&yol).expect("okunmalı");
        std::fs::write(&yol, &veri[..veri.len() - 30]).expect("kesik yazılmalı");
        assert!(PaketIndeksi::ac(&yol).is_err());
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn indeks_ozeti_uyusmazligi_tespit_edilir() {
        let (dizin, _, _) = ornek_indeks("ozet");
        let yol = dizin.join("p1.idx");
        let mut veri = std::fs::read(&yol).expect("okunmalı");
        veri[1100] ^= 0xFF;
        std::fs::write(&yol, &veri).expect("bozuk veri yazılmalı");
        let hata = match PaketIndeksi::ac(&yol) {
            Ok(_) => panic!("özet uyuşmazlığı reddedilmeliydi"),
            Err(h) => h,
        };
        assert!(matches!(hata, Hata::IndeksBozuk { .. }));
        let _ = std::fs::remove_dir_all(&dizin);
    }

    #[test]
    fn siralanmamis_nesne_adlari_reddedilir() {
        let (dizin, _, _) = ornek_indeks("sirala");
        let yol = dizin.join("p1.idx");
        let mut veri = std::fs::read(&yol).expect("okunmalı");
        // İlk iki nesne adını takas et: liste artık artan değil.
        let ilk = 8 + 1024;
        let a: Vec<u8> = veri[ilk..ilk + 20].to_vec();
        let b: Vec<u8> = veri[ilk + 20..ilk + 40].to_vec();
        veri[ilk..ilk + 20].copy_from_slice(&b);
        veri[ilk + 20..ilk + 40].copy_from_slice(&a);
        let son = veri.len();
        let yeni = crate::sha1::sha1(&veri[..son - 20]);
        veri[son - 20..].copy_from_slice(&yeni);
        std::fs::write(&yol, &veri).expect("sırasız indeks yazılmalı");
        let hata = match PaketIndeksi::ac(&yol) {
            Ok(_) => panic!("sırasız nesne adları reddedilmeliydi"),
            Err(h) => h,
        };
        assert!(matches!(hata, Hata::IndeksBozuk { .. }));
        let _ = std::fs::remove_dir_all(&dizin);
    }
}
