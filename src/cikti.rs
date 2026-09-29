//! Çıktı üretimi: JSON ve Markdown.
//!
//! Üç çıktı vardır — `ozet`, `gecmis`, `diff` — ve her ikisi de boru hattına uygundur.
//! JSON şeması sözleşmedir; alan adları değişirse `tests/sema.rs` kırılır.
//!
//! **Gizlilik kuralı:** hiçbir çıktı commit yazarının e-posta adresini içermez. Yalnızca
//! kayıttaki ad (`Kimlik::gorunur_ad`) kullanılır.

use serde_json::{json, Value};

use crate::ayarlar::Ayarlar;
use crate::commit::{Commit, Etiket};
use crate::depo::{Depo, HeadDurumu, ReferansKaynagi};
use crate::fark::{Hunk, SatirFarki, SatirIslem, YolFarki};
use crate::gecmis::GecmisKaydi;
use crate::hata::Hata;
use crate::magaza::Magaza;
use crate::oid::Oid;

/// `ozet` alt komutunun topladığı veriler.
pub struct OzetGirdisi {
    /// Depo kökünün görüntülenen yolu.
    pub depo_yolu: String,
    /// `.git` dizininin görüntülenen yolu.
    pub git_dizini: String,
    /// HEAD durumunun insan okunur anlatımı.
    pub head: String,
    /// Toplam commit sayısı.
    pub commit_sayisi: usize,
    /// Yazar dağılımı (ad → commit sayısı, çoktan çoğa).
    pub yazarlar: Vec<(String, usize)>,
    /// Etiket listesi: (ad, oid, hedef türü).
    pub etiketler: Vec<(String, String, Option<String>)>,
    /// Aktif dönem (ilk ve son commit zamanı).
    pub donem: Option<(i64, i64)>,
}

/// `diff` alt komutunun çıktısı için hazırlanmış tek dosya farkı.
pub struct DosyaFarki {
    /// Depo içi yol.
    pub yol: String,
    /// Ağaç karşılaştırmasından gelen durum.
    pub yol_farki: YolFarki,
    /// Satır farkı (ikili veya tek taraflıysa `None`).
    pub satir: Option<SatirFarki>,
    /// İkili dosya olarak işaretlendiyse nedeni.
    pub ikili_notu: Option<String>,
}

/// `ozet` çıktısını JSON olarak üretir.
pub fn ozet_json(
    magaza: &Magaza,
    ayar: &Ayarlar,
    girdi: &OzetGirdisi,
    butunluk: Option<&crate::magaza::DogurmaRaporu>,
) -> Value {
    let sayaclar = magaza.sayaclar();
    json!({
        "arac": "gitatlas",
        "surum": env!("CARGO_PKG_VERSION"),
        "komut": "ozet",
        "depo": {
            "yol": girdi.depo_yolu,
            "git_dizini": girdi.git_dizini,
            "salt_okunur": true,
        },
        "head": {
            "durum": girdi.head,
        },
        "istatistik": {
            "commit_sayisi": girdi.commit_sayisi,
            "yazar_sayisi": girdi.yazarlar.len(),
            "paket_sayisi": magaza.paket_sayisi(),
            "paket_nesne_sayisi": magaza.paket_nesne_sayisi(),
        },
        "donem": {
            "ilk_unix": girdi.donem.map(|d| d.0),
            "son_unix": girdi.donem.map(|d| d.1),
        },
        "yazarlar": girdi
            .yazarlar
            .iter()
            .map(|(ad, sayi)| json!({"ad": ad, "commit": sayi}))
            .collect::<Vec<Value>>(),
        "etiketler": girdi
            .etiketler
            .iter()
            .map(|(ad, oid, tur)| json!({"ad": ad, "oid": oid, "tur": tur}))
            .collect::<Vec<Value>>(),
        "paketler": magaza.paket_ozetleri(),
        "ayarlar": ayar.json(),
        "sayaclar": {
            "onbellek_ten": sayaclar.onbellek_isi,
            "gevsek_ten": sayaclar.gevsek_isi,
            "pack_ten": sayaclar.paket_isi,
            "sha1_dogrulanan": sayaclar.dogrulanan,
        },
        "butunluk": butunluk.map(|r| json!({
            "paket_sayisi": r.paket_sayisi,
            "dogrulanan_nesne": r.dogrulanan_nesne,
            "durum": "gecerli",
        })),
    })
}

