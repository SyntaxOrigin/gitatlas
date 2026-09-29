//! GitAtlas'ın bütün hataları için tek hata tipi ve elle yazılmış `Display` uygulaması.
//!
//! `thiserror` bağımlılık politikası gereği kullanılamaz (bkz. WORKER_CONTRACT.md § 4.3),
//! bu yüzden `Display` ve `Error` uygulamaları elle yazılmıştır.
//!
//! Kapsam: yalnızca **okuma** sırasında oluşabilecek hatalar. Yazma yeteneği yoktur,
//! dolayısıyla "izin yok" veya "salt okunur" türü çalışma zamanı hataları üretilmez.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// GitAtlas'ın ürettiği bütün hatalar.
#[derive(Debug)]
#[non_exhaustive]
pub enum Hata {
    /// Dosya sistemi işlemi başarısız oldu.
    Io {
        /// Erişilmeye çalışılan yol.
        yol: PathBuf,
        /// Alt katmandan gelen hata.
        kaynak: io::Error,
    },
    /// Verilen yol bir git deposu değil (`.git` bulunamadı).
    DepoBulunamadi {
        /// Aranan depo kökü.
        yol: PathBuf,
    },
    /// `.git/HEAD` okunamadı veya tanınmadı.
    HeadOkunamadi {
        /// Ayrıntılı açıklama.
        ayrinti: String,
    },
    /// `packed-refs` ayrıştırılamadı.
    PackedRefsBozuk {
        /// Hatalı satır.
        satir: String,
    },
    /// İstenen revizyon (HEAD, dal adı, etiket veya nesne adı) çözülemedi.
    RevCozulemedi {
        /// Kullanıcının verdiği revizyon dizesi.
        rev: String,
    },
    /// Nesne adı çözümlendi ama nesnenin kendisi bulunamadı.
    NesneBulunamadi {
        /// 40 onaltılık nesne adı.
        oid: String,
    },
    /// Nesne bulundu ama içeriği geçersiz (başlık, SHA-1 uyuşmazlığı, bozuk ağaç/commit).
    NesneBozuk {
        /// 40 onaltılık nesne adı.
        oid: String,
        /// Hatanın somut açıklaması.
        ayrinti: String,
    },
    /// zlib akışı çözülemedi (bozuk veya kesik).
    ZlibHatasi {
        /// Hatanın somut açıklaması.
        ayrinti: String,
    },
    /// Nesne çözme sırasında izin verilen bayt sınırı aşıldı.
    CozmeSiniriAsildi {
        /// Sınırı aşan miktar (bayt).
        bayt: u64,
        /// Uygulanan tavan (bayt).
        tav: u64,
    },
    /// `.idx` dosyasının sürümü desteklenmiyor (yalnızca v2 okunur).
    IndeksSurumuDesteklenmiyor {
        /// `.idx` dosyasının yolu.
        yol: PathBuf,
        /// Tespit edilen sürüm açıklaması.
        surum: String,
    },
    /// `.idx` dosyasının yapısı geçersiz (fanout/tablo tutarsızlığı, kesik dosya).
    IndeksBozuk {
        /// `.idx` dosyasının yolu.
        yol: PathBuf,
        /// Hatanın somut açıklaması.
        ayrinti: String,
    },
    /// `.pack` dosyasının yapısı geçersiz (imza, nesne başlığı, ofset).
    PaketBozuk {
        /// Hatanın somut açıklaması.
        ayrinti: String,
    },
    /// Delta zinciri izin verilen derinliği aştı.
    DeltaZinciriAsildi {
        /// Uygulanan tavan.
        tavan: u32,
    },
    /// Delta verisi geçersiz (komut, boyut veya kaynak sınırı ihlali).
    DeltaBozuk {
        /// Hatanın somut açıklaması.
        ayrinti: String,
    },
    /// Delta tabanı aranırken nesne kendisine bağlı çıktı (olası döngü).
    DeltaDongusu {
        /// Döngüde yakalanan nesne adı.
        oid: String,
    },
    /// Verilen depo içi dosya yolu geçersiz (`..`, mutlak yol, boş bileşen vb.).
    YolGecersiz {
        /// Kullanıcının verdiği yol.
        yol: String,
        /// Ret gerekçesi.
        gerekce: String,
    },
    /// Yol bir blob değil, alt ağaç (tree) çözümlendi.
    YolBirAltAgac {
        /// Çözümlenen yol.
        yol: String,
    },
    /// Çıktı üretimi başarısız oldu.
    CiktiHatasi {
        /// Hatanın somut açıklaması.
        ayrinti: String,
    },
}

