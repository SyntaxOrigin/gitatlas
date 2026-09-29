//! GitAtlas komut satırı arayüzü: `ozet`, `gecmis` ve `diff` alt komutları.
//!
//! Bu dosya yalnızca argüman ayrıştırma ve çıktı yönlendirmesini yapar; tüm mantık
//! `gitatlas` kütüphanesindedir. Depo hiçbir komutta yazılmaz.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use gitatlas::ayarlar::{AramaSirasi, Ayarlar};
use gitatlas::cikti::{self, DosyaFarki, OzetGirdisi};
use gitatlas::commit::Commit;
use gitatlas::depo::Depo;
use gitatlas::fark::{agac_karsilastir, ikili_mi, satir_farki, YolFarki};
use gitatlas::gecmis::{self, GecmisSecenekleri};
use gitatlas::hata::Hata;
use gitatlas::magaza::Magaza;

/// git deposunu klonlamadan okuyan haritalayıcı.
#[derive(Parser, Debug)]
#[command(
    name = "gitatlas",
    version,
    about = "Git kurulu olmayan bir makinede .git dizinini salt okunur okuyan depo görüntüleyici",
    long_about = "GitAtlas, yalnızca `.git` dizinini okuyarak deponun özetini, geçmişini ve \
                  dosya farklarını üretir. Hedef makinede `git` kurulu olması gerekmez; \
                  depo hiçbir koşulda yazılmaz."
)]
struct Cli {
    #[command(subcommand)]
    komut: Komut,
}

