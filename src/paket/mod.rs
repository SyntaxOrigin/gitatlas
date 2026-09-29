//! Pack dosyası (`.pack`) okuma, nesne başlığı ayrıştırma ve delta çözümleme.
//!
//! Bir `.pack` dosyası şu düzendedir:
//!
//! ```text
//! 4 bayt   imza      "PACK"
//! 4 bayt   sürüm     2 (veya 3)
//! 4 bayt   nesne sayısı
//! …        nesne girdileri (tür/boyut varint'i, delta başlığı, zlib gövde)
//! 20 bayt  özet      dosyanın geri kalanının SHA-1'i
//! ```
//!
//! Üç türlü nesne girdisi vardır: düz nesne, `OBJ_OFS_DELTA` (taban ofsetle) ve
//! `OBJ_REF_DELTA` (taban nesne adıyla). Her ikisi de taban nesneyi önce çözümler,
//! sonra delta komutlarını uygular.
//!
//! Güvenlik: hiçbir yazma yapılmaz. Dosya yalnızca `File::open` ile açılır; ölçülen
//! nesnenin sıkıştırılmış gövdesi CRC-32'si `.idx` ile karşılaştırılır.

pub mod delta;
pub mod indeks;
pub mod yazici;

use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::ayarlar::Ayarlar;
use crate::crc32::crc32;
use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::nesne::NesneTuru;
use crate::oid::Oid;
use crate::zlib::{inflate_takip, ZlibHata};

use indeks::PaketIndeksi;

/// Bir sıkıştırılmış nesne parçasının ilk okumada alınan boyutu.
const ILK_PARC: usize = 16 * 1024;

/// Bir nesne başlığının okunması için gereken azami pencere boyutu.
const BASLIK_PENCERE: usize = 64;

fn bozuk(ayrinti: impl Into<String>) -> Hata {
    Hata::PaketBozuk {
        ayrinti: ayrinti.into(),
    }
}

/// Bir `.pack` dosyası ve onun `.idx` dizini.
#[derive(Debug)]
pub struct PaketDosyasi {
    paket_yolu: PathBuf,
    indeks: PaketIndeksi,
    boyut: u64,
    bildirilen_sayi: u32,
    ozet: [u8; 20],
    dosya: RefCell<File>,
}

impl PaketDosyasi {
    /// `idx_yolu` ile eşleşen `.pack` dosyasını açar ve başlık doğrulaması yapar.
    ///
    /// `.idx` içindeki paket özeti ile `.pack` dosyasının son 20 baytı karşılaştırılır;
    /// ikisi tutmazsa dosya çifti reddedilir (git `index-pack --verify` ile aynı niyet).
    pub fn ac(idx_yolu: &Path) -> Result<Self, Hata> {
        let indeks = PaketIndeksi::ac(idx_yolu)?;
        let paket_yolu = idx_yolu.with_extension("pack");
        let mut dosya = File::open(&paket_yolu).map_err(|hata| Hata::io(&paket_yolu, hata))?;
        let boyut = dosya
            .metadata()
            .map_err(|hata| Hata::io(&paket_yolu, hata))?
            .len();

        if boyut < 32 {
            return Err(bozuk(format!(
                "{} çok kısa ({} bayt)",
                paket_yolu.display(),
                boyut
            )));
        }

        let mut baslik = [0u8; 12];
        dosya
            .read_exact(&mut baslik)
            .map_err(|hata| Hata::io(&paket_yolu, hata))?;
        if &baslik[..4] != b"PACK" {
            return Err(bozuk(format!(
                "{} PACK imzasıyla başlamıyor",
                paket_yolu.display()
            )));
        }
        let surum = u32::from_be_bytes([baslik[4], baslik[5], baslik[6], baslik[7]]);
        if surum != 2 && surum != 3 {
            return Err(bozuk(format!("desteklenmeyen pack sürümü: {surum}")));
        }
        let bildirilen_sayi = u32::from_be_bytes([baslik[8], baslik[9], baslik[10], baslik[11]]);
        if bildirilen_sayi as usize != indeks.nesne_sayisi() {
            return Err(bozuk(format!(
                "pack {} nesne diyor, indeks {} nesne diyor",
                bildirilen_sayi,
                indeks.nesne_sayisi()
            )));
        }

        let mut ozet = [0u8; 20];
        dosya
            .seek(SeekFrom::Start(boyut - 20))
            .and_then(|_| dosya.read_exact(&mut ozet))
            .map_err(|hata| Hata::io(&paket_yolu, hata))?;

        if ozet != indeks.paket_ozeti() {
            return Err(bozuk(format!(
                "{} özeti {}.idx özetiyle eşleşmiyor",
                paket_yolu.display(),
                idx_yolu.display()
            )));
        }

        Ok(PaketDosyasi {
            paket_yolu,
            indeks,
            boyut,
            bildirilen_sayi,
            ozet,
            dosya: RefCell::new(dosya),
        })
    }

