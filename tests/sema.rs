//! Çıktı şeması testleri: `ozet`, `gecmis` ve `diff` JSON/Markdown çıktılarının
//! alan adları ve gizlilik kuralı (e-posta sızmaz) sabitlenir.
//!
//! Şema bir sözleşmedir: burada doğrulanan alan adları değişirse bu test kırılır.

mod yardimci;

use std::collections::BTreeMap;

use gitatlas::agac::agac_girdileri;
use gitatlas::ayarlar::Ayarlar;
use gitatlas::cikti::{
    diff_json, diff_markdown, gecmis_json, gecmis_markdown, json_metni, ozet_json, ozet_markdown,
    utc_iso, DosyaFarki, OzetGirdisi,
};
use gitatlas::commit::Commit;
use gitatlas::depo::Depo;
use gitatlas::fark::{agac_karsilastir, satir_farki, YolFarki};
use gitatlas::gecmis::{self, GecmisSecenekleri};
use gitatlas::magaza::Magaza;
use gitatlas::oid::Oid;
use serde_json::Value;
use yardimci::GeciciDizin;

/// Ucuz, tamamen belirlenimci bir depo kurar (git çalıştırmaz).
fn fikstur(etiket: &str) -> (GeciciDizin, Oid, Oid) {
    // Her test kendi dizinini kullanır: testler paralel çalışır, ortak dizin paylaşılamaz.
    let gecici = GeciciDizin::yeni(etiket).expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let nesne = git.join("objects");

    let ilk_blob = yardimci::gevsek_yaz(&nesne, "blob", b"bir\n");
    let ikinci_blob = yardimci::gevsek_yaz(&nesne, "blob", b"iki\n");
    let ilk_agac = yardimci::agac_yaz(&nesne, &[("100644", "a.txt", ilk_blob)]);
    let ikinci_agac = yardimci::agac_yaz(
        &nesne,
        &[
            ("100644", "a.txt", ikinci_blob),
            ("100644", "b.txt", ilk_blob),
        ],
    );
    let ilk = yardimci::commit_yaz(&nesne, ilk_agac, &[], "Ada Lovelace", 1_700_000_000, "ilk");
    let ikinci = yardimci::commit_yaz(
        &nesne,
        ikinci_agac,
        &[ilk],
        "Alan Turing",
        1_700_000_500,
        "ikinci",
    );
    std::fs::write(
        git.join("refs/heads/main"),
        format!("{}\n", ikinci.onaltilik()),
    )
    .expect("main");
    (gecici, ikinci, ilk)
}

#[test]
fn ozet_json_sema_dogru() {
    let (gecici, ikinci, _) = fikstur("sema-ozet");
    let depo = Depo::ac(gecici.kok().as_path()).expect("depo");
    let magaza = Magaza::ac(depo.git_dir(), Ayarlar::default()).expect("mağaza");
    let kayitlar = gecmis::gecmis(&magaza, &ikinci, &GecmisSecenekleri::default()).expect("geçmiş");

    let girdi = OzetGirdisi {
        depo_yolu: depo.kok().display().to_string(),
        git_dizini: depo.git_dir().display().to_string(),
        head: "dal `main`".to_string(),
        commit_sayisi: kayitlar.len(),
        yazarlar: vec![
            ("Ada Lovelace".to_string(), 1),
            ("Alan Turing".to_string(), 1),
        ],
        etiketler: vec![],
        donem: Some((1_700_000_000, 1_700_000_500)),
    };
    let deger = ozet_json(&magaza, &Ayarlar::default(), &girdi, None);
    let metin = json_metni(&deger).expect("json");

    for alan in [
        "arac",
        "surum",
        "komut",
        "depo",
        "head",
        "istatistik",
        "donem",
        "yazarlar",
        "etiketler",
        "paketler",
        "ayarlar",
        "sayaclar",
    ] {
        assert!(metin.contains(alan), "alan eksik: {alan}");
    }
    assert_eq!(deger["komut"], "ozet");
    assert_eq!(deger["depo"]["salt_okunur"], true);
    assert_eq!(deger["istatistik"]["commit_sayisi"], 2);
    assert!(!metin.contains('@'), "e-posta adresi çıktıya sızmamalı");
    assert!(deger["ayarlar"]["arama_sirasi"].is_string());
}

#[test]
fn gecmis_json_sema_dogru() {
    let (gecici, ikinci, _) = fikstur("sema-gecmis");
    let depo = Depo::ac(gecici.kok().as_path()).expect("depo");
    let magaza = Magaza::ac(depo.git_dir(), Ayarlar::default()).expect("mağaza");
    let kayitlar = gecmis::gecmis(&magaza, &ikinci, &GecmisSecenekleri::default()).expect("geçmiş");
    let deger = gecmis_json("HEAD", &kayitlar, Some("a.txt"));
    let metin = json_metni(&deger).expect("json");

    assert_eq!(deger["komut"], "gecmis");
    assert_eq!(deger["adet"], 2);
    assert_eq!(deger["yol_filtresi"], "a.txt");
    let ilk = &deger["kayitlar"][0];
    for alan in [
        "kisa",
        "oid",
        "yazar",
        "zaman",
        "bolge",
        "konu",
        "dosya_sayisi",
        "ebeveyn_sayisi",
    ] {
        assert!(ilk.get(alan).is_some(), "kayıt alanı eksik: {alan}");
    }
    assert!(!metin.contains('@'), "e-posta adresi çıktıya sızmamalı");

    let md = gecmis_markdown("HEAD", &kayitlar, Some("a.txt"));
    assert!(md.contains("| Commit | Yazar |"));
    assert!(!md.contains('@'));
}