/// `gecmis` çıktısını JSON olarak üretir.
pub fn gecmis_json(girdi: &str, kayitlar: &[GecmisKaydi], yol: Option<&str>) -> Value {
    json!({
        "arac": "gitatlas",
        "surum": env!("CARGO_PKG_VERSION"),
        "komut": "gecmis",
        "baslangic": girdi,
        "yol_filtresi": yol,
        "adet": kayitlar.len(),        "kayitlar": kayitlar
            .iter()
            .map(|k| json!({
                "kisa": k.kisa,
                "oid": k.oid,
                "yazar": k.yazar,
                "zaman": k.zaman,
                "bolge": k.bolge,
                "konu": k.konu,
                "dosya_sayisi": k.dosya_sayisi,
                "ebeveyn_sayisi": k.ebeveyn_sayisi,
            }))
            .collect::<Vec<Value>>(),
    })
}

/// `diff` çıktısını JSON olarak üretir.
pub fn diff_json(sol: &str, sag: &str, dosyalar: &[DosyaFarki], yol: Option<&str>) -> Value {
    json!({
        "arac": "gitatlas",
        "surum": env!("CARGO_PKG_VERSION"),
        "komut": "diff",
        "sol": sol,
        "sag": sag,
        "yol_filtresi": yol,
        "dosya_sayisi": dosyalar.len(),
        "dosyalar": dosyalar.iter().map(dosya_json).collect::<Vec<Value>>(),
    })
}

fn dosya_json(d: &DosyaFarki) -> Value {
    let mut nesne = json!({
        "yol": d.yol,
        "durum": d.yol_farki.tur.ad(),
        "isaret": d.yol_farki.tur.isaret().to_string(),
        "modul": d.yol_farki.modul,
        "eski_oid": d.yol_farki.eski.map(|o| o.onaltilik()),
        "yeni_oid": d.yol_farki.yeni.map(|o| o.onaltilik()),
        "eski_mod": d.yol_farki.eski_mod,
        "yeni_mod": d.yol_farki.yeni_mod,
    });
    if let Some(not) = &d.ikili_notu {
        nesne["ikili"] = json!(not);
    }
    if let Some(satir) = &d.satir {
        nesne["satir"] = json!({
            "eklenen": satir.eklenen,
            "silinen": satir.silinen,
            "hunk_sayisi": satir.hunks.len(),
            "kaba": satir.kaba,
            "hunklar": satir.hunks.iter().map(hunk_json).collect::<Vec<Value>>(),
        });
    }
    nesne
}

fn hunk_json(h: &Hunk) -> Value {
    json!({
        "eski_baslangic": h.eski_baslangic,
        "yeni_baslangic": h.yeni_baslangic,
        "eklenen": h.eklenen(),
        "silinen": h.silinen(),
        "satirlar": h
            .satirlar
            .iter()
            .map(|(islem, metin)| json!({"islem": islem_adi(islem), "metin": metin}))
            .collect::<Vec<Value>>(),
    })
}

fn islem_adi(islem: &SatirIslem) -> &'static str {
    match islem {
        SatirIslem::Esit => "esit",
        SatirIslem::Silinen => "silinen",
        SatirIslem::Eklenen => "eklenen",
    }
}

