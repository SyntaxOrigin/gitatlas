//! Commit (ve etiket) kayıtlarının ayrıştırılması.
//!
//! Commit gövdesi satır tabanlıdır: `tree`, `parent`, `author`, `committer` başlıkları,
//! isteğe bağlı `gpgsig` gibi çok satırlı başlıklar ve sonra boş satırdan sonra mesaj.
//!
//! **Gizlilik:** kimlik kaydında e-posta adresi ayrıştırılır ama hiçbir çıktıda
//! gösterilmez. `Kimlik::gorunur_ad` yalnızca kayıttaki **ad** alanını döndürür; bu,
//! raporun b10 bölümündeki "e-posta rapora sızmaz" kuralının mekanik uygulamasıdır.

use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::nesne::NesneTuru;
use crate::oid::Oid;

/// Commit kaydındaki kimlik satırı.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Kimlik {
    /// Kayıttaki ad (orneğin "Ada Lovelace"). Yalnızca bu alan çıktıya girer.
    pub ad: String,
    /// `zaman` alanı: Unix saniyesi.
    pub zaman: i64,
    /// `saat dilimi` alanı (ör. `+0300`).
    pub bolge: String,
}

impl Kimlik {
    /// Çıktıda gösterilecek ad.
    pub fn gorunur_ad(&self) -> &str {
        &self.ad
    }
}

/// Ayrıştırılmış bir commit kaydı.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Commit {
    /// Commit'in nesne adı.
    pub oid: Oid,
    /// Kök ağacın nesne adı.
    pub agac: Oid,
    /// Ebeveyn commit'ler (kök commit'te boş).
    pub ebeveynler: Vec<Oid>,
    /// Yazar.
    pub yazar: Kimlik,
    /// Commit'i yazan (rebase/cherry-pick sonrası farklı olabilir).
    pub gonderen: Kimlik,
    /// Konu satırı (mesajın ilk satırı).
    pub konu: String,
    /// Mesajın tamamı.
    pub mesaj: String,
}

impl Commit {
    /// `oid` nesnesini okuyup commit olarak ayrıştırır.
    pub fn ayikla(magaza: &Magaza, oid: &Oid) -> Result<Self, Hata> {
        let nesne = magaza.nesne(oid)?;
        if nesne.tur != NesneTuru::Commit {
            return Err(Hata::NesneBozuk {
                oid: oid.onaltilik(),
                ayrinti: format!("commit beklenirken {} bulundu", nesne.tur),
            });
        }
        Self::ayikla_ham(oid, &nesne.veri)
    }

    /// Ham commit gövdesini ayrıştırır (dosya erişimi olmadan).
    pub fn ayikla_ham(oid: &Oid, veri: &[u8]) -> Result<Self, Hata> {
        let bozuk = |ayrinti: &str| Hata::NesneBozuk {
            oid: oid.onaltilik(),
            ayrinti: ayrinti.to_string(),
        };

        let metin = String::from_utf8_lossy(veri);
        let mut satirlar = metin.split('\n');

        let mut agac: Option<Oid> = None;
        let mut ebeveynler: Vec<Oid> = Vec::new();
        let mut yazar: Option<Kimlik> = None;
        let mut gonderen: Option<Kimlik> = None;

        // Başlıklar: "anahtar değer". Değer bir sonraki satırla devam edebilir
        // (devam satırları bir boşlukla başlar); bu, `gpgsig` için gereklidir.
        let mut anahtar = String::new();
        let mut deger = String::new();

        for satir in satirlar.by_ref() {
            if satir.is_empty() {
                break;
            }
            if satir.starts_with(' ') && !deger.is_empty() {
                deger.push('\n');
                deger.push_str(&satir[1..]);
                continue;
            }
            if !deger.is_empty() {
                basligi_isle(
                    &anahtar,
                    &deger,
                    &mut agac,
                    &mut ebeveynler,
                    &mut yazar,
                    &mut gonderen,
                );
                anahtar.clear();
                deger.clear();
            }
            match satir.split_once(' ') {
                Some((k, v)) => {
                    anahtar = k.to_string();
                    deger = v.to_string();
                }
                None => return Err(bozuk(&format!("başlık satırı bozuk: {satir}"))),
            }
        }
        if !deger.is_empty() {
            basligi_isle(
                &anahtar,
                &deger,
                &mut agac,
                &mut ebeveynler,
                &mut yazar,
                &mut gonderen,
            );
        }

        let mesaj: String = satirlar.collect::<Vec<_>>().join("\n");
        let konu = mesaj.lines().next().unwrap_or("").to_string();

        let agac = agac.ok_or_else(|| bozuk("tree başlığı yok"))?;
        let yazar = yazar.ok_or_else(|| bozuk("author başlığı yok"))?;
        let gonderen = gonderen.unwrap_or_else(|| yazar.clone());

        Ok(Commit {
            oid: *oid,
            agac,
            ebeveynler,
            yazar,
            gonderen,
            konu,
            mesaj,
        })
    }
}