#[test]
fn diff_json_sema_dogru() {
    let (gecici, ikinci, ilk) = fikstur("sema-diff");
    let depo = Depo::ac(gecici.kok().as_path()).expect("depo");
    let magaza = Magaza::ac(depo.git_dir(), Ayarlar::default()).expect("mağaza");
    let a = Commit::ayikla(&magaza, &ilk).expect("a");
    let b = Commit::ayikla(&magaza, &ikinci).expect("b");
    let farklar = agac_karsilastir(&magaza, Some(&a.agac), &b.agac).expect("fark");

    let dosyalar: Vec<DosyaFarki> = farklar
        .iter()
        .map(|f| DosyaFarki {
            yol: f.yol.clone(),
            yol_farki: f.clone(),
            satir: match (f.eski, f.yeni) {
                (Some(x), Some(y)) => {
                    let eski = magaza.nesne(&x).expect("eski");
                    let yeni = magaza.nesne(&y).expect("yeni");
                    Some(satir_farki(&eski.veri, &yeni.veri, 3))
                }
                _ => None,
            },
            ikili_notu: None,
        })
        .collect();

    let deger = diff_json("sol", "sag", &dosyalar, None);
    let metin = json_metni(&deger).expect("json");
    assert_eq!(deger["komut"], "diff");
    assert_eq!(deger["dosya_sayisi"], 2);
    let d0 = &deger["dosyalar"][0];
    for alan in ["yol", "durum", "isaret", "modul", "eski_oid", "yeni_oid"] {
        assert!(d0.get(alan).is_some(), "dosya alanı eksik: {alan}");
    }
    assert!(metin.contains("\"hunklar\"") || metin.contains("\"hunk_sayisi\""));
    assert!(!metin.contains('@'));

    let md = diff_markdown("sol", "sag", &dosyalar);
    assert!(md.contains("```diff"));
    assert!(md.contains("@@"));
}

#[test]
fn ikili_dosya_json_alanini_ekler() {
    let dosya = DosyaFarki {
        yol: "resim.bin".to_string(),
        yol_farki: YolFarki {
            yol: "resim.bin".to_string(),
            tur: gitatlas::fark::DegisimTuru::Degisti,
            eski: None,
            yeni: None,
            eski_mod: None,
            yeni_mod: None,
            modul: false,
        },
        satir: None,
        ikili_notu: Some("içerik ikili (NUL bayt içeriyor)".to_string()),
    };
    let deger = diff_json("a", "b", &[dosya], None);
    assert!(deger["dosyalar"][0]["ikili"].is_string());
    let md = diff_markdown("a", "b", &[]);
    assert!(md.contains("Fark yok"));
}

#[test]
fn json_ve_markdown_bos_depo_icin_gecerli() {
    let gecici = GeciciDizin::yeni("sema-bos").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let depo = Depo::ac(&git).expect("depo");
    let magaza = Magaza::ac(&git, Ayarlar::default()).expect("mağaza");
    let girdi = OzetGirdisi {
        depo_yolu: depo.kok().display().to_string(),
        git_dizini: depo.git_dir().display().to_string(),
        head: "dal `main` (henüz doğmamış)".to_string(),
        commit_sayisi: 0,
        yazarlar: vec![],
        etiketler: vec![],
        donem: None,
    };
    let deger = ozet_json(&magaza, &Ayarlar::default(), &girdi, None);
    assert_eq!(deger["istatistik"]["commit_sayisi"], 0);
    assert!(deger["donem"]["ilk_unix"].is_null());
    let md = ozet_markdown(&magaza, &girdi, None);
    assert!(md.contains("Depo Özeti"));
}

#[test]
fn utc_iso_donusturmesi_bilinen_degerler_verir() {
    assert_eq!(utc_iso(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc_iso(1_700_000_000), "2023-11-14T22:13:20Z");
    // Artık yıl kuralı: 2024-02-29 birinci gün 1970'den 19785 gün sonra gelir.
    assert_eq!(utc_iso(1_709_164_800), "2024-02-29T00:00:00Z");
    assert_eq!(utc_iso(-1), "1969-12-31T23:59:59Z");
}

#[test]
fn agac_girdileri_sayisi_dogru() {
    let (gecici, ikinci, _) = fikstur("sema-agac");
    let depo = Depo::ac(gecici.kok().as_path()).expect("depo");
    let magaza = Magaza::ac(depo.git_dir(), Ayarlar::default()).expect("mağaza");
    let commit = Commit::ayikla(&magaza, &ikinci).expect("commit");
    let girdiler = agac_girdileri(&magaza, &commit.agac).expect("ağaç");
    let adlar: BTreeMap<&str, &str> = girdiler
        .iter()
        .map(|g| (g.ad.as_str(), g.ad.as_str()))
        .collect();
    assert!(adlar.contains_key("a.txt"));
    assert!(adlar.contains_key("b.txt"));
}

#[test]
fn json_uyumlu_sema_tip_kontrolu() {
    // `serde_json::Value` olarak üretilen çıktı gerçekten JSON'a çevrilebilir.
    let deger: Value = serde_json::json!({"a": [1, 2, 3]});
    let metin = json_metni(&deger).expect("json");
    let geri: Value = serde_json::from_str(&metin).expect("geri ayrıştırma");
    assert_eq!(geri, deger);
}
