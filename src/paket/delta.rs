//! Delta verisinin ayrıştırılması ve uygulanması.
//!
//! Pack içindeki delta nesneleri, taban nesneye uygulanan iki türlü komut taşır:
//!
//! - **Kopyala**: kaynak içeriğin bir dilimini hedefe yazar. Komut baytının üst biti set'tir;
//!   alt dört bit hangi ofset baytlarının, sonraki üç bit hangi boyut baytlarının
//!   geldiğini söyler. Boyut alanı 0 ise 65536 kabul edilir (git'in `0x10000` kuralı).
//! - **Ekle**: delta akışından 1–127 bayt doğrudan hedefe yazar. Komut baytının üst biti
//!   sıfırdır ve alt yedi bit eklenen bayt sayısıdır; sıfır geçersizdir.
//!
//! Delta başlığı iki değişken uzunluklu tam sayıdır: kaynak boyutu ve hedef boyutu.
//! Üretilen hedef, bildirilen boyutla birebir aynı olmak zorundadır; aksi hâlde pack
//! bozuk sayılır.

use crate::hata::Hata;

/// Ayrıştırılmış delta başlığı.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeltaBasligi {
    /// Uygulama için gereken kaynak nesnenin uzunluğu.
    pub kaynak_boyut: u64,
    /// Elde edilmesi gereken nesnenin uzunluğu.
    pub hedef_boyut: u64,
    /// Komutların başladığı bayt konumu.
    pub komut_basi: usize,
}

fn bozuk(ayrinti: impl Into<String>) -> Hata {
    Hata::DeltaBozuk {
        ayrinti: ayrinti.into(),
    }
}

/// Git'in değişken uzunluklu tam sayı kodlamasını okur (küçük endian, 7 bit).
fn varint(veri: &[u8], imlec: &mut usize) -> Result<u64, Hata> {
    let mut sonuc = 0u64;
    let mut kaydirma = 0u32;
    for _ in 0..10 {
        let bayt = *veri
            .get(*imlec)
            .ok_or_else(|| bozuk("delta başlığı varint'ı yarıda kesildi"))?;
        *imlec += 1;
        sonuc |= u64::from(bayt & 0x7f) << kaydirma;
        if bayt & 0x80 == 0 {
            return Ok(sonuc);
        }
        kaydirma += 7;
    }
    Err(bozuk("delta başlığı varint'ı 10 baytı aştı"))
}

/// Delta başlığını ayrıştırır.
pub fn baslik_ayikla(veri: &[u8]) -> Result<DeltaBasligi, Hata> {
    let mut imlec = 0usize;
    let kaynak_boyut = varint(veri, &mut imlec)?;
    let hedef_boyut = varint(veri, &mut imlec)?;
    Ok(DeltaBasligi {
        kaynak_boyut,
        hedef_boyut,
        komut_basi: imlec,
    })
}