#[derive(Subcommand, Debug)]
enum Komut {
    /// Deponun genel özetini üretir: HEAD, commit sayısı, yazar dağılımı, etiketler, pack'ler.
    Ozet {
        /// Depo yolu (çalışma ağacı kökü, `.git` dizini veya çıplak depo).
        depo: PathBuf,
        /// Tüm pack nesnelerinin CRC-32 ve SHA-1 doğrulamasını çalıştırır.
        #[arg(long)]
        butunluk: bool,
        /// Çıktı biçimi.
        #[arg(long, value_enum, default_value_t = Bicim::Json)]
        format: Bicim,
    },
    /// `git log` benzeri geçmiş listesi.
    Gecmis {
        /// Depo yolu.
        depo: PathBuf,
        /// Başlangıç revizyonu (HEAD, dal adı, etiket veya tam nesne adı).
        #[arg(long, default_value = "HEAD")]
        rev: String,
        /// Yalnızca bu dosya yoluna dokunan commit'leri listeler.
        #[arg(long = "yol")]
        yol: Option<String>,
        /// En fazla bu kadar kayıt listeler (0 = sınırsız).
        #[arg(long, default_value_t = 0)]
        limit: usize,
        /// Çıktı biçimi.
        #[arg(long, value_enum, default_value_t = Bicim::Json)]
        format: Bicim,
    },
    /// İki revizyon arasındaki ağaç ve satır farkı.
    Diff {
        /// Depo yolu.
        depo: PathBuf,
        /// Karşılaştırmanın sol (eski) tarafı.
        sol: String,
        /// Sağ (yeni) taraf. Verilmezse çalışma ağacı karşısına karşılaştırılır.
        sag: Option<String>,
        /// Yalnızca bu yolu karşılaştırır.
        #[arg(long = "yol")]
        yol: Option<String>,
        /// Unified diff bağlam satırı sayısı.
        #[arg(long, default_value_t = 3)]
        baglam: usize,
        /// Çıktı biçimi.
        #[arg(long, value_enum, default_value_t = Bicim::Duz)]
        format: Bicim,
        /// `sha1 → nesne` arama sırası.
        #[arg(long, default_value = "pack-once")]
        arama_sirasi: String,
        /// Tek nesne için izin verilen en büyük çözülmüş boyut (bayt).
        #[arg(long, default_value_t = 64 * 1024 * 1024)]
        nesne_tavani: u64,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
enum Bicim {
    /// JSON (makine okunur, boru hattına uygun).
    Json,
    /// Markdown (insan okunur).
    Md,
    /// Yalnızca unified diff gövdesi ( standart `patch` girdisi).
    Duz,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match calistir(cli) {
        Ok(metin) => {
            print!("{metin}");
            ExitCode::SUCCESS
        }
        Err(hata) => {
            eprintln!("gitatlas: {hata}");
            ExitCode::FAILURE
        }
    }
}

fn calistir(cli: Cli) -> Result<String, Hata> {
    match cli.komut {
        Komut::Ozet {
            depo,
            butunluk,
            format,
        } => ozet_calistir(&depo, butunluk, format),
        Komut::Gecmis {
            depo,
            rev,
            yol,
            limit,
            format,
        } => gecmis_calistir(&depo, &rev, yol.as_deref(), limit, format),
        Komut::Diff {
            depo,
            sol,
            sag,
            yol,
            baglam,
            format,
            arama_sirasi,
            nesne_tavani,
        } => {
            let ayar = Ayarlar {
                baglam,
                arama_sirasi: AramaSirasi::ayikla(&arama_sirasi)
                    .map_err(|ayrinti| Hata::CiktiHatasi { ayrinti })?,
                nesne_tavani,
                ..Ayarlar::default()
            };
            diff_calistir(&depo, &sol, sag.as_deref(), yol.as_deref(), ayar, format)
        }
    }
}

fn ac(depo_yolu: &Path, ayar: &Ayarlar) -> Result<(Depo, Magaza), Hata> {
    let depo = Depo::ac(depo_yolu)?;
    let magaza = Magaza::ac(depo.git_dir(), *ayar)?;
    Ok((depo, magaza))
}

fn ozet_calistir(depo_yolu: &Path, butunluk: bool, format: Bicim) -> Result<String, Hata> {
    let ayar = Ayarlar::default();
    let (depo, magaza) = ac(depo_yolu, &ayar)?;

    let mut kayitlar = Vec::new();
    let mut commit_sayisi = 0usize;
    if let Ok(baslangic) = depo.head_oid(&magaza) {
        let secenekler = GecmisSecenekleri::default();
        kayitlar = gecmis::gecmis(&magaza, &baslangic, &secenekler)?;
        commit_sayisi = kayitlar.len();
    }

    let mut yazar_sayaci: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut en_eski = i64::MAX;
    let mut en_yeni = i64::MIN;
    for k in &kayitlar {
        *yazar_sayaci.entry(k.yazar.clone()).or_insert(0) += 1;
        en_eski = en_eski.min(k.zaman);
        en_yeni = en_yeni.max(k.zaman);
    }
    let mut yazarlar: Vec<(String, usize)> = yazar_sayaci.into_iter().collect();
    yazarlar.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut etiketler = Vec::new();
    for referans in depo.etiketler() {
        etiketler.push(cikti::etiket_ucilesi(
            &magaza,
            referans.kisa_ad(),
            &referans.oid,
        ));
    }

    let donem = if kayitlar.is_empty() {
        None
    } else {
        Some((en_eski, en_yeni))
    };

    let girdi = OzetGirdisi {
        depo_yolu: depo.kok().display().to_string(),
        git_dizini: depo.git_dir().display().to_string(),
        head: cikti::head_aciklamasi(&depo, &magaza),
        commit_sayisi,
        yazarlar,
        etiketler,
        donem,
    };

    let rapor = if butunluk {
        Some(magaza.paketleri_dogrula()?)
    } else {
        None
    };

    match format {
        Bicim::Json => {
            let deger = cikti::ozet_json(&magaza, &ayar, &girdi, rapor.as_ref());
            cikti::json_metni(&deger)
        }
        Bicim::Md => Ok(cikti::ozet_markdown(&magaza, &girdi, rapor.as_ref())),
        Bicim::Duz => Err(Hata::CiktiHatasi {
            ayrinti: "`ozet` için --format düz kullanılamaz".to_string(),
        }),
    }
}

fn gecmis_calistir(
    depo_yolu: &Path,
    rev: &str,
    yol: Option<&str>,
    limit: usize,
    format: Bicim,
) -> Result<String, Hata> {
    let ayar = Ayarlar::default();
    let (depo, magaza) = ac(depo_yolu, &ayar)?;
    let baslangic = depo.coz(&magaza, rev)?;
    if let Some(y) = yol {
        gitatlas::agac::yolu_ayikla(y)?;
    }
    let secenekler = GecmisSecenekleri {
        yol: yol.map(str::to_string),
        limit: if limit == 0 { None } else { Some(limit) },
    };
    let kayitlar = gecmis::gecmis(&magaza, &baslangic, &secenekler)?;

    Ok(match format {
        Bicim::Json => {
            cikti::json_metni(&cikti::gecmis_json(&baslangic.onaltilik(), &kayitlar, yol))?
        }
        Bicim::Md => cikti::gecmis_markdown(&baslangic.onaltilik(), &kayitlar, yol),
        Bicim::Duz => {
            let mut c = String::new();
            for k in &kayitlar {
                c.push_str(&format!(
                    "{} {} {}\t{}\n",
                    cikti::utc_iso(k.zaman),
                    &k.oid[..7],
                    k.yazar,
                    k.konu
                ));
            }
            c
        }
    })
}

fn diff_calistir(
    depo_yolu: &Path,
    sol: &str,
    sag: Option<&str>,
    yol_filtresi: Option<&str>,
    ayar: Ayarlar,
    format: Bicim,
) -> Result<String, Hata> {
    let (depo, magaza) = ac(depo_yolu, &ayar)?;
    let sol_oid = depo.coz(&magaza, sol)?;
    let sag_oid = match sag {
        Some(d) => depo.coz(&magaza, d)?,
        None => depo.head_oid(&magaza)?,
    };

    let sol_commit = Commit::ayikla(&magaza, &sol_oid)?;
    let sag_commit = Commit::ayikla(&magaza, &sag_oid)?;
    let farklar = agac_karsilastir(&magaza, Some(&sol_commit.agac), &sag_commit.agac)?;

    let dosyalar: Vec<DosyaFarki> = farklar
        .iter()
        .filter(|f| yol_filtresi.map(|y| f.yol == y).unwrap_or(true))
        .map(|f| dosya_farki_hazirla(&magaza, f, ayar.baglam))
        .collect::<Result<Vec<_>, Hata>>()?;

    Ok(match format {
        Bicim::Json => cikti::json_metni(&cikti::diff_json(
            &sol_oid.onaltilik(),
            &sag_oid.onaltilik(),
            &dosyalar,
            yol_filtresi,
        ))?,
        Bicim::Md => cikti::diff_markdown(&sol_oid.onaltilik(), &sag_oid.onaltilik(), &dosyalar),
        Bicim::Duz => cikti::diff_duz(&sol_oid.onaltilik(), &sag_oid.onaltilik(), &dosyalar),
    })
}

fn dosya_farki_hazirla(
    magaza: &Magaza,
    yol_farki: &YolFarki,
    baglam: usize,
) -> Result<DosyaFarki, Hata> {
    if yol_farki.modul {
        return Ok(DosyaFarki {
            yol: yol_farki.yol.clone(),
            yol_farki: yol_farki.clone(),
            satir: None,
            ikili_notu: Some("alt modül (gitlink) içeriği okunamaz".to_string()),
        });
    }

    let (eski, yeni) = match (yol_farki.eski, yol_farki.yeni) {
        (Some(a), Some(b)) => {
            let eski = magaza.nesne(&a)?;
            let yeni = magaza.nesne(&b)?;
            (Some(eski), Some(yeni))
        }
        (Some(a), None) => (Some(magaza.nesne(&a)?), None),
        (None, Some(b)) => (None, Some(magaza.nesne(&b)?)),
        (None, None) => (None, None),
    };

    let ikili = eski.as_ref().map(|n| ikili_mi(&n.veri)).unwrap_or(false)
        || yeni.as_ref().map(|n| ikili_mi(&n.veri)).unwrap_or(false);

    if ikili {
        return Ok(DosyaFarki {
            yol: yol_farki.yol.clone(),
            yol_farki: yol_farki.clone(),
            satir: None,
            ikili_notu: Some("içerik ikili (NUL bayt içeriyor)".to_string()),
        });
    }

    let satir = match (&eski, &yeni) {
        (Some(a), Some(b)) => Some(satir_farki(&a.veri, &b.veri, baglam)),
        _ => None,
    };

    Ok(DosyaFarki {
        yol: yol_farki.yol.clone(),
        yol_farki: yol_farki.clone(),
        satir,
        ikili_notu: None,
    })
}
