//! Git nesne türleri ve `<tür> <uzunluk>\0<payload>` başlığının ayrıştırılması.
//!
//! Kapsam: yalnızca biçim tanımı ve ayrıştırma. Nerede bulunduğu (gevşek nesne mi,
//! pack mi) bu modülün işi değildir; o [`crate::gevsek`] ve [`crate::paket`] modüllerindedir.

use std::fmt;

use crate::hata::Hata;

/// Git nesne türleri. Sayısal değerler pack dosyasındaki tip kodlarıdır.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NesneTuru {
    /// Commit kaydı.
    Commit,
    /// Ağaç (dizin) kaydı.
    Agac,
    /// Dosya içeriği.
    Blob,
    /// Etiket kaydı.
    Etiket,
    /// Pack içinde, tabanı ofsetle tanımlanan delta nesnesi.
    OfsDelta,
    /// Pack içinde, tabanı nesne adıyla tanımlanan delta nesnesi.
    RefDelta,
}

impl NesneTuru {
    /// Git'in nesne başlığında kullandığı adı döndürür (`blob`, `commit`, …).
    pub fn ad(&self) -> &'static str {
        match self {
            NesneTuru::Commit => "commit",
            NesneTuru::Agac => "tree",
            NesneTuru::Blob => "blob",
            NesneTuru::Etiket => "tag",
            NesneTuru::OfsDelta => "ofs-delta",
            NesneTuru::RefDelta => "ref-delta",
        }
    }

    /// Delta olmayan temel türlerden biri olup olmadığını bildirir.
    pub fn temel_mi(&self) -> bool {
        !matches!(self, NesneTuru::OfsDelta | NesneTuru::RefDelta)
    }

    /// Başlıktaki adı türe çevirir (`delta` önekini temel türe indirger).
    pub fn ad_dan(ad: &str) -> Option<Self> {
        match ad {
            "commit" => Some(NesneTuru::Commit),
            "tree" => Some(NesneTuru::Agac),
            "blob" => Some(NesneTuru::Blob),
            "tag" => Some(NesneTuru::Etiket),
            _ => None,
        }
    }
}

impl fmt::Display for NesneTuru {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ad())
    }
}

/// Ayrıştırılmış bir Git nesnesi: türü ve ham yükü.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Nesne {
    /// Nesne türü.
    pub tur: NesneTuru,
    /// Nesne yükü (başlık hariç).
    pub veri: Vec<u8>,
}

impl Nesne {
    /// Tür ve yükten nesne kurar.
    pub fn yeni(tur: NesneTuru, veri: Vec<u8>) -> Self {
        Nesne { tur, veri }
    }

    /// Nesnenin başlık dahil, içerik olarak saklanan baytlarını üretir.
    ///
    /// Git nesne adını bu baytların SHA-1'i olarak hesaplar; SHA-1 doğrulaması bu
    /// yüzden burada merkezîdir.
    pub fn ham_icerik(&self) -> Vec<u8> {
        let baslik = format!("{} {}\0", self.tur.ad(), self.veri.len());
        let mut ham = Vec::with_capacity(baslik.len() + self.veri.len());
        ham.extend_from_slice(baslik.as_bytes());
        ham.extend_from_slice(&self.veri);
        ham
    }
}

/// Ayrıştırılmış nesne başlığı.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NesneBasligi {
    /// Başlıktaki nesne türü.
    pub tur: NesneTuru,
    /// Başlıkta bildirilen yük uzunluğu.
    pub boyut: u64,
    /// Yükün başladığı bayt konumu.
    pub yol_basi: usize,
}

/// `<tür> <uzunluk>\0<payload>` başlığını ayrıştırır.
///
/// Uzunluk alanının sonundaki `NUL` baytının bir sonraki baytla (`\n` veya NUL) birleşmesi
/// Git'in kullandığı normalizasyondur ve burada birebir uygulanır.
pub fn baslik_ayikla(ham: &[u8]) -> Result<NesneBasligi, Hata> {
    let ayirac = ham
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| Hata::NesneBozuk {
            oid: "-".to_string(),
            ayrinti: "nesne başlığında NUL ayırıcı yok".to_string(),
        })?;

    let baslik = std::str::from_utf8(&ham[..ayirac]).map_err(|_| Hata::NesneBozuk {
        oid: "-".to_string(),
        ayrinti: "nesne başlığı UTF-8 değil".to_string(),
    })?;

    let (tur_adi, boyut_adi) = baslik.split_once(' ').ok_or_else(|| Hata::NesneBozuk {
        oid: "-".to_string(),
        ayrinti: format!("nesne başlığı 'tür boyut' biçiminde değil: {baslik}"),
    })?;

    let tur = NesneTuru::ad_dan(tur_adi).ok_or_else(|| Hata::NesneBozuk {
        oid: "-".to_string(),
        ayrinti: format!("bilinmeyen nesne türü: {tur_adi}"),
    })?;

    let boyut = boyut_adi.parse::<u64>().map_err(|_| Hata::NesneBozuk {
        oid: "-".to_string(),
        ayrinti: format!("nesne boyutu sayı değil: {boyut_adi}"),
    })?;

    Ok(NesneBasligi {
        tur,
        boyut,
        yol_basi: ayirac + 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baslik_ayikla_ayiklanir() {
        let baslik = baslik_ayikla(b"blob 5\0hello").expect("başlık ayrıştırılmalı");
        assert_eq!(baslik.tur, NesneTuru::Blob);
        assert_eq!(baslik.boyut, 5);
        assert_eq!(baslik.yol_basi, 7);
    }

    #[test]
    fn baslik_ayikla_bos_yuk_olur() {
        let baslik = baslik_ayikla(b"tree 0\0").expect("başlık ayrıştırılmalı");
        assert_eq!(baslik.tur, NesneTuru::Agac);
        assert_eq!(baslik.boyut, 0);
    }

    #[test]
    fn baslik_ayikla_buyuk_boyutu_kabul_eder() {
        let ham = format!("blob 1048576\0{}", "a".repeat(10));
        let baslik = baslik_ayikla(ham.as_bytes()).expect("başlık ayrıştırılmalı");
        assert_eq!(baslik.boyut, 1_048_576);
    }

    #[test]
    fn baslik_ayikla_nul_yoksa_hata_verir() {
        let hata = baslik_ayikla(b"blob 5 hello").expect_err("NUL yoksa hata verilmeli");
        assert!(matches!(hata, Hata::NesneBozuk { .. }));
    }

    #[test]
    fn baslik_ayikla_bilinmeyen_tur_hata_verir() {
        let hata = baslik_ayikla(b"blub 5\0hello").expect_err("bilinmeyen tür hata vermeli");
        assert!(matches!(hata, Hata::NesneBozuk { .. }));
    }

    #[test]
    fn baslik_ayikla_boyut_sayi_degilse_hata_verir() {
        let hata =
            baslik_ayikla(b"blob bes\0hello").expect_err("boyut sayı değilse hata verilmeli");
        assert!(matches!(hata, Hata::NesneBozuk { .. }));
    }

    #[test]
    fn ham_iceri_kisa_yol_donusu_ile_ayni() {
        let nesne = Nesne::yeni(NesneTuru::Blob, b"hello".to_vec());
        assert_eq!(nesne.ham_icerik(), b"blob 5\0hello");
    }

    #[test]
    fn nesne_turu_temel_mi_ayrimi_yapar() {
        assert!(NesneTuru::Blob.temel_mi());
        assert!(!NesneTuru::RefDelta.temel_mi());
        assert!(!NesneTuru::OfsDelta.temel_mi());
    }
}
