//! Sentetik `.pack` / `.idx` **yazıcısı**.
//!
//! Bu modül yalnızca test fikstürü üretimi içindir: gerçek `git gc` çıktısına bağımlı
//! olmayan delta zinciri, bozuk pack, CRC uyuşmazlığı ve büyük offset senaryolarını
//! mekanik olarak kurmak için gereklidir. GitAtlas'ın çalışma zamanı yolu bu modülü
//! **hiç çağırmaz** — depo hiçbir koşulda yazılmaz.
//!
//! Bağımsızlık notu: yazıcı ve okuyucu aynı belirtimden yazıldığı için birbirini
//! doğrulamaz. Bağımsız doğrulama `git cat-file` / `git log` / `git show` çıktılarıyla
//! `tests/entegrasyon.rs` içindeki gerçek depolar üzerinde yapılır.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::crc32::crc32;
use crate::hata::Hata;
use crate::nesne::NesneTuru;
use crate::oid::Oid;
use crate::sha1::sha1;

/// Sıkıştırma düzeyi. Fikstürlerin hızlı üretilmesi için en düşük düzey seçilir;
/// seviye biçimi etkilemez.
const SEVIYE: u32 = 1;

/// Pack içindeki tek bir nesne girdisi (indeks için gereken alanlar).
struct Giris {
    oid: Oid,
    ofset: u64,
    crc: u32,
}

fn hata(metin: &str) -> Hata {
    Hata::CiktiHatasi {
        ayrinti: metin.to_string(),
    }
}

/// Pack dosyası ve `.idx` dizini üreten inşa edici.
#[derive(Default)]
pub struct PaketYazici {
    girdiler: Vec<Giris>,
    govde: Vec<u8>,
    yazildi: bool,
}

impl PaketYazici {
    /// Boş bir yazıcı üretir.
    pub fn yeni() -> Self {
        PaketYazici::default()
    }

    /// Yazılacak pack'in tam baytlarını üretir: 12 baytlık başlık + girdiler + özet.
    pub fn pack_baytlari(&self) -> Vec<u8> {
        let mut tam = Vec::with_capacity(12 + self.govde.len() + 20);
        tam.extend_from_slice(b"PACK");
        tam.extend_from_slice(&2u32.to_be_bytes());
        tam.extend_from_slice(&(self.girdiler.len() as u32).to_be_bytes());
        tam.extend_from_slice(&self.govde);
        let ozet = sha1(&tam);
        tam.extend_from_slice(&ozet);
        tam
    }

    /// Yazılmış pack'in `.idx` ham baytlarını üretir.
    pub fn idx_baytlari(&self) -> Vec<u8> {
        let mut sirali: Vec<&Giris> = self.girdiler.iter().collect();
        sirali.sort_by_key(|g| *g.oid.baytlar());

        let mut idx = Vec::new();
        idx.extend_from_slice(&[0xff, b't', b'O', b'c']);
        idx.extend_from_slice(&2u32.to_be_bytes());

        let mut sayaclar = [0u32; 256];
        for giris in &sirali {
            sayaclar[giris.oid.baytlar()[0] as usize] += 1;
        }
        let mut toplam = 0u32;
        for sayi in sayaclar.iter() {
            toplam += sayi;
            idx.extend_from_slice(&toplam.to_be_bytes());
        }

        for giris in &sirali {
            idx.extend_from_slice(giris.oid.baytlar());
        }
        for giris in &sirali {
            idx.extend_from_slice(&giris.crc.to_be_bytes());
        }
        for giris in &sirali {
            idx.extend_from_slice(&(giris.ofset as u32).to_be_bytes());
        }

        // Paket özeti, özetsiz pack içeriğinin SHA-1'idir.
        let paket_ozeti = sha1(&self.ozetsiz_pack());
        idx.extend_from_slice(&paket_ozeti);
        let indeks_ozeti = sha1(&idx);
        idx.extend_from_slice(&indeks_ozeti);
        idx
    }

