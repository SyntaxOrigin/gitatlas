//! Depo açma: `.git` keşfi, `HEAD`, `refs/heads`, `refs/tags` ve `packed-refs` okuma.
//!
//! **Salt okunur sözleşmesi:** bu modül yalnızca `File::open`, `read_to_string` ve
//! `read_dir` kullanır. Hiçbir yol `File::create`, `OpenOptions::write`, `rename` veya
//! `remove_file` çağırmaz; `refs` yazma ve `index-pack` bilinçli olarak uygulanmamıştır.
//!
//! **Öncelik kuralı:** aynı referans hem ayrışık `refs/…` dosyasında hem `packed-refs`
//! içinde bulunabilir. Ayrışık dosya kazanır — git'in davranışı budur ve
//! `git pack-refs` ayrışık dosyaları sildikten sonra kayıt paketli kopyaya düşer.
//! Yalnızca ayrışıkta bulunanlar `kaynak: "ayrisik"`, yalnızca pakettekiler
//! `kaynak: "packed"` olarak raporlanır; işaretli etiketlerin çözülmüş hedefi
//! `packed-refs` içindeki `^` satırından gelir ve ayrışık dosya onu ezmez.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::oid::Oid;

/// HEAD'in işaret ettiği yer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HeadDurumu {
    /// `ref: refs/heads/main` gibi bir sembolik referans.
    Dal(String),
    /// Doğrudan bir nesne adına bağlanmış (detached HEAD).
    Ayrik(Oid),
    /// HEAD bir dala işaret ediyor ama o dal henüz yok (unborn branch).
    Dogmadi(String),
}

impl HeadDurumu {
    /// HEAD'in okunabilir bir nesne adına çözülüp çözülemediğini bildirir.
    pub fn cozulebilir_mi(&self) -> bool {
        matches!(self, HeadDurumu::Dal(_) | HeadDurumu::Ayrik(_))
    }
}

/// Referansın nereden okunduğu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReferansKaynagi {
    /// Ayrışık `refs/…` dosyası.
    Ayrisik,
    /// `packed-refs` satırı.
    Packed,
}

impl ReferansKaynagi {
    /// JSON çıktısında kullanılan ad.
    pub fn ad(&self) -> &'static str {
        match self {
            ReferansKaynagi::Ayrisik => "ayrisik",
            ReferansKaynagi::Packed => "packed",
        }
    }
}

/// Tek bir referans kaydı.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Referans {
    /// Tam referans adı (`refs/heads/main`).
    pub ad: String,
    /// İşaret ettiği nesne.
    pub oid: Oid,
    /// Kaynağı (ayrışık mı paketli mi).
    pub kaynak: ReferansKaynagi,
    /// İşaretli (annotated) etiketin çözülmüş commit'i; yalnızca etiketlerde `Some`.
    pub soyulmus: Option<Oid>,
}

impl Referans {
    /// Referansın kısa adını döndürür (`refs/heads/ana/dal` → `ana/dal`).
    pub fn kisa_ad(&self) -> &str {
        self.ad
            .strip_prefix("refs/heads/")
            .or_else(|| self.ad.strip_prefix("refs/tags/"))
            .unwrap_or(&self.ad)
    }
}

/// `packed-refs` satırından okunan kayıt.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PackedKayit {
    /// Tam referans adı.
    pub ad: String,
    /// Referansın nesne adı.
    pub oid: Oid,
    /// İşaretli etiket için çözülmüş hedef (`^` satırından gelir).
    pub soyulmus: Option<Oid>,
}

/// Salt okunur açılmış bir git deposu.
#[derive(Debug)]
pub struct Depo {
    kok: PathBuf,
    git_dir: PathBuf,
    ham: bool,
    head: HeadDurumu,
    head_metni: String,
    referanslar: BTreeMap<String, Referans>,
    alternates: Vec<PathBuf>,
}

