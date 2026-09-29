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
//!
//! Güvenlik: bildirilen hedef boyut güvenilmeyen pack'ten gelir ve varint ile 10
//! bayta kadar kodlandığı için teorik olarak `u64::MAX`'e kadar olabilir. Bu yüzden
//! boyut **önce** `tavan` ile sınırlanır, ardından hedef komut başına blok blok
//! büyütülür — bildirilen sayı hiçbir zaman doğrudan tahsis miktarı olmaz
//! (`zlib.rs`teki inflate ile aynı politika).

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
///
/// `tavan` tek nesne için izin verilen çözülmüş boyut sınırıdır (`Ayarlar::nesne_tavani`).
/// Başlıkta bildirilen hedef boyut bu sınırı aşıyorsa **tahsis yapılmadan** hata
/// döner: bildirilen sayı `u64::MAX`'e kadar çıkabilir ve doğrudan
/// `Vec::with_capacity` ile kullanılırsa `capacity overflow` paniği üretir — panik
/// `abort` olduğu için bu, hata değil sürecin düşmesidir.
pub fn uygula(kaynak: &[u8], veri: &[u8], tavan: u64) -> Result<Vec<u8>, Hata> {
    let baslik = baslik_ayikla(veri)?;
    if baslik.kaynak_boyut != kaynak.len() as u64 {
        return Err(bozuk(format!(
            "delta {} baytlık kaynak bekliyor, taban {} bayt",
            baslik.kaynak_boyut,
            kaynak.len()
        )));
    }
    if baslik.hedef_boyut > tavan {
        return Err(Hata::CozmeSiniriAsildi {
            bayt: baslik.hedef_boyut,
            tav: tavan,
        });
    }

    // Hedef komut başına blok blok büyür; bildirilen boyut yalnızca tavan denetimi
    // ve döngü sonundaki sonuç doğrulaması için kullanılır, tahsis için değil.
    let mut hedef: Vec<u8> = Vec::new();
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

        // Büyümenin üst sınırı: başlık denetiminden geçen `hedef_boyut` tavanın
        // altında olduğu için bu kontrol aynı zamanda hedefin bellek tahtasını da
        // sınırlar; komutlar küçük bir akışla devasa içerik üretmeye çalışsa burada kesilir.
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

    /// Testlerde kullanılan nesne tavanı; `Ayarlar::nesne_tavani` ile aynı varsayılan.
    const TAVAN: u64 = 64 * 1024 * 1024;

    /// Küçük bir taban nesne: saldırgan delta'ları bunun üzerine yazılır.
    const TABAN: &[u8] = b"taban";

    fn uygula_test(kaynak: &[u8], veri: &[u8]) -> Result<Vec<u8>, Hata> {
        uygula(kaynak, veri, TAVAN)
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
        assert_eq!(uygula_test(&[], &veri).expect("uygulanmalı"), b"abc");
    }

    #[test]
    fn kopyala_komutu_uygulanir() {
        // 0x80 | 0x10 (boyut 1 bayt) | ofset baytı yok -> kaynak[0..1]
        let veri = delta(5, 1, &[0x90, 1]);
        assert_eq!(uygula_test(b"hello", &veri).expect("uygulanmalı"), b"h");
    }

    #[test]
    fn kopyala_ofset_baytlari_okunur() {
        // ofset 2, boyut 3 -> kaynak[2..5]
        let veri = delta(5, 3, &[0x91, 2, 3]);
        assert_eq!(uygula_test(b"hello", &veri).expect("uygulanmalı"), b"llo");
    }

    #[test]
    fn sifir_boyut_kopyalama_65536_kabul_edilir() {
        let kaynak = vec![b'x'; 0x10000];
        // 0x80 | 0x20 (boyut baytı 1) | 0 -> boyut 0 => 0x10000
        let veri = delta(0x10000, 0x10000, &[0xa0, 0x00]);
        let sonuc = uygula_test(&kaynak, &veri).expect("uygulanmalı");
        assert_eq!(sonuc.len(), 0x10000);
    }

    #[test]
    fn kaynak_disi_kopyalama_hata_verir() {
        let veri = delta(3, 5, &[0x91, 1, 5]);
        assert!(matches!(
            uygula_test(b"abc", &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn sifir_bayt_ekleme_hata_verir() {
        let veri = delta(0, 0, &[0x00]);
        assert!(matches!(
            uygula_test(&[], &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn ekleme_aktisi_asersa_hata_verir() {
        let veri = delta(0, 5, &[4, b'a']);
        assert!(matches!(
            uygula_test(&[], &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn hedef_boyut_tutmazsa_hata_verir() {
        let veri = delta(0, 9, &[3, b'a', b'b', b'c']);
        assert!(matches!(
            uygula_test(&[], &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn kaynak_boyut_tutmazsa_hata_verir() {
        let veri = delta(7, 1, &[1, b'x']);
        assert!(matches!(
            uygula_test(b"abc", &veri),
            Err(Hata::DeltaBozuk { .. })
        ));
    }

    #[test]
    fn varint_yarida_kesilirse_hata_verir() {
        let veri = [0x80u8, 0x80];
        assert!(matches!(baslik_ayikla(&veri), Err(Hata::DeltaBozuk { .. })));
    }

    /// Regresyon: güvenilmeyen pack'te `hedef_boyut` 10 baytlık varint ile
    /// `u64::MAX`'e kadar yazılabilir. Bu boyut doğrudan `Vec::with_capacity`
    /// girdiğinde `capacity overflow` paniği üretir; profil `panic = "abort"`
    /// olduğu için hata değil **sürecin düşmesi** olurdu.
    #[test]
    fn devasa_hedef_boyut_tahsis_yapmadan_hata_verir() {
        for bildirilen in [u64::MAX, 1u64 << 63, 1u64 << 40] {
            // Komutlar küçük: yalnızca başlıktaki sayı tehlikeli.
            let veri = delta(TABAN.len() as u64, bildirilen, &[4, b'a', b'b', b'c', b'd']);
            let hata = uygula_test(TABAN, &veri).expect_err("tavan aşımı hata vermeli");
            assert!(
                matches!(
                    hata,
                    Hata::CozmeSiniriAsildi {
                        bayt,
                        tav: TAVAN
                    } if bayt == bildirilen
                ),
                "beklenmeyen hata ({bildirilen}): {hata}"
            );
        }
    }

    /// Aynı senaryo tavan biraz düşürüldüğünde de geçerli: sınır, sabit bir
    /// "güvenli sayı" değil, her zaman `tavan` argümanıdır.
    #[test]
    fn tavan_asimi_tam_sinirda_gecerli() {
        let veri = delta(0, 1024, &[1, b'x']);
        // Tavan tam sınırda: başlık denetimi geçer, hata yalnızca sonuç boyutundan gelir.
        let tam = uygula(&[], &veri, 1024).expect_err("sonuç boyutu tutmamalı");
        assert!(matches!(tam, Hata::DeltaBozuk { .. }), "beklenmeyen: {tam}");

        // Tavan bir bayt altında: tahsis yapılmadan tavan hatası.
        let alt = uygula(&[], &veri, 1023).expect_err("tavan altı reddedilmeli");
        assert!(
            matches!(
                alt,
                Hata::CozmeSiniriAsildi {
                    bayt: 1024,
                    tav: 1023
                }
            ),
            "beklenmeyen: {alt}"
        );
    }

    /// Blok blok büyüme deseni sonucu bozmamalı: çok sayıda küçük komut, tek bir
    /// büyük tahsis olmadan da birebir aynı içeriği üretir.
    #[test]
    fn blok_blok_buyume_dogru_sonuc_uretir() {
        let kaynak: Vec<u8> = (0..256u16).map(|i| i as u8).collect();
        let mut komutlar: Vec<u8> = Vec::new();
        let mut beklenen: Vec<u8> = Vec::new();

        // 400 tur: kaynaktan 37 bayt kopyala, 61 bayt ekle. Hedef ~39 KB,
        // yani başlangıç tahsisi olmadan defalarca büyüme döngüsü çalışır.
        for _ in 0..400 {
            komutlar.extend_from_slice(&kopya_komutu(7, 37));
            beklenen.extend_from_slice(&kaynak[7..44]);

            let parca: Vec<u8> = (0..61u8).map(|i| i.wrapping_add(b'0')).collect();
            komutlar.push(61);
            komutlar.extend_from_slice(&parca);
            beklenen.extend_from_slice(&parca);
        }

        let veri = delta(kaynak.len() as u64, beklenen.len() as u64, &komutlar);
        let sonuc = uygula_test(&kaynak, &veri).expect("blok blok büyüme doğru sonuç vermeli");
        assert_eq!(sonuc.len(), beklenen.len());
        assert_eq!(sonuc, beklenen);
    }

    /// Meşru ve tavan içinde kalan büyük delta hâlâ okunabilmeli: ön tahsis
    /// kaldırıldığı için 4 MiB'lık bir hedef de blok blok büyüyerek üretilir.
    #[test]
    fn tavan_icinde_buyuk_delta_okunur() {
        const BOYUT: usize = 4 * 1024 * 1024;
        let kaynak = vec![b'g'; 0x10000];
        // Her komut 2 bayt: 0x80|0x10 komut baytı ve 64'lük boyut baytı.
        let komut = kopya_komutu(0, 64);
        let tekrar = BOYUT / 64;
        let mut komutlar = Vec::with_capacity(tekrar * komut.len());
        for _ in 0..tekrar {
            komutlar.extend_from_slice(&komut);
        }
        let veri = delta(kaynak.len() as u64, BOYUT as u64, &komutlar);
        let sonuc = uygula_test(&kaynak, &veri).expect("tavan içindeki büyük delta okunmalı");
        assert_eq!(sonuc.len(), BOYUT);
        assert!(sonuc.iter().all(|b| *b == b'g'));
    }

    /// Hedefin tam tavan boyutunda olması da kabul edilir; sınır `>` ile denetlenir.
    #[test]
    fn tam_tavan_boyutlu_hedef_kabul_edilir() {
        let kaynak = vec![b'k'; 64];
        let veri = delta(64, 64, &kopya_komutu(0, 64));
        let sonuc = uygula(&kaynak, &veri, 64).expect("tavan sınırındaki nesne kabul edilmeli");
        assert_eq!(sonuc, kaynak);
    }

    /// Testlerde kullanılan "kopyala ofset..ofset+boyut" komutu üreticisi.
    /// `yazici::delta_kopya` ile aynı belirtimi uygular; buradaki sürüm, delta.rs'in
    /// kendi iç testinin `pub` yüzeye bağımlı kalmaması için gereklidir.
    fn kopya_komutu(ofset: usize, boyut: usize) -> Vec<u8> {
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
}