    /// 12 baytlık başlık + nesne girdileri (son 20 bayt özet olmadan).
    fn ozetsiz_pack(&self) -> Vec<u8> {
        let mut tam = Vec::with_capacity(12 + self.govde.len());
        tam.extend_from_slice(b"PACK");
        tam.extend_from_slice(&2u32.to_be_bytes());
        tam.extend_from_slice(&(self.girdiler.len() as u32).to_be_bytes());
        tam.extend_from_slice(&self.govde);
        tam
    }

    fn govde_ekle(&mut self, oid: Oid, baslik: &[u8], ham: &[u8]) -> Result<u64, Hata> {
        if self.yazildi {
            return Err(hata("yazıcı zaten yazıldı"));
        }
        let ofset = 12 + self.govde.len() as u64;
        let mut baytlar = baslik.to_vec();
        let siki = sikistir(ham)?;
        baytlar.extend_from_slice(&siki);
        let crc = crc32(&baytlar);
        self.govde.extend_from_slice(&baytlar);
        self.girdiler.push(Giris { oid, ofset, crc });
        Ok(ofset)
    }

    /// Düz bir nesne ekler; nesne adını ve pack içi ofsetini döndürür.
    pub fn duz(&mut self, tur: NesneTuru, veri: &[u8]) -> Result<(Oid, u64), Hata> {
        let oid = nesne_adi(tur, veri);
        let baslik = baslik_kodla(tur_kodu(tur)?, veri.len() as u64);
        let ofset = self.govde_ekle(oid, &baslik, veri)?;
        Ok((oid, ofset))
    }

    /// `OBJ_OFS_DELTA` girdisi ekler. Delta komutları otomatik üretilir
    /// ("kaynağı tümüyle sil, hedefi ekle"); özel komutlar için
    /// [`PaketYazici::ofs_delta_ozel`] kullanılır.
    ///
    /// `taban_boyut`, delta başlığında bildirilen kaynak uzunluğudur; okuyucu bunu
    /// taban nesnenin gerçek boyutuyla karşılaştırır.
    pub fn ofs_delta(
        &mut self,
        taban_ofset: u64,
        taban_tur: NesneTuru,
        taban_boyut: u64,
        hedef: &[u8],
    ) -> Result<(Oid, u64), Hata> {
        self.ofs_delta_ozel(
            taban_ofset,
            taban_tur,
            taban_boyut,
            hedef,
            &delta_ekle(hedef),
        )
    }

    /// Elle komut verilen `OBJ_OFS_DELTA` girdisi.
    pub fn ofs_delta_ozel(
        &mut self,
        taban_ofset: u64,
        taban_tur: NesneTuru,
        taban_boyut: u64,
        hedef: &[u8],
        komutlar: &[u8],
    ) -> Result<(Oid, u64), Hata> {
        self.ofs_delta_bildirilen(
            taban_ofset,
            taban_tur,
            taban_boyut,
            hedef,
            hedef.len() as u64,
            komutlar,
        )
    }