impl Depo {
    /// Verilen yoldan git deposunu açar.
    ///
    /// Kabul edilen biçimler: çalışma ağacı kökü (`.git` dizini içerir), `.git` dizininin
    /// kendisi, çıplak depo, veya `gitdir: …` yönlendirmesi içeren `.git` dosyası.
    pub fn ac(yol: &Path) -> Result<Self, Hata> {
        let kok = normalizasyon(&yol.canonicalize().map_err(|hata| Hata::io(yol, hata))?);
        let (git_dir, ham) = git_dizini_bul(&kok)?;
        let (head, head_metni) = head_oku(&git_dir.join("HEAD"))?;

        let mut referanslar: BTreeMap<String, Referans> = BTreeMap::new();
        for kayit in packed_refs_oku(&git_dir.join("packed-refs"))? {
            referanslar.insert(
                kayit.ad.clone(),
                Referans {
                    ad: kayit.ad,
                    oid: kayit.oid,
                    kaynak: ReferansKaynagi::Packed,
                    soyulmus: kayit.soyulmus,
                },
            );
        }

        for onek in ["refs/heads", "refs/tags"] {
            let dizin = git_dir.join(onek);
            let mut yollar = Vec::new();
            if dizin.is_dir() {
                topla_yollar(&dizin, &mut yollar);
            }
            yollar.sort();
            for yol_dosya in yollar {
                let Some(ad) = dosya_yolundan_referans_adi(&git_dir, &yol_dosya) else {
                    continue;
                };
                let Ok(metin) = std::fs::read_to_string(&yol_dosya) else {
                    continue;
                };
                let Ok(oid) = referans_adini_coz(&ad, metin.trim()) else {
                    continue;
                };
                let soyulmus = referanslar.get(&ad).and_then(|r| r.soyulmus);
                referanslar.insert(
                    ad.clone(),
                    Referans {
                        ad,
                        oid,
                        kaynak: ReferansKaynagi::Ayrisik,
                        soyulmus,
                    },
                );
            }
        }

        // Unborn branch: HEAD bir dala işaret ediyor ama referans dosyası yok.
        let head = match &head {
            HeadDurumu::Dal(ad) if !referanslar.contains_key(ad) => HeadDurumu::Dogmadi(ad.clone()),
            diger => diger.clone(),
        };

        let alternates = alternates_oku(&git_dir.join("objects/info/alternates"))?;

        Ok(Depo {
            kok,
            git_dir,
            ham,
            head,
            head_metni,
            referanslar,
            alternates,
        })
    }

    /// Deponun çalışma ağacı kökünü döndürür.
    pub fn kok(&self) -> &Path {
        &self.kok
    }

    /// Deponun `.git` dizinini döndürür.
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Deponun nesne dizinini döndürür.
    pub fn objects_dir(&self) -> PathBuf {
        self.git_dir.join("objects")
    }

    /// Deponun çıplak (bare) olup olmadığını bildirir.
    pub fn ham_mi(&self) -> bool {
        self.ham
    }

    /// HEAD'in durumunu döndürür.
    pub fn head(&self) -> &HeadDurumu {
        &self.head
    }

    /// HEAD dosyasının ham içeriğini döndürür.
    pub fn head_metni(&self) -> &str {
        &self.head_metni
    }

    /// HEAD'in sembolik olarak işaret ettiği tam referans adı.
    pub fn head_referansi(&self) -> Option<String> {
        let metin = self.head_metni.trim();
        metin.strip_prefix("ref:").map(|ad| ad.trim().to_string())
    }

    /// HEAD doğrudan bir nesne adına bağlanmışsa o adı döndürür (detached HEAD).
    pub fn head_ayrik(&self) -> Option<Oid> {
        Oid::ayikla(self.head_metni.trim()).ok()
    }

    /// Tüm referansları ada göre sıralı döndürür.
    pub fn referanslar(&self) -> impl Iterator<Item = &Referans> {
        self.referanslar.values()
    }

    /// `ad` (ör. `refs/heads/main`) referansını bulur.
    pub fn referans(&self, ad: &str) -> Option<&Referans> {
        self.referanslar.get(ad)
    }

    /// `refs/heads/` altındaki dalları sıralı döndürür.
    pub fn dallar(&self) -> impl Iterator<Item = &Referans> {
        self.referanslar
            .values()
            .filter(|r| r.ad.starts_with("refs/heads/"))
    }

    /// `refs/tags/` altındaki etiketleri sıralı döndürür.
    pub fn etiketler(&self) -> impl Iterator<Item = &Referans> {
        self.referanslar
            .values()
            .filter(|r| r.ad.starts_with("refs/tags/"))
    }

    /// Depoda tanımlı `objects/info/alternates` yollarını döndürür.
    ///
    /// Git bu nesneleri dışarıdan getirir; GitAtlas yalnızca **bildirir** ve izlemez,
    /// çünkü izlemek yerel dosya sistemi dışına çıkmak anlamına gelir.
    pub fn alternates(&self) -> &[PathBuf] {
        &self.alternates
    }

