//! `git log` benzeri geçmiş yürüyüşü ve dosya yolu filtresi.
//!
//! Yürüyüş git'in varsayılan `--date-order` davranışını taklit eder: commit'ler
//! committer zamanına göre **yeniden eskiye** sıralanır; aynı zaman damgasında **keşif
//! sırası** korunur. Böylece birleşim commit'lerinde hangi kolun önce yürütüleceği
//! deterministiktir ve iki çalıştırma arasında çıktı değişmez.
//!
//! Yürüyüş akış hâlindedir: tüm commit'ler bellekte tutulmaz, yalnızca ziyaret edilmiş
//! küme (200 bin commit'te ~5 MB) ve istenen kadar kayıt tutulur. Bu, raporun b08
//! bellek bütçesinin (260 MB tepe RSS) anahtar noktasıdır.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

use crate::agac::{yol_coz, YolSonucu};
use crate::commit::Commit;
use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::oid::Oid;

/// Geçmiş listesindeki tek bir kayıt.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GecmisKaydi {
    /// Commit'in kısa adı (7 karakter).
    pub kisa: String,
    /// Commit'in tam adı.
    pub oid: String,
    /// Yazarın kayıttaki adı (e-posta **gösterilmez**).
    pub yazar: String,
    /// Committer zamanı (Unix saniyesi).
    pub zaman: i64,
    /// Saat dilimi.
    pub bolge: String,
    /// Konu satırı.
    pub konu: String,
    /// Birinci ebeveyne göre değişen dosya sayısı.
    pub dosya_sayisi: usize,
    /// Ebeveyn sayısı (0 = kök commit).
    pub ebeveyn_sayisi: usize,
}

/// Öncelik kuyruğu girdisi: en yeni ve en erken keşfedilen önce çıkar.
#[derive(PartialEq, Eq)]
struct OncelikGirdisi {
    zaman: i64,
    sira: u64,
    oid: Oid,
}

impl Ord for OncelikGirdisi {
    fn cmp(&self, diger: &Self) -> Ordering {
        // `BinaryHeap` en büyüğü öne aldığı için "önce çıkması gereken" büyük sayılır:
        // yeni zaman damgası büyük, eşit zamanda küçük keşif sırası büyük olur.
        self.zaman
            .cmp(&diger.zaman)
            .then_with(|| diger.sira.cmp(&self.sira))
            .then_with(|| diger.oid.cmp(&self.oid))
    }
}

impl PartialOrd for OncelikGirdisi {
    fn partial_cmp(&self, diger: &Self) -> Option<Ordering> {
        Some(self.cmp(diger))
    }
}

/// Geçmiş yürüyüşünün seçenekleri.
#[derive(Clone, Debug, Default)]
pub struct GecmisSecenekleri {
    /// Yalnızca bu dosya yoluna dokunan commit'ler listelensin.
    pub yol: Option<String>,
    /// En fazla bu kadar kayıt döndürülsün (`None` = sınırsız).
    pub limit: Option<usize>,
}