/// Delta verisini `kaynak` üzerine uygular ve hedef içeriği döndürür.
///
/// `veri` yalnızca komut gövdesidir (başlık ayrıca okunur); başlık yine de burada
/// yeniden ayrıştırılır çünkü hedef boyutu doğrulaması zorunludur.
pub fn uygula(kaynak: &[u8], veri: &[u8]) -> Result<Vec<u8>, Hata> {
    let baslik = baslik_ayikla(veri)?;
    if baslik.kaynak_boyut != kaynak.len() as u64 {
        return Err(bozuk(format!(
            "delta {} baytlık kaynak bekliyor, taban {} bayt",
            baslik.kaynak_boyut,
            kaynak.len()
        )));
    }

    let mut hedef: Vec<u8> = Vec::with_capacity(baslik.hedef_boyut as usize);
    let mut imlec = baslik.komut_basi;

    while imlec < veri.len() {
        let komut = veri[imlec];
        imlec += 1;

        if komut & 0x80 != 0 {
            // Kopyala komutu.
            let mut ofset = 0u64;
            let mut boyut = 0u64;
            for (bit, kaydirma) in [(0x01u8, 0u32), (0x02, 8), (0x04, 16), (0x08, 24)] {
                if komut & bit != 0 {
                    let b = *veri
                        .get(imlec)
                        .ok_or_else(|| bozuk("kopyala komutunda ofset baytı eksik"))?;
                    imlec += 1;
                    ofset |= u64::from(b) << kaydirma;
                }
            }
            for (bit, kaydirma) in [(0x10u8, 0u32), (0x20, 8), (0x40, 16)] {
                if komut & bit != 0 {
                    let b = *veri
                        .get(imlec)
                        .ok_or_else(|| bozuk("kopyala komutunda boyut baytı eksik"))?;
                    imlec += 1;
                    boyut |= u64::from(b) << kaydirma;
                }
            }
            if boyut == 0 {
                boyut = 0x10000;
            }
            let son = ofset
                .checked_add(boyut)
                .ok_or_else(|| bozuk("kopyala komutu taşma yarattı"))?;
            if son > kaynak.len() as u64 {
                return Err(bozuk(format!(
                    "kopyala komutu kaynak dışına taşıyor: {ofset}..{son} > {}",
                    kaynak.len()
                )));
            }
            hedef.extend_from_slice(&kaynak[ofset as usize..son as usize]);
        } else {
            // Ekle komutu.
            let adet = usize::from(komut & 0x7f);
            if adet == 0 {
                return Err(bozuk("sıfır bayt ekleyen komut geçersiz"));
            }
            let son = imlec
                .checked_add(adet)
                .ok_or_else(|| bozuk("ekle komutu taşma yarattı"))?;
            let parca = veri
                .get(imlec..son)
                .ok_or_else(|| bozuk("ekle komutu delta akışını aşıyor"))?;
            hedef.extend_from_slice(parca);
            imlec = son;
        }

        if hedef.len() as u64 > baslik.hedef_boyut {
            return Err(bozuk(format!(
                "hedef {} baytı aştı (bildirilen {})",
                hedef.len(),
                baslik.hedef_boyut
            )));
        }
    }

    if hedef.len() as u64 != baslik.hedef_boyut {
        return Err(bozuk(format!(
            "hedef {} bayt, delta {} bayt diyor",
            hedef.len(),
            baslik.hedef_boyut
        )));
    }
    Ok(hedef)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint_yaz(deger: u64) -> Vec<u8> {
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

    fn delta(kaynak_boyut: u64, hedef_boyut: u64, komutlar: &[u8]) -> Vec<u8> {
        let mut v = varint_yaz(kaynak_boyut);
        v.extend_from_slice(&varint_yaz(hedef_boyut));
        v.extend_from_slice(komutlar);
        v
    }

    #[test]
    fn baslik_ayiklanir() {
        let veri = delta(10, 3, &[3, b'a', b'b', b'c']);
        let baslik = baslik_ayikla(&veri).expect("başlık ayrıştırılmalı");
        assert_eq!(baslik.kaynak_boyut, 10);
        assert_eq!(baslik.hedef_boyut, 3);
    }

    #[test]
    fn ekle_komutu_uygulanir() {
        let veri = delta(0, 3, &[3, b'a', b'b', b'c']);
        assert_eq!(uygula(&[], &veri).expect("uygulanmalı"), b"abc");
    }

    #[test]
    fn kopyala_komutu_uygulanir() {
        // 0x80 | 0x10 (boyut 1 bayt) | ofset baytı yok -> kaynak[0..1]
        let veri = delta(5, 1, &[0x90, 1]);
        assert_eq!(uygula(b"hello", &veri).expect("uygulanmalı"), b"h");
    }

    #[test]
    fn kopyala_ofset_baytlari_okunur() {
        // ofset 2, boyut 3 -> kaynak[2..5]
        let veri = delta(5, 3, &[0x91, 2, 3]);
        assert_eq!(uygula(b"hello", &veri).expect("uygulanmalı"), b"llo");
    }

    #[test]
    fn sifir_boyut_kopyalama_65536_kabul_edilir() {
        let kaynak = vec![b'x'; 0x10000];
        // 0x80 | 0x20 (boyut baytı 1) | 0 -> boyut 0 => 0x10000
        let veri = delta(0x10000, 0x10000, &[0xa0, 0x00]);
        let sonuc = uygula(&kaynak, &veri).expect("uygulanmalı");
        assert_eq!(sonuc.len(), 0x10000);
    }

    #[test]
    fn kaynak_disi_kopyalama_hata_verir() {
        let veri = delta(3, 5, &[0x91, 1, 5]);
        assert!(matches!(
            uygula(b"abc", &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn sifir_bayt_ekleme_hata_verir() {
        let veri = delta(0, 0, &[0x00]);
        assert!(matches!(uygula(&[], &veri), Err(Hata::DeltaBozuk { .. })));
    }

    #[test]
    fn ekleme_aktisi_asersa_hata_verir() {
        let veri = delta(0, 5, &[4, b'a']);
        assert!(matches!(uygula(&[], &veri), Err(Hata::DeltaBozuk { .. })));
    }

    #[test]
    fn hedef_boyut_tutmazsa_hata_verir() {
        let veri = delta(0, 9, &[3, b'a', b'b', b'c']);
        assert!(matches!(uygula(&[], &veri), Err(Hata::DeltaBozuk { .. })));
    }

    #[test]
    fn kaynak_boyut_tutmazsa_hata_verir() {
        let veri = delta(7, 1, &[1, b'x']);
        assert!(matches!(
            uygula(b"abc", &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn varint_yarida_kesilirse_hata_verir() {
        let veri = [0x80u8, 0x80];
        assert!(matches!(baslik_ayikla(&veri), Err(Hata::DeltaBozuk { .. })));
    }
}