    /// `.pack` dosyasının yolunu döndürür.
    pub fn paket_yolu(&self) -> &Path {
        &self.paket_yolu
    }

    /// Bu paketteki nesne sayısını döndürür.
    pub fn nesne_sayisi(&self) -> usize {
        self.bildirilen_sayi as usize
    }

    /// `.pack` dosyasının bayt cinsinden boyutunu döndürür.
    pub fn dosya_boyutu(&self) -> u64 {
        self.boyut
    }

    /// Pack dosyasının özetini döndürür.
    pub fn ozet(&self) -> [u8; 20] {
        self.ozet
    }

    /// Bütün pack içeriğinin SHA-1'ini yeniden hesaplar ve bildirilen özetle karşılaştırır.
    ///
    /// Bu işlem tüm dosyayı okuduğu için çağıranın seçimiyle yapılır; `ozet` alt komutunda
    /// tek seferlik bütünlük kanıtı olarak kullanılır.
    pub fn butunluk_denetle(&self) -> Result<(), Hata> {
        let veri =
            std::fs::read(&self.paket_yolu).map_err(|hata| Hata::io(&self.paket_yolu, hata))?;
        let son = veri.len();
        let hesaplanan = crate::sha1::sha1(&veri[..son - 20]);
        if hesaplanan != self.ozet {
            return Err(bozuk(format!(
                "pack özeti uyuşmuyor: bildirilen {}, hesaplanan {}",
                Oid::baytlardan(self.ozet).onaltilik(),
                Oid::baytlardan(hesaplanan).onaltilik()
            )));
        }
        Ok(())
    }

    /// `oid` için bu paketteki ofseti döndürür (yoksa `None`).
    pub fn ofset_bul(&self, oid: &Oid) -> Option<u64> {
        self.indeks.ofset_bul(oid)
    }