fn basligi_isle(
    anahtar: &str,
    deger: &str,
    agac: &mut Option<Oid>,
    ebeveynler: &mut Vec<Oid>,
    yazar: &mut Option<Kimlik>,
    gonderen: &mut Option<Kimlik>,
) {
    match anahtar {
        "tree" => {
            if let Ok(oid) = Oid::ayikla(deger) {
                *agac = Some(oid);
            }
        }
        "parent" => {
            if let Ok(oid) = Oid::ayikla(deger) {
                ebeveynler.push(oid);
            }
        }
        "author" => *yazar = kimlik_ayikla(deger),
        "committer" => *gonderen = kimlik_ayikla(deger),
        _ => {}
    }
}

/// `Ada Lovelace <ada@example.com> 1700000000 +0300` biçimini ayrıştırır.
fn kimlik_ayikla(deger: &str) -> Option<Kimlik> {
    let acilis = deger.rfind('<')?;
    let kapanis = deger[acilis..].find('>')? + acilis;
    let ad = deger[..acilis].trim().to_string();
    let kalan = deger[kapanis + 1..].trim();
    let mut parcalar = kalan.split_whitespace();
    let zaman = parcalar.next()?.parse::<i64>().ok()?;
    let bolge = parcalar.next().unwrap_or("+0000").to_string();
    Some(Kimlik { ad, zaman, bolge })
}

/// Etiket (tag) kaydı.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Etiket {
    /// Etiketin nesne adı.
    pub oid: Oid,
    /// Etiket nesnesinin işaret ettiği nesne (`object` başlığı).
    pub hedef: Oid,
    /// Hedefin türü (`type` başlığı).
    pub hedef_turu: String,
    /// Etiketin adı (`tag` başlığı).
    pub ad: String,
    /// Etiketleyen kişi.
    pub etiketleyen: Option<Kimlik>,
    /// Etiket mesajı.
    pub mesaj: String,
}