impl Hata {
    /// Bir yol ve `io::Error`'dan [`Hata::Io`] üretir.
    pub fn io(yol: impl Into<PathBuf>, kaynak: io::Error) -> Self {
        Hata::Io {
            yol: yol.into(),
            kaynak,
        }
    }

    /// Bir yol ve açıklamadan [`Hata::Io`] üretir (`io::ErrorKind::InvalidData`).
    pub fn bozuk_veri(yol: impl Into<PathBuf>, ayrinti: impl Into<String>) -> Self {
        Hata::Io {
            yol: yol.into(),
            kaynak: io::Error::new(io::ErrorKind::InvalidData, ayrinti.into()),
        }
    }
}

impl fmt::Display for Hata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Hata::Io { yol, kaynak } => write!(f, "{} okunamadı: {}", yol.display(), kaynak),
            Hata::DepoBulunamadi { yol } => {
                write!(
                    f,
                    "{} bir git deposu değil (.git bulunamadı)",
                    yol.display()
                )
            }
            Hata::HeadOkunamadi { ayrinti } => write!(f, "HEAD okunamadı: {ayrinti}"),
            Hata::PackedRefsBozuk { satir } => {
                write!(f, "packed-refs bozuk satır: {satir}")
            }
            Hata::RevCozulemedi { rev } => write!(f, "revizyon çözülemedi: {rev}"),
            Hata::NesneBulunamadi { oid } => {
                write!(f, "nesne bulunamadı (loose ve pack içinde yok): {oid}")
            }
            Hata::NesneBozuk { oid, ayrinti } => write!(f, "nesne bozuk {oid}: {ayrinti}"),
            Hata::ZlibHatasi { ayrinti } => write!(f, "zlib akışı çözülemedi: {ayrinti}"),
            Hata::CozmeSiniriAsildi { bayt, tav } => {
                write!(f, "çözme sınırı aşıldı: {bayt} bayt > {tav} bayt")
            }
            Hata::IndeksSurumuDesteklenmiyor { yol, surum } => {
                write!(f, "{} desteklenmiyor (sürüm {surum})", yol.display())
            }
            Hata::IndeksBozuk { yol, ayrinti } => {
                write!(f, "{} bozuk: {ayrinti}", yol.display())
            }
            Hata::PaketBozuk { ayrinti } => write!(f, "pack bozuk: {ayrinti}"),
            Hata::DeltaZinciriAsildi { tavan } => {
                write!(f, "delta zincir derinliği tavanı ({tavan}) aşıldı")
            }
            Hata::DeltaBozuk { ayrinti } => write!(f, "delta bozuk: {ayrinti}"),
            Hata::DeltaDongusu { oid } => write!(f, "delta döngüsü tespit edildi: {oid}"),
            Hata::YolGecersiz { yol, gerekce } => write!(f, "geçersiz yol '{yol}': {gerekce}"),
            Hata::YolBirAltAgac { yol } => write!(f, "'{yol}' bir blob değil, alt ağaç"),
            Hata::CiktiHatasi { ayrinti } => write!(f, "çıktı üretilemedi: {ayrinti}"),
        }
    }
}

impl Error for Hata {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Hata::Io { kaynak, .. } => Some(kaynak),
            _ => None,
        }
    }
}