    /// Delta başlığındaki **hedef boyutu ayrıca yazılabilen** `OBJ_OFS_DELTA` girdisi.
    ///
    /// `hedef` yalnızca nesne adını doğru hesaplamak içindir; başlığa yazılan hedef
    /// boyutudur `bildirilen_hedef_boyut` olur. Gerçek `git` bu ikisini eşit yazar;
    /// ayrılabilmeleri, "başlıkta devasa bir boyut bildiren ama gövdesi birkaç bayt
    /// olan" saldırgan pack'ini kurabilmek için gereklidir. Okuyucu bildirilen boyuta
    /// doğrudan tahsis yaparsa bu fikstür süreci düşürür — `tests/pack.rs`
    /// içindeki bellek tavanı regresyon testleri tam olarak bunu ölçer.
    pub fn ofs_delta_bildirilen(
        &mut self,
        taban_ofset: u64,
        taban_tur: NesneTuru,
        taban_boyut: u64,
        hedef: &[u8],
        bildirilen_hedef_boyut: u64,
        komutlar: &[u8],
    ) -> Result<(Oid, u64), Hata> {
        let ofset = 12 + self.govde.len() as u64;
        let mesafe = ofset
            .checked_sub(taban_ofset)
            .ok_or_else(|| hata("ofs-delta taban ofseti nesneden sonra"))?;
        let mut ham = delta_basligi(taban_boyut, bildirilen_hedef_boyut);
        ham.extend_from_slice(komutlar);
        let mut baslik = baslik_kodla(6, ham.len() as u64);
        baslik.extend_from_slice(&mesafe_kodla(mesafe));
        let oid = nesne_adi(taban_tur, hedef);
        let ofset = self.govde_ekle(oid, &baslik, &ham)?;
        Ok((oid, ofset))
    }

    /// `OBJ_REF_DELTA` girdisi ekler; delta komutları otomatik üretilir.
    ///
    /// `taban_tur` yalnızca nesne adını doğru hesaplamak için gereklidir: ref-delta
    /// nesnenin adı çözülmüş içeriğin özetidir, delta verisinin değil.
    pub fn ref_delta(
        &mut self,
        taban_oid: Oid,
        taban_tur: NesneTuru,
        taban_boyut: u64,
        hedef: &[u8],
    ) -> Result<(Oid, u64), Hata> {
        self.ref_delta_ozel(taban_oid, taban_tur, taban_boyut, hedef, &delta_ekle(hedef))
    }

    /// Elle komut verilen `OBJ_REF_DELTA` girdisi.
    pub fn ref_delta_ozel(
        &mut self,
        taban_oid: Oid,
        taban_tur: NesneTuru,
        taban_boyut: u64,
        hedef: &[u8],
        komutlar: &[u8],
    ) -> Result<(Oid, u64), Hata> {
        let mut ham = delta_basligi(taban_boyut, hedef.len() as u64);
        ham.extend_from_slice(komutlar);
        let mut baslik = baslik_kodla(7, ham.len() as u64);
        baslik.extend_from_slice(taban_oid.baytlar());
        let oid = nesne_adi(taban_tur, hedef);
        let ofset = self.govde_ekle(oid, &baslik, &ham)?;
        Ok((oid, ofset))
    }

    /// Test amaçlı: içerik özetinden **bağımsız**, verilen `oid` ile bir pack girdisi yazar.
    ///
    /// Bu yalnızca "etiket ile içerik tutarsız" senaryoları kurmak için gereklidir:
    /// gerçek `git`in ürettiği pack dosyalarında nesne adı her zaman içeriğin özetidir.
    /// Delta döngüsü korumasının testi ancak böyle bir yapay girdiyle kurulabilir.
    pub fn sahte_giris(
        &mut self,
        oid: Oid,
        tur_kodu: u8,
        on_ek: &[u8],
        ham: &[u8],
    ) -> Result<u64, Hata> {
        let mut baslik = baslik_kodla(tur_kodu, ham.len() as u64);
        baslik.extend_from_slice(on_ek);
        self.govde_ekle(oid, &baslik, ham)
    }

    /// Pack ve `.idx` dosyalarını `dizin` altına `<ad>.pack` / `<ad>.idx` olarak yazar.
    pub fn yaz(&mut self, dizin: &Path, ad: &str) -> Result<(PathBuf, PathBuf), Hata> {
        if self.yazildi {
            return Err(hata("yazıcı zaten yazıldı"));
        }
        self.yazildi = true;
        let pack = self.pack_baytlari();
        let idx = self.idx_baytlari();

        fs::create_dir_all(dizin).map_err(|hata| Hata::io(dizin, hata))?;
        let pack_yolu = dizin.join(format!("{ad}.pack"));
        let idx_yolu = dizin.join(format!("{ad}.idx"));
        fs::write(&pack_yolu, &pack).map_err(|hata| Hata::io(&pack_yolu, hata))?;
        fs::write(&idx_yolu, &idx).map_err(|hata| Hata::io(&idx_yolu, hata))?;
        Ok((pack_yolu, idx_yolu))
    }