/// `ozet` çıktısını Markdown olarak üretir.
pub fn ozet_markdown(
    magaza: &Magaza,
    girdi: &OzetGirdisi,
    butunluk: Option<&crate::magaza::DogurmaRaporu>,
) -> String {
    let mut c = String::new();
    c.push_str("# GitAtlas — Depo Özeti\n\n");
    c.push_str(&format!("- **Depo**: `{}`\n", girdi.depo_yolu));
    c.push_str(&format!("- **HEAD**: {}\n", girdi.head));
    c.push_str(&format!("- **Commit sayısı**: {}\n", girdi.commit_sayisi));
    c.push_str(&format!("- **Yazar sayısı**: {}\n", girdi.yazarlar.len()));
    c.push_str(&format!(
        "- **Pack dosyası**: {} ({} nesne)\n",
        magaza.paket_sayisi(),
        magaza.paket_nesne_sayisi()
    ));
    c.push_str("- **Erişim**: salt okunur (depo hiçbir yerde yazılmadı)\n\n");

    c.push_str("## Yazarlar\n\n");
    c.push_str("| Yazar | Commit |\n|---|---:|\n");
    for (ad, sayi) in &girdi.yazarlar {
        c.push_str(&format!("| {ad} | {sayi} |\n"));
    }
    c.push('\n');

    if !girdi.etiketler.is_empty() {
        c.push_str("## Etiketler\n\n| Etiket | Nesne | Tür |\n|---|---|---|\n");
        for (ad, oid, tur) in &girdi.etiketler {
            c.push_str(&format!(
                "| {ad} | `{}` | {} |\n",
                &oid[..7.min(oid.len())],
                tur.as_deref().unwrap_or("—")
            ));
        }
        c.push('\n');
    }

    if let Some(r) = butunluk {
        c.push_str("## Bütünlük\n\n");
        c.push_str(&format!(
            "{} pack dosyasındaki {} nesnenin CRC-32 ve SHA-1 doğrulaması geçti.\n",
            r.paket_sayisi, r.dogrulanan_nesne
        ));
    }
    c
}

/// `gecmis` çıktısını Markdown olarak üretir.
pub fn gecmis_markdown(girdi: &str, kayitlar: &[GecmisKaydi], yol: Option<&str>) -> String {
    let mut c = String::new();
    c.push_str("# GitAtlas — Geçmiş\n\n");
    c.push_str(&format!("Başlangıç: `{girdi}`"));
    if let Some(y) = yol {
        c.push_str(&format!(" · yol filtresi: `{y}`"));
    }
    c.push_str(&format!(" · {} kayıt\n\n", kayitlar.len()));
    c.push_str("| Commit | Yazar | Zaman (UTC) | Konu | Dosya |\n|---|---|---|---|---:|\n");
    for k in kayitlar {
        c.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            k.kisa,
            k.yazar,
            utc_iso(k.zaman),
            k.konu.replace('|', "\\|"),
            k.dosya_sayisi
        ));
    }
    c
}

/// `diff` çıktısını unified diff metni olarak üretir.
pub fn diff_markdown(sol: &str, sag: &str, dosyalar: &[DosyaFarki]) -> String {
    let mut c = String::new();
    c.push_str(&format!(
        "# GitAtlas — Diff\n\n`sol` = `{sol}` · `sag` = `{sag}`\n\n"
    ));
    if dosyalar.is_empty() {
        c.push_str("Fark yok.\n");
        return c;
    }
    for d in dosyalar {
        c.push_str(&format!("## `{}` ({})\n\n", d.yol, d.yol_farki.tur.ad()));
        if let Some(not) = &d.ikili_notu {
            c.push_str(&format!("**İkili dosya** — {not}\n\n"));
            continue;
        }
        match &d.satir {
            None => c.push_str("İçerik farkı hesaplanmadı (tek taraflı değişiklik).\n\n"),
            Some(satir) if satir.hunks.is_empty() => c.push_str("Satır farkı yok.\n\n"),
            Some(satir) => {
                if satir.kaba {
                    c.push_str(
                        "> Myers tavanı aşıldı: fark kaba blok olarak gösteriliyor \
                         (minimal değil, ancak doğrudur).\n\n",
                    );
                }
                c.push_str("```diff\n");
                for hunk in &satir.hunks {
                    c.push_str(&format!(
                        "@@ -{},{} +{},{} @@\n",
                        hunk.eski_baslangic,
                        hunk.eski_satir(),
                        hunk.yeni_baslangic,
                        hunk.yeni_satir()
                    ));
                    for (islem, metin) in &hunk.satirlar {
                        c.push(islem.onek());
                        c.push_str(metin);
                        c.push('\n');
                    }
                }
                c.push_str("```\n\n");
            }
        }
    }
    c
}

