//! Ağaç (tree) girdilerinin ayrıştırılması ve depo içi yol çözümleme.
//!
//! Ağaç nesnesi `<mod> <ad>\0<20 bayt nesne adı>` üçlülerinin art arda gelmesinden
//! oluşur. Mod değerleri: `100644` (dosya), `100755` (çalıştırılabilir), `120000`
//! (sembolik bağ), `40000` (alt ağaç), `160000` (alt modül / gitlink).
//!
//! Yol çözümlemesi güvenlik açısından katıdır: `..`, `.`, mutlak yol, boş bileşen,
//! geri eğik çizgi ve kontrol karakteri **reddedilir**. Böylece dışarıdan gelen bir
//! yol ile depo dışına çıkılamaz ve `HEAD` gibi özel bir referans yanlışlıkla okunamaz.

use std::collections::BTreeMap;

use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::oid::Oid;

/// Ağaç içindeki tek bir girdi.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AgacGirdi {
    /// Dosya sistemi modu (`100644`, `40000`, …).
    pub mod_bilgi: String,
    /// Girdinin adı (tam yol değil, bir bileşen).
    pub ad: String,
    /// Girdinin işaret ettiği nesne.
    pub oid: Oid,
}

impl AgacGirdi {
    /// Girdi bir alt ağacı mı gösterir?
    pub fn agac_mi(&self) -> bool {
        self.mod_bilgi == "40000" || self.mod_bilgi == "040000"
    }

    /// Girdi bir alt modül (gitlink) mi gösterir?
    pub fn modul_mi(&self) -> bool {
        self.mod_bilgi == "160000"
    }
}

/// Ham ağaç gövdesini ayrıştırır.
pub fn ayikla(veri: &[u8]) -> Result<Vec<AgacGirdi>, Hata> {
    let mut girdiler = Vec::new();
    let mut i = 0usize;
    while i < veri.len() {
        let bosluk = veri[i..]
            .iter()
            .position(|b| *b == b' ')
            .map(|p| i + p)
            .ok_or_else(|| Hata::NesneBozuk {
                oid: "-".to_string(),
                ayrinti: "ağaç girdisinde mod alanı yok".to_string(),
            })?;
        let mod_bilgi = String::from_utf8_lossy(&veri[i..bosluk]).to_string();
        let nul = veri[bosluk + 1..]
            .iter()
            .position(|b| *b == 0)
            .map(|p| bosluk + 1 + p)
            .ok_or_else(|| Hata::NesneBozuk {
                oid: "-".to_string(),
                ayrinti: "ağaç girdisinde ad sonlandırıcısı yok".to_string(),
            })?;
        let ad = String::from_utf8_lossy(&veri[bosluk + 1..nul]).to_string();
        let sha_bas = nul + 1;
        if sha_bas + 20 > veri.len() {
            return Err(Hata::NesneBozuk {
                oid: "-".to_string(),
                ayrinti: format!("ağaç girdisi ({ad}) nesne adı için bayt yok"),
            });
        }
        let mut sha = [0u8; 20];
        sha.copy_from_slice(&veri[sha_bas..sha_bas + 20]);
        girdiler.push(AgacGirdi {
            mod_bilgi,
            ad,
            oid: Oid::baytlardan(sha),
        });
        i = sha_bas + 20;
    }
    Ok(girdiler)
}

/// Bir ağacın girdilerini ada göre eşleme çevirir (son girdi kazanır, git sıralaması).
pub fn harita(girdiler: &[AgacGirdi]) -> BTreeMap<&str, &AgacGirdi> {
    girdiler.iter().map(|g| (g.ad.as_str(), g)).collect()
}

/// Bir depo içi yolun geçerli olup olmadığını denetler ve bileşenlere ayırır.
///
/// Kabul edilen biçim: `/` ile ayrılmış, göreli, `..` ve `.` içermeyen bileşenler.
pub fn yolu_ayikla(yol: &str) -> Result<Vec<String>, Hata> {
    let gecersiz = |gerekce: &str| Hata::YolGecersiz {
        yol: yol.to_string(),
        gerekce: gerekce.to_string(),
    };

    if yol.is_empty() {
        return Err(gecersiz("yol boş"));
    }
    if yol.starts_with('/') {
        return Err(gecersiz("mutlak yol kabul edilmez"));
    }
    if yol.contains('\\') {
        return Err(gecersiz("geri eğik çizgi kullanılamaz"));
    }
    if yol.ends_with('/') {
        return Err(gecersiz("sonuçta bölüç olamaz"));
    }
    if yol.chars().any(|c| c.is_control()) {
        return Err(gecersiz("kontrol karakteri içeriyor"));
    }

    let mut bilesenler = Vec::new();
    for parca in yol.split('/') {
        match parca {
            "" => return Err(gecersiz("boş bileşen (// içeriyor)")),
            "." => return Err(gecersiz("'.' bileşeni kullanılamaz")),
            ".." => return Err(gecersiz("'..' bileşeni kullanılamaz")),
            ".git" => return Err(gecersiz("'.git' bileşeni kullanılamaz")),
            _ => bilesenler.push(parca.to_string()),
        }
    }
    Ok(bilesenler)
}

/// Yolun bir ağaçta nereye karşılık geldiği.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum YolSonucu {
    /// Bir blob (dosya) bulundu.
    Dosya {
        /// Blob nesne adı.
        oid: Oid,
        /// Dosya modu.
        mod_bilgi: String,
    },
    /// Bir alt ağaç bulundu (yol bir dizine işaret ediyor).
    Agac(Oid),
    /// Yol bulunamadı.
    Yok,
    /// Yol bir alt modülü (gitlink) gösteriyor; içeriği okunamaz.
    Modul(Oid),
}