    /// Pack içindeki nesne adlarını sırayla verir.
    pub fn nesne_adlari(&self) -> impl Iterator<Item = Oid> + '_ {
        self.indeks.nesne_adlari()
    }

    /// `oid` nesnesini çözer ve temel türüyle yükünü döndürür.
    ///
    /// Delta tabanı bulunamazsa hata üretilir; "sessiz devam" yolu yoktur (E-004).
    pub fn nesne(
        &self,
        magaza: &Magaza,
        oid: &Oid,
        ayar: &Ayarlar,
    ) -> Result<(NesneTuru, Vec<u8>), Hata> {
        let ofset = self
            .indeks
            .ofset_bul(oid)
            .ok_or_else(|| Hata::NesneBulunamadi {
                oid: oid.onaltilik(),
            })?;
        self.coz(magaza, ofset, ayar, 0)
    }

    /// `ofset` konumundaki nesneyi çözer.
    pub fn coz(
        &self,
        magaza: &Magaza,
        ofset: u64,
        ayar: &Ayarlar,
        derinlik: u32,
    ) -> Result<(NesneTuru, Vec<u8>), Hata> {
        if derinlik > ayar.delta_derinligi {
            return Err(Hata::DeltaZinciriAsildi {
                tavan: ayar.delta_derinligi,
            });
        }
        if ofset < 12 || ofset >= self.boyut - 20 {
            return Err(bozuk(format!("geçersiz nesne ofseti: {ofset}")));
        }

        let baslik = self.baslik_oku(ofset)?;
        let (veri, tuketilen) = self.sikistirilmis_oku(ofset, baslik.veri_basi, ayar)?;

        // Pack başlığındaki boyut, zlib ile çözülmüş verinin uzunluğudur; delta
        // nesnelerinde bu uzunluk delta akışının kendisine aittir.
        if veri.len() as u64 != baslik.boyut {
            return Err(bozuk(format!(
                "{ofset} ofsetindeki nesne {} bayt diyor, çözülen {} bayt",
                baslik.boyut,
                veri.len()
            )));
        }
        let _ = tuketilen;

        match baslik.tur {
            NesneTuru::OfsDelta => {
                let taban_ofset = ofset
                    .checked_sub(baslik.taban_ofset)
                    .ok_or_else(|| bozuk("ofs-delta taban ofseti negatif"))?;
                if taban_ofset < 12 || taban_ofset >= ofset {
                    return Err(bozuk(format!(
                        "ofs-delta tabanı ileriye işaret ediyor: {taban_ofset} >= {ofset}"
                    )));
                }
                let (tur, taban) = self.coz(magaza, taban_ofset, ayar, derinlik + 1)?;
                let hedef = delta::uygula(&taban, &veri, ayar.nesne_tavani)?;
                Ok((tur, hedef))
            }
            NesneTuru::RefDelta => {
                let taban_oid = baslik
                    .taban_oid
                    .ok_or_else(|| bozuk("ref-delta başlığında taban nesne adı yok"))?;
                // Kilit, taban çözülene kadar canlı tutulur: tabanın kendisi (doğrudan
                // ya da zincirle) aynı nesneyi talep ederse döngü hatası üretilir.
                let _kilitle = magaza.cozum_kilidi(&taban_oid)?;
                let taban_nesne = magaza.coz(&taban_oid, ayar, derinlik + 1)?;
                let hedef = delta::uygula(&taban_nesne.veri, &veri, ayar.nesne_tavani)?;
                Ok((taban_nesne.tur, hedef))
            }
            tur => Ok((tur, veri)),
        }
    }

    /// Pack içindeki nesnelerin tür dağılımını döndürür.
    ///
    /// Yalnızca nesne başlıkları okunur (gövdeler çözülmez), bu yüzden büyük
    /// paketlerde de ucuzdur. `ozet` çıktısında ve testlerde "delta içeren pack
    /// gerçekten okundu mu" sorusunu kanıtlamak için kullanılır.
    pub fn tur_sayilari(&self) -> std::collections::BTreeMap<&'static str, usize> {
        let mut sayaclar = std::collections::BTreeMap::new();
        for ofset in self.indeks.ofsetler() {
            if let Ok(baslik) = self.baslik_oku(ofset) {
                *sayaclar.entry(baslik.tur.ad()).or_insert(0) += 1;
            }
        }
        sayaclar
    }

    /// Pack içindeki nesnelerin tür dağılımını JSON değeri olarak döndürür.
    pub fn tur_sayilari_json(&self) -> serde_json::Value {
        serde_json::json!(self.tur_sayilari())
    }

    /// `oid` nesnesinin sıkıştırılmış gövdesini açıp `.idx` CRC-32 değeriyle karşılaştırır.
    ///
    /// `.idx` içindeki CRC-32 değeri, nesnenin ofseti ile sıkıştırılmış gövdesinin son
    /// baytı arasındaki *tüm* baytların (tür/boyut varint'i + delta başlığı + zlib
    /// gövde) sağlamasıdır. Bu denetim, çözülen içeriğin SHA-1 kontrolüne ek olarak
    /// sıkıştırma katmanının da sağlam olduğunu gösterir.
    pub fn crc_dogrula(&self, oid: &Oid, ayar: &Ayarlar) -> Result<(), Hata> {
        let ofset = self
            .indeks
            .ofset_bul(oid)
            .ok_or_else(|| Hata::NesneBulunamadi {
                oid: oid.onaltilik(),
            })?;
        let baslik = self.baslik_oku(ofset)?;
        let (_, tuketilen) = self.sikistirilmis_oku(ofset, baslik.veri_basi, ayar)?;
        let govde_uzunluk = (baslik.veri_basi - ofset) as usize + tuketilen;
        let parca = self
            .bayt_oku(ofset, govde_uzunluk)?
            .ok_or_else(|| bozuk("CRC için nesne baytları okunamadı"))?;
        match self.indeks.crc_bul(oid) {
            Some(beklenen) if crc32(&parca) == beklenen => Ok(()),
            Some(beklenen) => Err(bozuk(format!(
                "nesne {} CRC-32 uyuşmuyor (indeks {beklenen}, hesaplanan {})",
                oid.onaltilik(),
                crc32(&parca)
            ))),
            None => Err(bozuk(format!(
                "nesne {} için indeks CRC kaydı yok",
                oid.onaltilik()
            ))),
        }
    }

    /// `ofset` konumundaki nesne başlığını okur.
    fn baslik_oku(&self, ofset: u64) -> Result<PackBasligi, Hata> {
        let pencere = self
            .bayt_oku(ofset, BASLIK_PENCERE)?
            .ok_or_else(|| bozuk(format!("{ofset} ofseti dosya sonunda")))?;
        let mut i = 0usize;

        let ilk = *pencere.first().ok_or_else(|| bozuk("boş başlık"))?;
        i += 1;
        let tur_kodu = (ilk >> 4) & 0x07;
        let mut boyut = u64::from(ilk & 0x0f);
        let mut kaydirma = 4u32;
        let mut bayt = ilk;
        while bayt & 0x80 != 0 {
            bayt = *pencere
                .get(i)
                .ok_or_else(|| bozuk("nesne başlığı yarıda kesildi"))?;
            i += 1;
            boyut |= u64::from(bayt & 0x7f) << kaydirma;
            kaydirma += 7;
            if kaydirma > 63 {
                return Err(bozuk("nesne boyutu taşma yarattı"));
            }
        }

        let mut taban_ofset = 0u64;
        let mut taban_oid = None;

        match tur_kodu {
            1..=4 => {}
            6 => {
                // OBJ_OFS_DELTA: geriye doğru, "1 ekle ve 7 kaydır" kodlaması.
                let mut b = *pencere
                    .get(i)
                    .ok_or_else(|| bozuk("ofs-delta ofseti yarıda kesildi"))?;
                i += 1;
                taban_ofset = u64::from(b & 0x7f);
                while b & 0x80 != 0 {
                    b = *pencere
                        .get(i)
                        .ok_or_else(|| bozuk("ofs-delta ofseti yarıda kesildi"))?;
                    i += 1;
                    taban_ofset = taban_ofset
                        .checked_add(1)
                        .and_then(|v| v.checked_mul(0x80))
                        .and_then(|v| v.checked_add(u64::from(b & 0x7f)))
                        .ok_or_else(|| bozuk("ofs-delta ofseti taşma yarattı"))?;
                }
            }
            7 => {
                // OBJ_REF_DELTA: 20 baytlık taban nesne adı.
                let parca = pencere
                    .get(i..i + 20)
                    .ok_or_else(|| bozuk("ref-delta taban adı yarıda kesildi"))?;
                taban_oid = Some(Oid::baytlardan(parca.try_into().unwrap_or([0u8; 20])));
                i += 20;
            }
            _ => return Err(bozuk(format!("bilinmeyen nesne tip kodu: {tur_kodu}"))),
        }

        let tur = match tur_kodu {
            1 => NesneTuru::Commit,
            2 => NesneTuru::Agac,
            3 => NesneTuru::Blob,
            4 => NesneTuru::Etiket,
            6 => NesneTuru::OfsDelta,
            _ => NesneTuru::RefDelta,
        };

        Ok(PackBasligi {
            tur,
            boyut,
            veri_basi: ofset + i as u64,
            taban_ofset,
            taban_oid,
        })
    }

    /// `baslangic` konumundaki zlib gövdesini çözer.
    ///
    /// Sıkıştırılmış gövdenin uzunluğu bilinmediğinden parça hâlinde okunur; çözücü
    /// "kesildi" derse parça iki katına çıkarılıp yeniden denenir.
    fn sikistirilmis_oku(
        &self,
        nesne_ofset: u64,
        baslangic: u64,
        ayar: &Ayarlar,
    ) -> Result<(Vec<u8>, usize), Hata> {
        let kalan = self.boyut - 20 - baslangic;
        let mut parca = ILK_PARC.min(kalan as usize);

        loop {
            let veri = self
                .bayt_oku(baslangic, parca)?
                .ok_or_else(|| bozuk(format!("{baslangic} konumundan okunamadı")))?;
            match inflate_takip(&veri, ayar.nesne_tavani) {
                Ok((cikti, tuketilen)) => return Ok((cikti, tuketilen)),
                Err(ZlibHata::Kesik { .. }) => {
                    if veri.len() as u64 >= kalan || parca >= kalan as usize {
                        return Err(Hata::ZlibHatasi {
                            ayrinti: format!(
                                "{} ofsetindeki nesnenin sıkıştırılmış gövdesi yarıda kesilmiş",
                                nesne_ofset
                            ),
                        });
                    }
                    parca = parca.saturating_mul(2).min(kalan as usize);
                }
                Err(hata) => return Err(hata.into()),
            }
        }
    }

    /// `baslangic` konumundan en fazla `uzunluk` bayt okur; dosya sonu aşıldıysa `None`.
    fn bayt_oku(&self, baslangic: u64, uzunluk: usize) -> Result<Option<Vec<u8>>, Hata> {
        let kalan = self.boyut.saturating_sub(baslangic);
        if kalan == 0 {
            return Ok(None);
        }
        let alinacak = (uzunluk as u64).min(kalan) as usize;
        let mut tampon = vec![0u8; alinacak];
        let mut dosya = self.dosya.borrow_mut();
        dosya
            .seek(SeekFrom::Start(baslangic))
            .and_then(|_| dosya.read_exact(&mut tampon))
            .map_err(|hata| Hata::io(&self.paket_yolu, hata))?;
        Ok(Some(tampon))
    }
}

/// Pack içindeki tek bir nesnenin başlığı.
struct PackBasligi {
    tur: NesneTuru,
    boyut: u64,
    veri_basi: u64,
    taban_ofset: u64,
    taban_oid: Option<Oid>,
}