/// `baslangic` commit'inden yürüyerek geçmiş kayıtlarını üretir.
pub fn gecmis(
    magaza: &Magaza,
    baslangic: &Oid,
    secenekler: &GecmisSecenekleri,
) -> Result<Vec<GecmisKaydi>, Hata> {
    let mut kuyruk: BinaryHeap<OncelikGirdisi> = BinaryHeap::new();
    let mut ziyaret: HashSet<Oid> = HashSet::new();
    let mut kayitlar: Vec<GecmisKaydi> = Vec::new();
    let mut sira = 0u64;

    kuyruk.push(OncelikGirdisi {
        zaman: i64::MAX,
        sira: 0,
        oid: *baslangic,
    });

    while let Some(gelen) = kuyruk.pop() {
        if !ziyaret.insert(gelen.oid) {
            continue;
        }
        let commit = Commit::ayikla(magaza, &gelen.oid)?;
        let kapsar = match &secenekler.yol {
            Some(yol) => yolu_degistirdi(magaza, &commit, yol)?,
            None => true,
        };

        if kapsar {
            let degisiklik = degisiklik_sayisi(magaza, &commit)?;
            kayitlar.push(GecmisKaydi {
                kisa: commit.oid.onaltilik()[..7].to_string(),
                oid: commit.oid.onaltilik(),
                yazar: commit.yazar.gorunur_ad().to_string(),
                zaman: commit.yazar.zaman,
                bolge: commit.yazar.bolge.clone(),
                konu: commit.konu.clone(),
                dosya_sayisi: degisiklik,
                ebeveyn_sayisi: commit.ebeveynler.len(),
            });
            if let Some(limit) = secenekler.limit {
                if kayitlar.len() >= limit {
                    return Ok(kayitlar);
                }
            }
        }

        for ebeveyn in &commit.ebeveynler {
            let ebeveyn_commit = Commit::ayikla(magaza, ebeveyn)?;
            sira += 1;
            kuyruk.push(OncelikGirdisi {
                zaman: ebeveyn_commit.yazar.zaman,
                sira,
                oid: *ebeveyn,
            });
        }
    }

    Ok(kayitlar)
}

/// Bir commit'in belirtilen yola dokunup dokunmadığını bulur.
///
/// Karşılaştırma birinci ebeveyne yapılır. Kök commit'te karşılaştırma boş ağaca
/// yapılır. Sembolik bağlar ve alt modüller karşılaştırmaya girmez; alt modül
/// yolları için [`Hata::YolBirAltAgac`] döner.
fn yolu_degistirdi(magaza: &Magaza, commit: &Commit, yol: &str) -> Result<bool, Hata> {
    let yeni = yol_coz(magaza, &commit.agac, yol)?;
    let eski = match commit.ebeveynler.first() {
        Some(ebeveyn) => {
            let ebeveyn_commit = Commit::ayikla(magaza, ebeveyn)?;
            yol_coz(magaza, &ebeveyn_commit.agac, yol)?
        }
        None => YolSonucu::Yok,
    };
    Ok(yeni != eski)
}

/// Bir commit'in birinci ebeveyne göre değiştirdiği yol sayısı.
pub fn degisiklik_sayisi(magaza: &Magaza, commit: &Commit) -> Result<usize, Hata> {
    let ebeveyn_agac = match commit.ebeveynler.first() {
        Some(ebeveyn) => Some(Commit::ayikla(magaza, ebeveyn)?.agac),
        None => None,
    };
    let fark = crate::fark::agac_karsilastir(magaza, ebeveyn_agac.as_ref(), &commit.agac)?;
    Ok(fark.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oncelik(kisa: u64) -> OncelikGirdisi {
        OncelikGirdisi {
            zaman: 100 - kisa as i64,
            sira: kisa,
            oid: Oid::default(),
        }
    }

    #[test]
    fn oncelik_kuyrugu_yeniden_eskiye_siralar() {
        let mut kuyruk = BinaryHeap::new();
        kuyruk.push(oncelik(0));
        kuyruk.push(oncelik(2));
        kuyruk.push(oncelik(1));
        // `into_iter()` sıralı boşaltmaz (ham diziyi verir); sıra `pop()` ile denenir.
        let sira: Vec<u64> = std::iter::from_fn(|| kuyruk.pop().map(|g| g.sira)).collect();
        assert_eq!(sira, vec![0, 1, 2]);
    }

    #[test]
    fn oncelik_kuyrugu_ayni_zamanda_kesif_sirasina_gore_siralar() {
        let mut kuyruk = BinaryHeap::new();
        kuyruk.push(OncelikGirdisi {
            zaman: 10,
            sira: 5,
            oid: Oid::default(),
        });
        kuyruk.push(OncelikGirdisi {
            zaman: 10,
            sira: 2,
            oid: Oid::default(),
        });
        assert_eq!(kuyruk.pop().map(|g| g.sira), Some(2));
        assert_eq!(kuyruk.pop().map(|g| g.sira), Some(5));
    }
}