impl Etiket {
    /// `oid` nesnesini okuyup etiket olarak ayrıştırır.
    pub fn ayikla(magaza: &Magaza, oid: &Oid) -> Result<Self, Hata> {
        let nesne = magaza.nesne(oid)?;
        if nesne.tur != NesneTuru::Etiket {
            return Err(Hata::NesneBozuk {
                oid: oid.onaltilik(),
                ayrinti: format!("etiket beklenirken {} bulundu", nesne.tur),
            });
        }
        let metin = String::from_utf8_lossy(&nesne.veri);
        let mut hedef = None;
        let mut hedef_turu = String::new();
        let mut ad = String::new();
        let mut etiketleyen = None;
        let mut mesaj = String::new();
        let mut baslikta = true;

        for satir in metin.split('\n') {
            if baslikta {
                if satir.is_empty() {
                    baslikta = false;
                    continue;
                }
                match satir.split_once(' ') {
                    Some(("object", v)) => hedef = Oid::ayikla(v).ok(),
                    Some(("type", v)) => hedef_turu = v.to_string(),
                    Some(("tag", v)) => ad = v.to_string(),
                    Some(("tagger", v)) => etiketleyen = kimlik_ayikla(v),
                    _ => {}
                }
            } else {
                mesaj.push_str(satir);
                mesaj.push('\n');
            }
        }

        Ok(Etiket {
            oid: *oid,
            hedef: hedef.ok_or_else(|| Hata::NesneBozuk {
                oid: oid.onaltilik(),
                ayrinti: "object başlığı yok".to_string(),
            })?,
            hedef_turu,
            ad,
            etiketleyen,
            mesaj: mesaj.trim_end().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "1111111111111111111111111111111111111111";
    const OID2: &str = "2222222222222222222222222222222222222222";

    fn oid(s: &str) -> Oid {
        Oid::ayikla(s).expect("geçerli ad")
    }

    #[test]
    fn commit_ayristirilir() {
        let govde = format!(
            "tree {OID}\nparent {OID2}\nauthor Ada Lovelace <ada@example.com> 1700000000 +0300\n\
             committer Ada Lovelace <ada@example.com> 1700000000 +0300\n\nIlk konu\n\nGovde satiri\n"
        );
        let c = Commit::ayikla_ham(&oid(OID), govde.as_bytes()).expect("ayrıştırılmalı");
        assert_eq!(c.agac, oid(OID));
        assert_eq!(c.ebeveynler, vec![oid(OID2)]);
        assert_eq!(c.yazar.ad, "Ada Lovelace");
        assert_eq!(c.yazar.zaman, 1_700_000_000);
        assert_eq!(c.yazar.bolge, "+0300");
        assert_eq!(c.konu, "Ilk konu");
        assert!(c.mesaj.contains("Govde satiri"));
    }

    #[test]
    fn coklu_ebeveyn_ayristirilir() {
        let govde = format!(
            "tree {OID}\nparent {OID2}\nparent {OID}\nauthor A B <a@b.c> 1 +0000\n\
             committer A B <a@b.c> 1 +0000\n\nkonu\n"
        );
        let c = Commit::ayikla_ham(&oid(OID), govde.as_bytes()).expect("ayrıştırılmalı");
        assert_eq!(c.ebeveynler.len(), 2);
    }

    #[test]
    fn gpgsig_cok_satirli_baslik_yutulur() {
        let govde = format!(
            "tree {OID}\nauthor A B <a@b.c> 1 +0000\ncommitter A B <a@b.c> 1 +0000\n\
             gpgsig -----BEGIN-----\n \n -----END-----\n\nkonu\n"
        );
        let c = Commit::ayikla_ham(&oid(OID), govde.as_bytes()).expect("ayrıştırılmalı");
        assert_eq!(c.konu, "konu");
        assert_eq!(c.yazar.ad, "A B");
    }

    #[test]
    fn kok_commit_ebeveynsiz_ayristirilir() {
        let govde = format!(
            "tree {OID}\nauthor A B <a@b.c> 1 +0000\ncommitter A B <a@b.c> 1 +0000\n\nilk\n"
        );
        let c = Commit::ayikla_ham(&oid(OID), govde.as_bytes()).expect("ayrıştırılmalı");
        assert!(c.ebeveynler.is_empty());
    }

    #[test]
    fn tree_basligi_yoksa_hata_verir() {
        let govde = "author A B <a@b.c> 1 +0000\ncommitter A B <a@b.c> 1 +0000\n\nx\n";
        assert!(Commit::ayikla_ham(&oid(OID), govde.as_bytes()).is_err());
    }

    #[test]
    fn kimlik_satiri_ayristirilir() {
        let k = kimlik_ayikla("Ada <a@b.c> 42 -0500").expect("kimlik okunmalı");
        assert_eq!(k.ad, "Ada");
        assert_eq!(k.zaman, 42);
        assert_eq!(k.bolge, "-0500");
    }

    #[test]
    fn kimlik_satiri_bicim_disinda_hata_verir() {
        assert!(kimlik_ayikla("Ada 42").is_none());
    }
}