    /// Bir revizyon dizesini nesne adına çevirir.
    ///
    /// Desteklenen biçimler: `HEAD`, `HEAD~N` (birinci ebeveyn izleyerek), tam `refs/…`
    /// adı, dal/etiket kısa adı, ve 40 onaltılık karakterlik nesne adı. Kısaltılmış
    /// nesne adları (7–39 karakter) **çözülmez**; belirsizlik üretmemek içindir.
    pub fn coz(&self, magaza: &Magaza, rev: &str) -> Result<Oid, Hata> {
        let rev = rev.trim();
        if rev.is_empty() {
            return Err(Hata::RevCozulemedi {
                rev: rev.to_string(),
            });
        }

        if rev == "HEAD" || rev.starts_with("HEAD~") {
            let baslangic = self.head_oid(magaza)?;
            let adim = rev.strip_prefix("HEAD~").unwrap_or("0");
            let adim = adim.parse::<usize>().map_err(|_| Hata::RevCozulemedi {
                rev: rev.to_string(),
            })?;
            return ebeveyn_izle(magaza, baslangic, adim);
        }

        if Oid::gecerli_mi(rev) {
            return Oid::ayikla(rev);
        }

        for ad in [
            rev.to_string(),
            format!("refs/heads/{rev}"),
            format!("refs/tags/{rev}"),
        ] {
            if let Some(kayit) = self.referanslar.get(&ad) {
                return Ok(kayit.soyulmus.unwrap_or(kayit.oid));
            }
        }

        Err(Hata::RevCozulemedi {
            rev: rev.to_string(),
        })
    }

    /// HEAD'in işaret ettiği nesne adını döndürür.
    pub fn head_oid(&self, _magaza: &Magaza) -> Result<Oid, Hata> {
        match self.head() {
            HeadDurumu::Ayrik(oid) => Ok(*oid),
            HeadDurumu::Dal(ad) => self
                .referanslar
                .get(ad)
                .map(|r| r.soyulmus.unwrap_or(r.oid))
                .ok_or_else(|| Hata::RevCozulemedi {
                    rev: "HEAD".to_string(),
                }),
            HeadDurumu::Dogmadi(ad) => Err(Hata::RevCozulemedi {
                rev: format!("HEAD → {ad} (henüz doğmamış dal)"),
            }),
        }
    }
}

fn ebeveyn_izle(magaza: &Magaza, baslangic: Oid, adim: usize) -> Result<Oid, Hata> {
    let mut simdiki = baslangic;
    let mut gecilen: Vec<Oid> = Vec::new();
    for _ in 0..adim {
        if gecilen.contains(&simdiki) {
            return Err(Hata::RevCozulemedi {
                rev: format!("HEAD~{adim} (ebeveyn döngüsü)"),
            });
        }
        gecilen.push(simdiki);
        let commit = crate::commit::Commit::ayikla(magaza, &simdiki)?;
        simdiki = *commit.ebeveynler.first().ok_or(Hata::RevCozulemedi {
            rev: format!("{simdiki} bir kök commit (ebeveyni yok)"),
        })?;
    }
    Ok(simdiki)
}

/// Windows'un `\\?\` (verbatim) yol önekini kaldırır.
///
/// `canonicalize()` bu öneki ekler; JSON/Markdown çıktısında `\\?\C:\...` biçimi
/// okunabilirliği bozar ve boru hattında beklenmedik bir dize üretir.
fn normalizasyon(yol: &Path) -> PathBuf {
    let metin = yol.to_string_lossy();
    match metin.strip_prefix(r"\\?\") {
        Some(kisa) => PathBuf::from(kisa),
        None => yol.to_path_buf(),
    }
}

fn git_dizini_bul(kok: &Path) -> Result<(PathBuf, bool), Hata> {
    let aday = kok.join(".git");
    if aday.is_dir() {
        return Ok((aday, false));
    }
    if aday.is_file() {
        let metin = std::fs::read_to_string(&aday).map_err(|hata| Hata::io(&aday, hata))?;
        if let Some(hedef) = metin.trim().strip_prefix("gitdir:") {
            let yol = Path::new(hedef.trim());
            let cozulen = if yol.is_absolute() {
                yol.to_path_buf()
            } else {
                aday.parent().unwrap_or(kok).join(yol)
            };
            if cozulen.is_dir() {
                return Ok((cozulen, false));
            }
        }
    }
    // Çıplak depo: HEAD, objects ve refs yan yana durur.
    if kok.join("HEAD").is_file() && kok.join("objects").is_dir() && kok.join("refs").is_dir() {
        return Ok((kok.to_path_buf(), true));
    }
    Err(Hata::DepoBulunamadi {
        yol: kok.to_path_buf(),
    })
}