impl YolSonucu {
    /// Sonucun dosya olup olmadığını bildirir.
    pub fn dosya_mi(&self) -> bool {
        matches!(self, YolSonucu::Dosya { .. })
    }
}

/// `yol`u `kok_agac` altında çözer.
pub fn yol_coz(magaza: &Magaza, kok_agac: &Oid, yol: &str) -> Result<YolSonucu, Hata> {
    let bilesenler = yolu_ayikla(yol)?;
    let mut simdiki = *kok_agac;
    let son = bilesenler.len() - 1;

    for (indeks, bilesen) in bilesenler.iter().enumerate() {
        let girdiler = agac_girdileri(magaza, &simdiki)?;
        let harita = harita(&girdiler);
        let giris = harita.get(bilesen.as_str()).ok_or(Hata::NesneBulunamadi {
            oid: format!("{bilesen} ({} içinde)", simdiki.onaltilik()),
        })?;

        if indeks == son {
            if giris.agac_mi() {
                return Ok(YolSonucu::Agac(giris.oid));
            }
            if giris.modul_mi() {
                return Ok(YolSonucu::Modul(giris.oid));
            }
            return Ok(YolSonucu::Dosya {
                oid: giris.oid,
                mod_bilgi: giris.mod_bilgi.clone(),
            });
        }

        if !giris.agac_mi() {
            return Err(Hata::NesneBulunamadi {
                oid: format!("{bilesen} bir dosya; ara dizin değil"),
            });
        }
        simdiki = giris.oid;
    }

    Ok(YolSonucu::Yok)
}

/// Bir ağaç nesnesinin girdilerini okur.
pub fn agac_girdileri(magaza: &Magaza, oid: &Oid) -> Result<Vec<AgacGirdi>, Hata> {
    let nesne = magaza.nesne(oid)?;
    if nesne.tur != crate::nesne::NesneTuru::Agac {
        return Err(Hata::NesneBozuk {
            oid: oid.onaltilik(),
            ayrinti: format!("ağaç beklenirken {} bulundu", nesne.tur),
        });
    }
    ayikla(&nesne.veri)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(s: &str) -> Oid {
        let mut tam = s.repeat(40);
        tam.truncate(40);
        Oid::ayikla(&tam).expect("geçerli ad")
    }

    fn girdi(mod_bilgi: &str, ad: &str, s: &str) -> Vec<u8> {
        let mut v = format!("{mod_bilgi} {ad}\0").into_bytes();
        v.extend_from_slice(oid(s).baytlar());
        v
    }

    #[test]
    fn agac_ayristirilir() {
        let mut veri = girdi("100644", "a.txt", "a");
        veri.extend_from_slice(&girdi("40000", "src", "b"));
        veri.extend_from_slice(&girdi("120000", "bag", "c"));
        let girdiler = ayikla(&veri).expect("ağaç okunmalı");
        assert_eq!(girdiler.len(), 3);
        assert_eq!(girdiler[0].ad, "a.txt");
        assert!(girdiler[1].agac_mi());
        assert!(!girdiler[1].modul_mi());
        assert_eq!(girdiler[2].mod_bilgi, "120000");
    }

    #[test]
    fn modul_girdi_isaretlenir() {
        let veri = girdi("160000", "alt-modul", "d");
        let girdiler = ayikla(&veri).expect("ağaç okunmalı");
        assert!(girdiler[0].modul_mi());
    }

    #[test]
    fn yolu_ayikla_normal_yol() {
        assert_eq!(
            yolu_ayikla("src/lib.rs").expect("geçerli"),
            vec!["src".to_string(), "lib.rs".to_string()]
        );
    }

    #[test]
    fn yolu_ayikla_iki_nokta_reder() {
        assert!(matches!(
            yolu_ayikla("../gizli"),
            Err(Hata::YolGecersiz { .. })
        ));
        assert!(matches!(
            yolu_ayikla("src/../../gizli"),
            Err(Hata::YolGecersiz { .. })
        ));
    }

    #[test]
    fn yolu_ayikla_mutlak_yol_reder() {
        assert!(matches!(
            yolu_ayikla("/etc/passwd"),
            Err(Hata::YolGecersiz { .. })
        ));
    }

    #[test]
    fn yolu_ayikla_bos_ve_nokta_reder() {
        assert!(yolu_ayikla("").is_err());
        assert!(yolu_ayikla(".").is_err());
        assert!(yolu_ayikla("a//b").is_err());
        assert!(yolu_ayikla("a/").is_err());
    }

    #[test]
    fn yolu_ayikla_geri_egik_reder() {
        assert!(matches!(
            yolu_ayikla("src\\lib.rs"),
            Err(Hata::YolGecersiz { .. })
        ));
    }

    #[test]
    fn yolu_ayikla_git_bileseni_reder() {
        assert!(matches!(
            yolu_ayikla(".git/config"),
            Err(Hata::YolGecersiz { .. })
        ));
    }

    #[test]
    fn yolu_ayikla_kontrol_karakteri_reder() {
        assert!(matches!(
            yolu_ayikla("a\u{0}b"),
            Err(Hata::YolGecersiz { .. })
        ));
    }

    #[test]
    fn kesik_agac_hata_verir() {
        assert!(ayikla(b"100644 ad\0kisa").is_err());
        assert!(ayikla(b"100644 ad").is_err());
    }
}