/// `diff` çıktısını yalnızca unified diff gövdesi olarak üretir (boru hattı için).
pub fn diff_duz(sol: &str, sag: &str, dosyalar: &[DosyaFarki]) -> String {
    let mut c = String::new();
    c.push_str(&format!("--- a/{sol}\n+++ b/{sag}\n"));
    for d in dosyalar {
        let Some(satir) = &d.satir else { continue };
        c.push_str(&format!("diff --git a/{0} b/{0}\n", d.yol));
        for hunk in &satir.hunks {
            c.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.eski_baslangic,
                hunk.eski_satir(),
                hunk.yeni_baslangic,
                hunk.yeni_satir()
            ));
            for (islem, metin) in &hunk.satirlar {
                c.push(islem.onek());
                c.push_str(metin);
                c.push('\n');
            }
        }
    }
    c
}

/// JSON çıktısını güzel biçimlendirilmiş metne çevirir.
pub fn json_metni(deger: &Value) -> Result<String, Hata> {
    serde_json::to_string_pretty(deger).map_err(|hata| Hata::CiktiHatasi {
        ayrinti: hata.to_string(),
    })
}

/// HEAD durumunu okunur tek cümleye çevirir.
pub fn head_aciklamasi(depo: &Depo, magaza: &Magaza) -> String {
    match depo.head() {
        HeadDurumu::Dal(ad) => match depo.head_oid(magaza) {
            Ok(oid) => format!("dal `{}` → `{}`", kisa_dal_adi(ad), &oid.onaltilik()[..7]),
            Err(_) => format!("dal `{}` (henüz doğmamış)", kisa_dal_adi(ad)),
        },
        HeadDurumu::Ayrik(oid) => {
            format!("ayrışık (detached) `{}`", &oid.onaltilik()[..7])
        }
        HeadDurumu::Dogmadi(ad) => format!("dal `{}` (henüz doğmamış)", kisa_dal_adi(ad)),
    }
}

fn kisa_dal_adi(ad: &str) -> &str {
    ad.strip_prefix("refs/heads/").unwrap_or(ad)
}

/// Referans kaynağını okunur metne çevirir.
pub fn kaynak_aciklamasi(kaynak: ReferansKaynagi) -> &'static str {
    kaynak.ad()
}

/// Unix zaman damgasını `YYYY-MM-DDTHH:MM:SSZ` biçimine çevirir (el hesaplama).
///
/// `chrono`/`time` bağımlılıkları yasaktır; bu, raporun 30 günlük dilim ihtiyacı için
/// yeterli olan küçük bir Gregory takvimi dönüşümüdür.
pub fn utc_iso(zaman: i64) -> String {
    let gun_sayisi = zaman.div_euclid(86_400);
    let kalan = zaman.rem_euclid(86_400);
    let (yil, ay, gun) = takvim_gunu(gun_sayisi);
    format!(
        "{yil:04}-{ay:02}-{gun:02}T{:02}:{:02}:{:02}Z",
        kalan / 3600,
        (kalan % 3600) / 60,
        kalan % 60
    )
}

/// 1970-01-01'den bu yana geçen gün sayısını (yıl, ay, gün) olarak verir.
///
/// Howard Hinnant'ın `civil_from_days` algoritmasının gün çözümü kısmıdır; artık yıl
/// kuralı proleptik Gregory'dir.
fn takvim_gunu(gun: i64) -> (i64, u32, u32) {
    let z = gun + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let gun = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let ay = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let yil = if ay <= 2 { y + 1 } else { y };
    (yil, ay, gun)
}

/// Etiket kaydını okunur üçlüye çevirir: (ad, oid, hedef türü).
pub fn etiket_ucilesi(magaza: &Magaza, ad: &str, oid: &Oid) -> (String, String, Option<String>) {
    match Etiket::ayikla(magaza, oid) {
        Ok(etiket) => (ad.to_string(), oid.onaltilik(), Some(etiket.hedef_turu)),
        Err(_) => (ad.to_string(), oid.onaltilik(), None),
    }
}

/// Commit kaydının kısa adını döndürür.
pub fn kisa_ad(commit: &Commit) -> String {
    commit.oid.onaltilik()[..7].to_string()
}