    /// Eklenen girdi sayısını döndürür.
    pub fn girdi_sayisi(&self) -> usize {
        self.girdiler.len()
    }

    /// Eklenen nesne adlarını sırayla döndürür.
    pub fn nesne_adlari(&self) -> Vec<Oid> {
        self.girdiler.iter().map(|g| g.oid).collect()
    }
}

/// Veriyi zlib ile sıkıştırır.
fn sikistir(veri: &[u8]) -> Result<Vec<u8>, Hata> {
    let mut kodlayici = ZlibEncoder::new(Vec::new(), Compression::new(SEVIYE));
    kodlayici
        .write_all(veri)
        .map_err(|e| hata(&format!("sıkıştırma yazılamadı: {e}")))?;
    kodlayici
        .finish()
        .map_err(|e| hata(&format!("sıkıştırma bitirilemedi: {e}")))
}

fn tur_kodu(tur: NesneTuru) -> Result<u8, Hata> {
    match tur {
        NesneTuru::Commit => Ok(1),
        NesneTuru::Agac => Ok(2),
        NesneTuru::Blob => Ok(3),
        NesneTuru::Etiket => Ok(4),
        NesneTuru::OfsDelta | NesneTuru::RefDelta => Err(hata("delta türü düz nesne olamaz")),
    }
}

/// `<tür> <uzunluk>\0<veri>` biçimindeki içeriğin SHA-1'ini nesne adı yapar.
pub fn nesne_adi(tur: NesneTuru, veri: &[u8]) -> Oid {
    Oid::baytlardan(sha1(&ham_icerik(tur, veri)))
}

fn ham_icerik(tur: NesneTuru, veri: &[u8]) -> Vec<u8> {
    let baslik = format!("{} {}\0", tur.ad(), veri.len());
    let mut ham = baslik.into_bytes();
    ham.extend_from_slice(veri);
    ham
}

/// Pack tür/boyut başlığını kodlar.
///
/// İlk bayt: `1ccc ssss` — `ccc` tür kodu, `ssss` boyutun düşük dört biti. Boyut
/// dört bitten uzunsa ilk baytın en yüksek biti **set** olur ve kalan bitler
/// yedişerli gruplar hâlinde sonraki baytlarda gelir.
pub fn baslik_kodla(tur_kodu: u8, boyut: u64) -> Vec<u8> {
    let mut kalan = boyut >> 4;
    let mut ilk = (tur_kodu << 4) | ((boyut & 0x0f) as u8);
    if kalan > 0 {
        ilk |= 0x80;
    }
    let mut baytlar = vec![ilk];
    while kalan > 0 {
        let mut bayt = (kalan & 0x7f) as u8;
        kalan >>= 7;
        if kalan > 0 {
            bayt |= 0x80;
        }
        baytlar.push(bayt);
    }
    baytlar
}

/// `OBJ_OFS_DELTA` geriye doğru ofset kodlaması: mesafenin **düz değişken uzunluklu**
/// kodlaması (küçük endian, 7 bit, en yüksek bit devam işareti).
///
/// Bu kodlama, okuyucunun `taban = nesne_ofseti - kodlanan` kuralıyla birebir
/// tersidir. Değer `git gc --aggressive` ile üretilen gerçek pack dosyalarının
/// çözümlenmesiyle doğrulanmıştır (`tests/entegrasyon.rs::delta_iceren_pack_cozulur`).
pub fn mesafe_kodla(mesafe: u64) -> Vec<u8> {
    varint_kodla(mesafe)
}

