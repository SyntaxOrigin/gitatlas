//! Bayt cinsinden sınırlı LRU nesne önbelleği.
//!
//! Neden "kaç nesne" değil "kaç bayt" kuralı: birkaç büyük blob, adet tabanlı bir
//! önbelleği tümüyle doldurup bellek bütçesini (260 MB tepe RSS) aşar. Bu önbellek
//! kapasiteyi bayt olarak tutar ve en eski kullanılanı önce atar.
//!
//! Kapsam: yalnızca bellek içi önbellekleme. Kalıcı bir indeks veya veritabanı yoktur.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::oid::Oid;

/// Önbellekteki tek bir giri.
#[derive(Clone)]
struct Kabuk {
    deger: Vec<u8>,
    bayt: usize,
    damga: u64,
}

/// Bayt cinsinden sınırlı, en son kullanılanı koruyan önbellek.
pub struct Onbellek {
    kapasite: usize,
    ic: RefCell<Ic>,
}

impl std::fmt::Debug for Onbellek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ic = self.ic.borrow();
        f.debug_struct("Onbellek")
            .field("kapasite", &self.kapasite)
            .field("giri_sayisi", &ic.giriler.len())
            .field("mevcut_bayt", &ic.mevcut_bayt)
            .finish()
    }
}

struct Ic {
    giriler: HashMap<Oid, Kabuk>,
    mevcut_bayt: usize,
    sayac: u64,
}

impl Onbellek {
    /// `kapasite` baytlık yeni bir önbellek üretir. Sıfır kapasite geçerlidir ve
    /// önbellek tamamen devre dışı kalır.
    pub fn yeni(kapasite: usize) -> Self {
        Onbellek {
            kapasite,
            ic: RefCell::new(Ic {
                giriler: HashMap::new(),
                mevcut_bayt: 0,
                sayac: 0,
            }),
        }
    }

    /// Depodaki bayt cinsinden kapasiteyi döndürür.
    pub fn kapasite(&self) -> usize {
        self.kapasite
    }

    /// Önbellekte tutulan toplam baytı döndürür.
    pub fn mevcut_bayt(&self) -> usize {
        self.ic.borrow().mevcut_bayt
    }

    /// Önbellekteki giri sayısını döndürür.
    pub fn giri_sayisi(&self) -> usize {
        self.ic.borrow().giriler.len()
    }

    /// `oid` için önbellekteki değeri varsa kopyasını döndürür ve giriyi tazeler.
    pub fn al(&self, oid: &Oid) -> Option<Vec<u8>> {
        if self.kapasite == 0 {
            return None;
        }
        let mut ic = self.ic.borrow_mut();
        let sayac = &mut ic.sayac;
        *sayac += 1;
        let damga = *sayac;
        match ic.giriler.get_mut(oid) {
            Some(kabuk) => {
                kabuk.damga = damga;
                Some(kabuk.deger.clone())
            }
            None => None,
        }
    }

    /// `oid` için değeri önbelleğe alır; kapasite aşılırsa en eski giri atılır.
    ///
    /// Değer tek başına kapasiteden büyükse hiç saklanmaz; aksi hâlde her eklemede
    /// önbellek boşalıp yeniden dolar, bu da sessiz bir bellek sızıntısına dönüşürdü.
    pub fn ekle(&self, oid: Oid, deger: Vec<u8>) {
        if self.kapasite == 0 {
            return;
        }
        let bayt = deger.len();
        if bayt > self.kapasite {
            return;
        }
        let mut ic = self.ic.borrow_mut();
        if let Some(eski) = ic.giriler.remove(&oid) {
            ic.mevcut_bayt -= eski.bayt;
        }
        ic.sayac += 1;
        let damga = ic.sayac;
        ic.mevcut_bayt += bayt;
        ic.giriler.insert(oid, Kabuk { deger, bayt, damga });

        while ic.mevcut_bayt > self.kapasite {
            let en_eski = ic
                .giriler
                .iter()
                .min_by_key(|(_, k)| k.damga)
                .map(|(k, _)| *k);
            match en_eski {
                Some(anahtar) => {
                    if let Some(kabuk) = ic.giriler.remove(&anahtar) {
                        ic.mevcut_bayt -= kabuk.bayt;
                    }
                }
                None => break,
            }
        }
    }

    /// Önbelleği tamamen boşaltır.
    pub fn temizle(&self) {
        let mut ic = self.ic.borrow_mut();
        ic.giriler.clear();
        ic.mevcut_bayt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(s: &str) -> Oid {
        let mut tam = s.repeat(40);
        tam.truncate(40);
        Oid::ayikla(&tam).expect("geçerli nesne adı")
    }

    #[test]
    fn bos_onbellek_oge_dondurmez() {
        let onbellek = Onbellek::yeni(1024);
        assert!(onbellek.al(&oid("a")).is_none());
    }

    #[test]
    fn eklenen_deger_tekrar_okunur() {
        let onbellek = Onbellek::yeni(1024);
        let a = oid("a");
        onbellek.ekle(a, b"veri".to_vec());
        assert_eq!(onbellek.al(&a), Some(b"veri".to_vec()));
        assert_eq!(onbellek.giri_sayisi(), 1);
    }

    #[test]
    fn kapasite_asilirsa_en_eski_atilir() {
        let onbellek = Onbellek::yeni(10);
        let a = oid("a");
        let b = oid("b");
        let c = oid("c");
        onbellek.ekle(a, vec![1; 4]);
        onbellek.ekle(b, vec![2; 4]);
        onbellek.ekle(c, vec![3; 4]);
        assert!(onbellek.al(&a).is_none(), "en eski girisi atılmalı");
        assert!(onbellek.al(&c).is_some());
        assert!(onbellek.mevcut_bayt() <= onbellek.kapasite());
    }

    #[test]
    fn kapasiteden_buyuk_deger_saklanmaz() {
        let onbellek = Onbellek::yeni(8);
        let a = oid("a");
        onbellek.ekle(a, vec![0; 64]);
        assert!(onbellek.al(&a).is_none());
        assert_eq!(onbellek.mevcut_bayt(), 0);
    }

    #[test]
    fn sifir_kapasite_hicbir_sey_tutmaz() {
        let onbellek = Onbellek::yeni(0);
        onbellek.ekle(oid("a"), b"x".to_vec());
        assert_eq!(onbellek.giri_sayisi(), 0);
    }

    #[test]
    fn temizle_bosaltir() {
        let onbellek = Onbellek::yeni(64);
        onbellek.ekle(oid("a"), vec![7; 16]);
        onbellek.temizle();
        assert_eq!(onbellek.giri_sayisi(), 0);
        assert_eq!(onbellek.mevcut_bayt(), 0);
    }

    #[test]
    fn ayni_anahtar_tekrar_eklenince_bayt_saymi_azalmaz() {
        let onbellek = Onbellek::yeni(1024);
        let a = oid("a");
        onbellek.ekle(a, vec![1; 16]);
        onbellek.ekle(a, vec![2; 16]);
        assert_eq!(onbellek.giri_sayisi(), 1);
        assert_eq!(onbellek.mevcut_bayt(), 16);
    }
}
