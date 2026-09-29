//! zlib (RFC 1950) akışlarının inflate edilmesi.
//!
//! Git iki yerde zlib kullanır: `objects/XX/…` altındaki gevşek nesneler ve `.pack`
//! içindeki nesne gövdeleri. DEFLATE çözücüsü elle yazılmaz; `flate2` saf Rust
//! `miniz_oxide` arka ucu kullanılır (bkz. WORKER_CONTRACT.md § 3.2-D).
//!
//! Bu sarmalayıcının eklediği iki şey vardır:
//!
//! 1. **Boyut tavanı.** Çözülen veri, çağıranın verdiği tavanı aşarsa hata üretilir.
//!    Yapay olarak şişirilmiş bir pack, aksi hâlde bellek bütçesini (260 MB) tüketirdi.
//! 2. **Kesik / bozuk ayrımı.** Pack okuyucu, nesnenin sıkıştırılmış gövdesinin uzunluğunu
//!    önceden bilmez. "Akış yarıda kaldı" ile "çözücü ilerleyemiyor" ayrımı, ilk durumda
//!    dosyadan daha fazla bayt okunup yeniden denenmesini sağlar.

use flate2::{Decompress, FlushDecompress, Status};

use crate::hata::Hata;

/// Çözme sırasında tek seferde kullanılan çıktı bloğunun boyutu.
const BLOK: usize = 16 * 1024;

/// zlib çözme hatasının nedeni.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ZlibHata {
    /// Sağlanan girdi tükendi, akış sonlanmadı. Pack okuyucu daha fazla bayt okuyup
    /// yeniden deneyebilir.
    Kesik {
        /// Tüketilen girdi baytı.
        okunan: usize,
    },
    /// Çözücü veri tükendiği hâlde ilerlemedi: veri bozuk.
    Bozuk,
    /// Çözülen veri izin verilen tavanı aştı.
    Tavan {
        /// Üretilen bayt.
        bayt: u64,
        /// Uygulanan tavan.
        tav: u64,
    },
    /// `flate2` katmanından gelen hata.
    Aykiri(String),
}

impl From<ZlibHata> for Hata {
    fn from(deger: ZlibHata) -> Self {
        match deger {
            ZlibHata::Kesik { okunan } => Hata::ZlibHatasi {
                ayrinti: format!("akış {okunan} bayttan sonra yarıda kesildi"),
            },
            ZlibHata::Bozuk => Hata::ZlibHatasi {
                ayrinti: "çözücü ilerlemedi (bozuk veri)".to_string(),
            },
            ZlibHata::Tavan { bayt, tav } => Hata::CozmeSiniriAsildi { bayt, tav },
            ZlibHata::Aykiri(metin) => Hata::ZlibHatasi { ayrinti: metin },
        }
    }
}

/// `veri` içindeki zlib akışını çözer, çıktıyı ve **tüketilen girdi baytı sayısını**
/// döndürür.
pub fn inflate_takip(veri: &[u8], tav: u64) -> Result<(Vec<u8>, usize), ZlibHata> {
    let mut cozucu = Decompress::new(true);
    let mut cikti: Vec<u8> = Vec::new();
    let mut okunan = 0usize;
    let mut blok = vec![0u8; BLOK];

    loop {
        if cikti.len() as u64 > tav {
            return Err(ZlibHata::Tavan {
                bayt: cikti.len() as u64,
                tav,
            });
        }

        let onceki_okunan = okunan;
        let cikti_onceki = cozucu.total_out();

        let durum = cozucu
            .decompress(&veri[okunan..], &mut blok, FlushDecompress::None)
            .map_err(|hata| ZlibHata::Aykiri(hata.to_string()))?;

        okunan = cozucu.total_in() as usize;
        let uretilen = (cozucu.total_out() - cikti_onceki) as usize;
        cikti.extend_from_slice(&blok[..uretilen]);

        // Tavan hem döngü başında hem üretim sonrası denetlenir: tek seferde üretilen
        // blok tavanı aşsa kontrol yalnızca başta kalmakla kalmaz, burada da yakalar.
        if cikti.len() as u64 > tav {
            return Err(ZlibHata::Tavan {
                bayt: cikti.len() as u64,
                tav,
            });
        }

        if durum == Status::StreamEnd {
            return Ok((cikti, okunan));
        }

        let ilerleme_yok = okunan == onceki_okunan && uretilen == 0;
        if ilerleme_yok && okunan < veri.len() {
            return Err(ZlibHata::Bozuk);
        }
        if okunan >= veri.len() {
            return Err(ZlibHata::Kesik { okunan });
        }
    }
}

/// `veri` içindeki zlib akışını çözer ve çıktıyı döndürür.
pub fn inflate(veri: &[u8], tav: u64) -> Result<Vec<u8>, Hata> {
    Ok(inflate_takip(veri, tav)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn sikistir(veri: &[u8]) -> Vec<u8> {
        let mut kodlayici = ZlibEncoder::new(Vec::new(), Compression::new(6));
        kodlayici.write_all(veri).expect("yazılmalı");
        kodlayici.finish().expect("bitmeli")
    }

    #[test]
    fn zlib_akisi_cozulur() {
        let siki = sikistir(b"merhaba dunya");
        let cikti = inflate(&siki, 1 << 20).expect("çözülmeli");
        assert_eq!(cikti, b"merhaba dunya");
    }

    #[test]
    fn tuketilen_bayt_sayisi_bildirilir() {
        let siki = sikistir(b"x");
        let (_, tuketilen) = inflate_takip(&siki, 1 << 20).expect("çözülmeli");
        assert_eq!(tuketilen, siki.len());
    }

    #[test]
    fn kesik_akis_kesik_hatasi_verir() {
        let siki = sikistir(&vec![b'a'; 800]);
        let yarisi = &siki[..siki.len() / 2];
        assert!(matches!(
            inflate_takip(yarisi, 1 << 20),
            Err(ZlibHata::Kesik { .. })
        ));
    }

    #[test]
    fn bozuk_akis_hata_verir() {
        let mut siki = sikistir(&vec![b'a'; 400]);
        for bayt in siki.iter_mut().skip(2).take(6) {
            *bayt ^= 0xFF;
        }
        let sonuc = inflate_takip(&siki, 1 << 20);
        assert!(sonuc.is_err());
    }

    #[test]
    fn boyut_tavani_aser() {
        let siki = sikistir(&vec![b'a'; 4096]);
        assert!(inflate(&siki, 64).is_err());
    }
}