/// Delta başlığını (kaynak ve hedef boyutu) kodlar.
pub fn delta_basligi(kaynak_boyut: u64, hedef_boyut: u64) -> Vec<u8> {
    let mut v = varint_kodla(kaynak_boyut);
    v.extend_from_slice(&varint_kodla(hedef_boyut));
    v
}

fn varint_kodla(deger: u64) -> Vec<u8> {
    let mut v = Vec::new();
    let mut kalan = deger;
    loop {
        let mut bayt = (kalan & 0x7f) as u8;
        kalan >>= 7;
        if kalan > 0 {
            bayt |= 0x80;
        }
        v.push(bayt);
        if kalan == 0 {
            break;
        }
    }
    v
}

/// Kaynağı tümüyle silip hedefi ekleyen delta komutları üretir.
pub fn delta_ekle(veri: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    for parca in veri.chunks(127) {
        v.push(parca.len() as u8);
        v.extend_from_slice(parca);
    }
    v
}

/// Kaynağın tamamını hedefe kopyalayan delta komutları üretir.
pub fn delta_tam_kopya(kaynak_boyut: u64) -> Vec<u8> {
    delta_kopya(0, kaynak_boyut)
}

/// Delta "kopyala" komutlarını üretir.
pub fn delta_kopya(ofset: u64, boyut: u64) -> Vec<u8> {
    let mut komut = 0x80u8;
    let mut baytlar = Vec::new();
    for i in 0..4u32 {
        let b = ((ofset >> (8 * i)) & 0xff) as u8;
        if b != 0 {
            komut |= 1 << i;
            baytlar.push(b);
        }
    }
    for i in 0..3u32 {
        let b = ((boyut >> (8 * i)) & 0xff) as u8;
        if b != 0 {
            komut |= 1 << (4 + i);
            baytlar.push(b);
        }
    }
    let mut v = vec![komut];
    v.extend_from_slice(&baytlar);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baslik_kodla_kucuk_boyut() {
        assert_eq!(baslik_kodla(3, 5), vec![0x35]);
        assert_eq!(baslik_kodla(3, 15), vec![0x3f]);
    }

    #[test]
    fn baslik_kodla_buyuk_boyut() {
        // 20 bayt: ilk bayt 0xB4 (tür 3 + 4 bit + devam), sonra 0x01.
        assert_eq!(baslik_kodla(3, 20), vec![0xB4, 0x01]);
        assert_eq!(baslik_kodla(3, 100), vec![0xB4, 0x06]);
    }

    #[test]
    fn mesafe_kodla_kucuk_mesafe() {
        assert_eq!(mesafe_kodla(1), vec![0x01]);
        assert_eq!(mesafe_kodla(127), vec![0x7f]);
        assert_eq!(mesafe_kodla(128), vec![0x80, 0x01]);
    }

    #[test]
    fn mesafe_kodla_orta_mesafe() {
        assert_eq!(mesafe_kodla(129), vec![0x81, 0x01]);
        assert_eq!(mesafe_kodla(1000), vec![0xE8, 0x07]);
    }

    #[test]
    fn delta_basligi_varint_uretir() {
        assert_eq!(delta_basligi(0, 3), vec![0x00, 0x03]);
        assert_eq!(delta_basligi(200, 1000), vec![0xc8, 0x01, 0xe8, 0x07]);
    }

    #[test]
    fn delta_ekle_parcalara_boler() {
        let veri = vec![b'x'; 200];
        let komut = delta_ekle(&veri);
        assert_eq!(komut[0], 127);
        assert_eq!(komut[128], 73);
    }

    #[test]
    fn nesne_adi_bilinen_deger_uretiyor() {
        // "blob 0\0" -> e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
        let oid = nesne_adi(NesneTuru::Blob, b"");
        assert_eq!(oid.onaltilik(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }
}
