//! Okuma ve çözme politikası: bellek tavanları, arama sırası ve delta derinliği.
//!
//! Bu modül, "doğru okumak" ile "öngörülebilir okumak" arasındaki tradeoff'u tek yerde
//! toplar. Her tavanın bir varsayılanı ve bir gerekçesi vardır.

use serde_json::Value;

/// `sha1 → nesne` sorgusunun hangi depoda önce arayacağını belirler.
///
/// Git'in `do_oid_object_info_extended` uygulaması **önce pack**, sonra gevşek nesne
/// arar. Bazı eski ve üçüncü taraf okuyucular ters sırayı kullanır; iki davranış da
/// burada seçilebilir ve `ozet` çıktısında hangisinin uygulandığı yazılır.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AramaSirasi {
    /// Önce pack, sonra gevşek nesne (git'in davranışı; varsayılan).
    #[default]
    PackOnce,
    /// Önce gevşek nesne, sonra pack.
    GevsekOnce,
}

impl AramaSirasi {
    /// Seçenek adını döndürür.
    pub fn ad(&self) -> &'static str {
        match self {
            AramaSirasi::PackOnce => "pack-once",
            AramaSirasi::GevsekOnce => "gevsek-once",
        }
    }

    /// Komut satırından gelen dizeyi çevirir (`pack-once`, `gevsek-once`).
    pub fn ayikla(dize: &str) -> Result<Self, String> {
        match dize {
            "pack-once" => Ok(AramaSirasi::PackOnce),
            "gevsek-once" => Ok(AramaSirasi::GevsekOnce),
            _ => Err(format!("geçersiz arama sırası: {dize}")),
        }
    }
}

/// GitAtlas'ın okuma davranışını belirleyen ayarlar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ayarlar {
    /// Tek bir nesnenin çözülmüş boyutunun üst sınırı (bayt).
    pub nesne_tavani: u64,
    /// Çözülmüş nesneler için bayt cinsinden önbellek kapasitesi.
    pub onbellek_bayt: usize,
    /// Delta zinciri için izin verilen azami çözümleme basamağı.
    pub delta_derinligi: u32,
    /// `sha1 → nesne` arama sırası.
    pub arama_sirasi: AramaSirasi,
    /// Unified diff çıktısında gösterilecek bağlam satırı sayısı.
    pub baglam: usize,
    /// `gecmis` alt komutunda listelenecek en fazla commit sayısı (0 = sınırsız).
    pub gecmis_limiti: usize,
}

impl Default for Ayarlar {
    /// Varsayılanlar README'de tablo hâlinde yazılıdır.
    fn default() -> Self {
        Ayarlar {
            nesne_tavani: 64 * 1024 * 1024,
            onbellek_bayt: 32 * 1024 * 1024,
            delta_derinligi: 64,
            arama_sirasi: AramaSirasi::default(),
            baglam: 3,
            gecmis_limiti: 0,
        }
    }
}

impl Ayarlar {
    /// Ayarların JSON çıktısındaki gösterimini üretir.
    pub fn json(&self) -> Value {
        serde_json::json!({
            "nesne_tavani_bayt": self.nesne_tavani,
            "onbellek_bayt": self.onbellek_bayt,
            "delta_derinligi_tavani": self.delta_derinligi,
            "arama_sirasi": self.arama_sirasi.ad(),
            "baglam_satiri": self.baglam,
            "gecmis_limiti": self.gecmis_limiti,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varsayilanlar_butce_icin_uygun() {
        let a = Ayarlar::default();
        assert_eq!(a.nesne_tavani, 64 * 1024 * 1024);
        assert_eq!(a.onbellek_bayt, 32 * 1024 * 1024);
        assert_eq!(a.delta_derinligi, 64);
        assert_eq!(a.arama_sirasi, AramaSirasi::PackOnce);
    }

    #[test]
    fn arama_sirasi_ad_ile_ayristirilir() {
        assert_eq!(
            AramaSirasi::ayikla("pack-once").expect("geçerli"),
            AramaSirasi::PackOnce
        );
        assert_eq!(
            AramaSirasi::ayikla("gevsek-once").expect("geçerli"),
            AramaSirasi::GevsekOnce
        );
        assert!(AramaSirasi::ayikla("karisik").is_err());
    }

    #[test]
    fn ayarlar_json_uretir() {
        let deger = Ayarlar::default().json();
        assert_eq!(deger["arama_sirasi"], "pack-once");
        assert_eq!(deger["delta_derinligi_tavani"], 64);
    }
}
