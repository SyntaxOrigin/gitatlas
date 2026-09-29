//! Test fikstürleri için ortak yardımcılar.
//!
//! `tempfile` crate'i bağımlılık politikası gereği yasaktır (WORKER_CONTRACT.md § 5.3),
//! bu yüzden geçici dizin yönetimi ve depo iskeleti kurulumu kendi kodumuzla yapılır.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Test içinde geçici `.git` dizini üreten, `Drop` ile temizleyen kapsayıcı.
///
/// Benzersizlik `std::process::id()` + etiketten gelir; rastgelelik crate'i yoktur.
/// Aynı etiketle ikinci bir dizin açılırsa eski içerik önce silinir.
pub struct GeciciDizin {
    yol: PathBuf,
}

impl GeciciDizin {
    /// `std::env::temp_dir()` altında, etiketten türetilmiş benzersiz bir dizin oluşturur.
    pub fn yeni(etiket: &str) -> std::io::Result<Self> {
        let kok = std::env::temp_dir().join(format!("gitatlas-{etiket}-{}", std::process::id()));
        // Aynı testin iki kez çalışması olası; içeriği temizle.
        let _ = fs::remove_dir_all(&kok);
        fs::create_dir_all(&kok)?;
        Ok(Self { yol: kok })
    }

    /// Dizin yolunu döndürür.
    pub fn yol(&self) -> &Path {
        &self.yol
    }

    /// Depo kökünü döndürür.
    pub fn kok(&self) -> PathBuf {
        self.yol.clone()
    }

    /// Deponun `.git` dizinini döndürür.
    pub fn git_dir(&self) -> PathBuf {
        self.yol.join(".git")
    }

    /// Depo iskeletini (`.git/objects`, `.git/refs`, `HEAD`) oluşturur.
    pub fn iskelet(&self) -> std::io::Result<PathBuf> {
        let git = self.git_dir();
        fs::create_dir_all(git.join("objects/pack"))?;
        fs::create_dir_all(git.join("refs/heads"))?;
        fs::create_dir_all(git.join("refs/tags"))?;
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n")?;
        Ok(git)
    }
}

impl Drop for GeciciDizin {
    fn drop(&mut self) {
        // Temizlik hatası testi düşürmemelidir; `Drop` içinden hata döndürülemez.
        let _ = fs::remove_dir_all(&self.yol);
    }
}

/// `git` çalıştırılabilir dosyası PATH üzerinde var mı?
pub fn git_var_mi() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|c| c.status.success())
        .unwrap_or(false)
}

/// `git` komutunu verilen dizinde çalıştırır; hata olursa panik yerine hata döndürür.
///
/// `git` yalnızca **test fikstürü üreticisidir**; GitAtlas'ın çalışma zamanı kodu
/// git'i hiç çağırmaz ve bağımlılık olarak görmez.
pub fn git_calistir(dizin: &Path, args: &[&str]) -> Result<String, String> {
    let cikti = Command::new("git")
        .args(args)
        .current_dir(dizin)
        .output()
        .map_err(|e| format!("git çalıştırılamadı: {e}"))?;
    if cikti.status.success() {
        Ok(String::from_utf8_lossy(&cikti.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&cikti.stderr).to_string())
    }
}

/// Kimliksiz (ama açık) git komutu çalıştırır.
///
/// Global git config'e dokunmaz; kimlik yalnızca komut satırında verilir.
pub fn git_kimlikli(dizin: &Path, args: &[&str]) -> Result<String, String> {
    let tam: Vec<&str> = vec![
        "-c",
        "user.name=SyntaxOrigin",
        "-c",
        "user.email=SyntaxOrigin@users.noreply.github.com",
    ];
    let mut hepsi: Vec<&str> = tam;
    hepsi.extend_from_slice(args);
    git_calistir(dizin, &hepsi)
}

/// Depo tüm dosyalarının (yol, boyut, içerik özeti) listesini döndürür.
///
/// Salt okunurluk kanıtının çekirdeği: işlemden önce ve sonra alınan iki liste
/// birebir aynı olmalıdır.
pub fn depo_ozeti(kok: &Path) -> Vec<(String, u64, String)> {
    let mut sonuc = Vec::new();
    topla(kok, kok, &mut sonuc);
    sonuc.sort();
    sonuc
}

fn topla(kok: &Path, dizin: &Path, cikti: &mut Vec<(String, u64, String)>) {
    let Ok(girdiler) = fs::read_dir(dizin) else {
        return;
    };
    for giris in girdiler.flatten() {
        let yol = giris.path();
        if yol.is_dir() {
            topla(kok, &yol, cikti);
        } else {
            let goreli = yol
                .strip_prefix(kok)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| yol.display().to_string());
            let veri = fs::read(&yol).unwrap_or_default();
            let ozet = gitatlas::sha1::sha1_onaltilik(&veri);
            cikti.push((goreli, veri.len() as u64, ozet));
        }
    }
}

/// Bir dosyaya `<tür> <uzunluk>\0<veri>` biçiminde, zlib sıkıştırılmış gevşek nesne yazar.
pub fn gevsek_yaz(objects_dir: &Path, tur: &str, veri: &[u8]) -> gitatlas::Oid {
    use std::io::Write as _;

    let baslik = format!("{tur} {}\0", veri.len());
    let mut ham = baslik.into_bytes();
    ham.extend_from_slice(veri);
    let oid = gitatlas::Oid::baytlardan(gitatlas::sha1::sha1(&ham));

    let mut kodlayici = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(1));
    kodlayici.write_all(&ham).expect("sıkıştırma yazmalı");
    let sikis = kodlayici.finish().expect("sıkıştırma bitmeli");

    let onaltilik = oid.onaltilik();
    let dizi = objects_dir.join(&onaltilik[..2]);
    fs::create_dir_all(&dizi).expect("nesne dizini oluşturulmalı");
    fs::write(dizi.join(&onaltilik[2..]), sikis).expect("nesne yazılmalı");
    oid
}

/// `<tür> <uzunluk>\0<girdiler>` biçiminde ağaç nesnesi yazar ve adını döndürür.
pub fn agac_yaz(objects_dir: &Path, girdiler: &[(&str, &str, gitatlas::Oid)]) -> gitatlas::Oid {
    let mut veri = Vec::new();
    for (mod_bilgi, ad, oid) in girdiler {
        veri.extend_from_slice(format!("{mod_bilgi} {ad}\0").as_bytes());
        veri.extend_from_slice(oid.baytlar());
    }
    gevsek_yaz(objects_dir, "tree", &veri)
}

/// Commit nesnesi yazar ve adını döndürür.
pub fn commit_yaz(
    objects_dir: &Path,
    agac: gitatlas::Oid,
    ebeveynler: &[gitatlas::Oid],
    yazar: &str,
    zaman: i64,
    konu: &str,
) -> gitatlas::Oid {
    let mut govde = format!("tree {}\n", agac.onaltilik());
    for ebeveyn in ebeveynler {
        govde.push_str(&format!("parent {}\n", ebeveyn.onaltilik()));
    }
    govde.push_str(&format!(
        "author {yazar} <ornek@example.com> {zaman} +0000\n"
    ));
    govde.push_str(&format!(
        "committer {yazar} <ornek@example.com> {zaman} +0000\n\n{konu}\n"
    ));
    gevsek_yaz(objects_dir, "commit", govde.as_bytes())
}