fn head_oku(yol: &Path) -> Result<(HeadDurumu, String), Hata> {
    let metin = std::fs::read_to_string(yol).map_err(|hata| Hata::io(yol, hata))?;
    let satir = metin.trim().to_string();
    if satir.is_empty() {
        return Err(Hata::HeadOkunamadi {
            ayrinti: "dosya boş".to_string(),
        });
    }
    if let Some(hedef) = satir.strip_prefix("ref:") {
        let ad = hedef.trim();
        if !ad.starts_with("refs/") {
            return Err(Hata::HeadOkunamadi {
                ayrinti: format!("refs/ ile başlamayan referans: {ad}"),
            });
        }
        return Ok((HeadDurumu::Dal(ad.to_string()), satir));
    }
    match Oid::ayikla(&satir) {
        Ok(oid) => Ok((HeadDurumu::Ayrik(oid), satir)),
        Err(_) => Err(Hata::HeadOkunamadi {
            ayrinti: format!("tanınmayan HEAD içeriği: {satir}"),
        }),
    }
}

/// `packed-refs` dosyasını okur. Yorum satırları atlanır, `^` satırları bir önceki
/// referansın çözülmüş hedefi olarak eklenir.
pub fn packed_refs_oku(yol: &Path) -> Result<Vec<PackedKayit>, Hata> {
    let mut kayitlar: Vec<PackedKayit> = Vec::new();
    if !yol.is_file() {
        return Ok(kayitlar);
    }
    let metin = std::fs::read_to_string(yol).map_err(|hata| Hata::io(yol, hata))?;
    for satir in metin.lines() {
        let satir = satir.trim();
        if satir.is_empty() || satir.starts_with('#') {
            continue;
        }
        if let Some(soyulmus) = satir.strip_prefix('^') {
            let oid = Oid::ayikla(soyulmus).map_err(|_| Hata::PackedRefsBozuk {
                satir: satir.to_string(),
            })?;
            let son = kayitlar.last_mut().ok_or_else(|| Hata::PackedRefsBozuk {
                satir: satir.to_string(),
            })?;
            son.soyulmus = Some(oid);
            continue;
        }
        let mut parcalar = satir.splitn(2, ' ');
        let oid_metni = parcalar.next().unwrap_or_default();
        let ad = parcalar.next().unwrap_or_default().trim().to_string();
        if ad.is_empty() {
            return Err(Hata::PackedRefsBozuk {
                satir: satir.to_string(),
            });
        }
        let oid = Oid::ayikla(oid_metni).map_err(|_| Hata::PackedRefsBozuk {
            satir: satir.to_string(),
        })?;
        kayitlar.push(PackedKayit {
            ad,
            oid,
            soyulmus: None,
        });
    }
    Ok(kayitlar)
}

fn alternates_oku(yol: &Path) -> Result<Vec<PathBuf>, Hata> {
    if !yol.is_file() {
        return Ok(Vec::new());
    }
    let metin = std::fs::read_to_string(yol).map_err(|hata| Hata::io(yol, hata))?;
    Ok(metin
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(PathBuf::from)
        .collect())
}

fn topla_yollar(dizin: &Path, cikti: &mut Vec<PathBuf>) {
    let Ok(girdiler) = std::fs::read_dir(dizin) else {
        return;
    };
    for giris in girdiler.flatten() {
        let yol = giris.path();
        if yol.is_dir() {
            topla_yollar(&yol, cikti);
        } else {
            cikti.push(yol);
        }
    }
}

/// Bir referans dosyasının yolundan tam referans adını üretir (`refs/heads/main`).
///
/// `git_dir` dizinine ulaşıldığında durulur; `.git` bileşeni adın parçası **değildir**.
fn dosya_yolundan_referans_adi(git_dir: &Path, dosya: &Path) -> Option<String> {
    // Dosyanın kendi adı referans adının son bileşenidir.
    let mut ad: Vec<String> = vec![dosya.file_name()?.to_string_lossy().to_string()];
    let mut mevcut = dosya.parent();
    while let Some(dizin) = mevcut {
        if dizin == git_dir {
            ad.reverse();
            return Some(ad.join("/"));
        }
        ad.push(dizin.file_name()?.to_string_lossy().to_string());
        mevcut = dizin.parent();
    }
    None
}

/// Bir referans dosyasının içeriğini nesne adına çevirir.
fn referans_adini_coz(ad: &str, icerik: &str) -> Result<Oid, Hata> {
    if let Some(hedef) = icerik.strip_prefix("ref:") {
        return Err(Hata::HeadOkunamadi {
            ayrinti: format!("{ad} sembolik referansa işaret ediyor ({hedef})"),
        });
    }
    Oid::ayikla(icerik)
}
