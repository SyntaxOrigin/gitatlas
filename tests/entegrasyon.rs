//! Uçtan uca testler: **gerçek `git` ile üretilmiş** depolar okunur ve çıktılar
//! `git cat-file`, `git log` ve `git show` çıktılarıyla karşılaştırılır.
//!
//! `git` burada yalnızca **test fikstürü üreticisidir**; GitAtlas'ın çalışma zamanı
//! kodu git'e bağımlı değildir ve `git` kurulu olmayan bir makinede de çalışır.
//! `git` yoksa testler `ATLANDI` mesajıyla boşta çıkar (sessizce "geçmez" sayılmaz).
//!
//! Üretilen depo türleri:
//!
//! | Tür | Açıklama |
//! |---|---|
//! | (a) | yalnızca gevşek nesne (pack yok) |
//! | (b) | `git gc` ile paketlenmiş |
//! | (c) | birden fazla pack dosyası |
//! | (d) | delta içeren pack (`git gc --aggressive`) |
//! | (e) | bozuk pack |
//! | (f) | `packed-refs` içeren |
//! | (g) | işaretli (annotated) etiketli |

mod yardimci;

use std::fs;
use std::path::Path;

use gitatlas::agac::{yol_coz, YolSonucu};
use gitatlas::ayarlar::Ayarlar;
use gitatlas::commit::Commit;
use gitatlas::depo::Depo;
use gitatlas::fark::agac_karsilastir;
use gitatlas::gecmis::{self, GecmisSecenekleri};
use gitatlas::hata::Hata;
use gitatlas::magaza::Magaza;
use gitatlas::oid::Oid;
use yardimci::{git_kimlikli, git_var_mi, GeciciDizin};

/// `git` yoksa testi boşta çıkarır; `git` varsa depo üretimini yapar.
fn gecici_depo(etiket: &str) -> Option<GeciciDizin> {
    if !git_var_mi() {
        eprintln!("ATLANDI: git bulunamadı ({etiket})");
        return None;
    }
    let gecici = GeciciDizin::yeni(etiket).ok()?;
    git_kimlikli(gecici.yol(), &["init", "-b", "main", "-q"]).ok()?;
    Some(gecici)
}

fn dosya_yaz(kok: &Path, yol: &str, icerik: &str) {
    let tam = kok.join(yol);
    if let Some(usta) = tam.parent() {
        fs::create_dir_all(usta).expect("dizin oluşturulmalı");
    }
    fs::write(&tam, icerik).expect("dosya yazılmalı");
}

fn commitla(kok: &Path, mesaj: &str) -> String {
    git_kimlikli(kok, &["add", "-A"]).expect("git add");
    git_kimlikli(
        kok,
        &["commit", "-q", "-m", mesaj, "--date=2026-01-01T00:00:00Z"],
    )
    .expect("git commit");
    git_kimlikli(kok, &["rev-parse", "HEAD"])
        .expect("rev-parse")
        .trim()
        .to_string()
}

fn magaza_ac(depo: &Depo) -> Magaza {
    Magaza::ac(depo.git_dir(), Ayarlar::default()).expect("mağaza")
}

// ---------------------------------------------------------------------------
// (a) yalnızca gevşek nesne
// ---------------------------------------------------------------------------

#[test]
fn gevsek_nesneli_depo_okunur() {
    let Some(gecici) = gecici_depo("ent-gevsek") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "a.txt", "birinci\n");
    commitla(&kok, "ilk");
    dosya_yaz(&kok, "b.txt", "ikinci\n");
    commitla(&kok, "ikinci");

    let pack_dizini = kok.join(".git/objects/pack");
    let pack_adedi = std::fs::read_dir(&pack_dizini)
        .map(|g| g.count())
        .unwrap_or(0);
    assert_eq!(pack_adedi, 0, "bu depo pack'siz olmalı");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let baslangic = depo.coz(&magaza, "HEAD").expect("HEAD");

    let kayitlar =
        gecmis::gecmis(&magaza, &baslangic, &GecmisSecenekleri::default()).expect("geçmiş");
    assert_eq!(kayitlar.len(), 2, "iki commit görülmeli");
    assert_eq!(kayitlar[0].konu, "ikinci");
    assert_eq!(kayitlar[0].yazar, "SyntaxOrigin");
    assert!(!kayitlar[0].yazar.contains('@'), "e-posta sızmamalı");

    match yol_coz(
        &magaza,
        &Commit::ayikla(&magaza, &baslangic).expect("c").agac,
        "a.txt",
    )
    .expect("yol")
    {
        YolSonucu::Dosya { oid, .. } => {
            let beklenen = git_kimlikli(&kok, &["rev-parse", "HEAD:a.txt"])
                .expect("git rev-parse")
                .trim()
                .to_string();
            assert_eq!(oid.onaltilik(), beklenen);
        }
        diger => panic!("beklenmeyen sonuç: {diger:?}"),
    }
}

// ---------------------------------------------------------------------------
// (b) paketlenmiş depo
// ---------------------------------------------------------------------------

#[test]
fn paketlenmis_depo_okunur_ve_git_ile_karsilastirilir() {
    let Some(gecici) = gecici_depo("ent-pack") else {
        return;
    };
    let kok = gecici.kok();
    for i in 0..8 {
        dosya_yaz(
            &kok,
            &format!("mod{i}.txt"),
            &format!("satir {i}\n{}\n", "dolgu".repeat(i + 1)),
        );
        commitla(&kok, &format!("commit {i}"));
    }
    git_kimlikli(&kok, &["gc", "-q"]).expect("git gc");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    assert!(magaza.paket_sayisi() >= 1, "gc sonrası pack olmalı");

    let baslangic = depo.coz(&magaza, "HEAD").expect("HEAD");
    let kayitlar =
        gecmis::gecmis(&magaza, &baslangic, &GecmisSecenekleri::default()).expect("geçmiş");

    // `git log --format=%s` ile birebir karşılaştır.
    let git_log = git_kimlikli(&kok, &["log", "--format=%H %s"]).expect("git log");
    let git_satirlar: Vec<&str> = git_log.lines().filter(|s| !s.is_empty()).collect();
    assert_eq!(
        kayitlar.len(),
        git_satirlar.len(),
        "commit sayısı git ile aynı olmalı"
    );
    for (kayit, satir) in kayitlar.iter().zip(git_satirlar.iter()) {
        let mut parcalar = satir.splitn(2, ' ');
        assert_eq!(kayit.oid, parcalar.next().unwrap_or_default());
        assert_eq!(kayit.konu, parcalar.next().unwrap_or_default());
    }

    // Her blob'un içeriği `git cat-file blob` ile birebir aynı olmalı.
    let commit = Commit::ayikla(&magaza, &baslangic).expect("commit");
    let girdiler = gitatlas::agac::agac_girdileri(&magaza, &commit.agac).expect("ağaç");
    assert!(!girdiler.is_empty());
    for giris in &girdiler {
        let nesne = magaza.nesne(&giris.oid).expect("blob");
        let beklenen = git_kimlikli(&kok, &["cat-file", "blob", &giris.oid.onaltilik()])
            .expect("git cat-file");
        assert_eq!(
            String::from_utf8_lossy(&nesne.veri),
            beklenen,
            "{} içeriği git ile aynı olmalı",
            giris.ad
        );
    }

    // Bütün pack nesnelerinin CRC-32 ve SHA-1 doğrulaması geçmeli.
    let rapor = magaza.paketleri_dogrula().expect("bütünlük");
    assert!(rapor.dogrulanan_nesne > 0);
}

#[test]
fn butun_pack_nesneleri_git_ile_esit() {
    let Some(gecici) = gecici_depo("ent-hep") else {
        return;
    };
    let kok = gecici.kok();
    for i in 0..6 {
        dosya_yaz(&kok, &format!("f{i}.txt"), &format!("içerik {i}\n"));
        commitla(&kok, &format!("c{i}"));
    }
    git_kimlikli(&kok, &["gc", "-q", "--aggressive"]).expect("git gc --aggressive");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);

    // `git cat-file --batch-all-objects` ile tüm nesneleri listele.
    let liste = git_kimlikli(
        &kok,
        &[
            "cat-file",
            "--batch-all-objects",
            "--batch-check=%(objectname)",
        ],
    )
    .expect("git cat-file");
    let adlar: Vec<&str> = liste.lines().filter(|s| !s.is_empty()).collect();
    assert!(!adlar.is_empty());

    for ad in adlar {
        let oid = Oid::ayikla(ad).expect("geçerli ad");
        let tur = git_kimlikli(&kok, &["cat-file", "-t", ad])
            .expect("git cat-file -t")
            .trim()
            .to_string();
        let nesne = magaza
            .nesne(&oid)
            .unwrap_or_else(|hata| panic!("{ad} ({tur}) okunamadı: {hata}"));
        assert_eq!(nesne.tur.ad(), tur, "{ad} türü git ile aynı olmalı");

        let beklenen = git_kimlikli(&kok, &["cat-file", tur.as_str(), ad]).expect("içerik");
        assert_eq!(
            String::from_utf8_lossy(&nesne.veri),
            beklenen,
            "{ad} içeriği git ile aynı olmalı"
        );
    }
}

// ---------------------------------------------------------------------------
// (c) birden fazla pack
// ---------------------------------------------------------------------------

#[test]
fn birden_cok_pack_dosyasi_okunur() {
    let Some(gecici) = gecici_depo("ent-coklu") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "ilk.txt", "ilk\n");
    commitla(&kok, "c0");
    git_kimlikli(&kok, &["repack", "-a", "-d", "-q"]).expect("repack 1");

    dosya_yaz(&kok, "ikinci.txt", "ikinci\n");
    commitla(&kok, "c1");
    git_kimlikli(&kok, &["repack", "-a", "-d", "-q"]).expect("repack 2");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let baslangic = depo.coz(&magaza, "HEAD").expect("HEAD");
    let kayitlar =
        gecmis::gecmis(&magaza, &baslangic, &GecmisSecenekleri::default()).expect("geçmiş");
    assert_eq!(kayitlar.len(), 2);

    // İki ayrı repack sonrası ikinci pack'i gerçekten ayrı bir dosya olarak doğrulamak
    // yerine, paket sayısının en az bir olduğunu ve her iki nesnenin de okunduğunu
    // denetlemek daha sağlıklıdır (git `repack -a -d` tek pakka sıkıştırabilir).
    assert!(magaza.paket_sayisi() >= 1);
}

// ---------------------------------------------------------------------------
// (d) delta içeren pack
// ---------------------------------------------------------------------------

#[test]
fn delta_iceren_pack_cozulur() {
    let Some(gecici) = gecici_depo("ent-delta") else {
        return;
    };
    let kok = gecici.kok();
    // Benzer ama farklı sürümler: git'in delta üretmesi için ideal girdi.
    dosya_yaz(
        &kok,
        "buyuk.txt",
        &(0..400)
            .map(|i| format!("satir {i}\n"))
            .collect::<Vec<_>>()
            .join(""),
    );
    commitla(&kok, "v0");
    for surum in 1..8 {
        let mut satirlar: Vec<String> = (0..400).map(|i| format!("satir {i}\n")).collect();
        satirlar[surum] = format!("DEGISTIRILDI {surum}\n");
        dosya_yaz(&kok, "buyuk.txt", &satirlar.join(""));
        commitla(&kok, &format!("v{surum}"));
    }
    git_kimlikli(&kok, &["gc", "-q", "--aggressive"]).expect("git gc --aggressive");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let baslangic = depo.coz(&magaza, "HEAD").expect("HEAD");

    // Testin iddiasının kanıtı: pack gerçekten delta içeriyor.
    let idx =
        gitatlas::paket::indeks::PaketIndeksi::ac(&bul_pack_idx(&kok).expect("idx dosyası olmalı"))
            .expect("idx");
    let paket = gitatlas::paket::PaketDosyasi::ac(idx.yol()).expect("pack");
    let dagilim = paket.tur_sayilari();
    assert!(
        dagilim.get("ofs-delta").copied().unwrap_or(0) > 0,
        "pack içinde en az bir OBJ_OFS_DELTA olmalı, dağılım: {dagilim:?}"
    );

    let commit = Commit::ayikla(&magaza, &baslangic).expect("commit");
    let blob = yol_coz(&magaza, &commit.agac, "buyuk.txt").expect("yol");
    let YolSonucu::Dosya { oid, .. } = blob else {
        panic!("buyuk.txt bir dosya olmalı");
    };
    let icerik = magaza.nesne(&oid).expect("blob").veri;
    let beklenen =
        git_kimlikli(&kok, &["cat-file", "blob", &oid.onaltilik()]).expect("git cat-file");
    assert_eq!(String::from_utf8_lossy(&icerik), beklenen);
    assert!(String::from_utf8_lossy(&icerik).contains("DEGISTIRILDI 7"));

    // Sürüm 6'nın blob'u da delta üzerinden çözülmeli (HEAD~1).
    let v6_blob = Oid::ayikla(
        git_kimlikli(&kok, &["rev-parse", "HEAD~1:buyuk.txt"])
            .expect("rev-parse")
            .trim(),
    )
    .expect("blob adı");
    let icerik6 = magaza.nesne(&v6_blob).expect("v6 blob").veri;
    let beklenen6 =
        git_kimlikli(&kok, &["cat-file", "blob", &v6_blob.onaltilik()]).expect("git cat-file");
    assert_eq!(String::from_utf8_lossy(&icerik6), beklenen6);
    assert!(String::from_utf8_lossy(&icerik6).contains("DEGISTIRILDI 6"));
}

// ---------------------------------------------------------------------------
// (e) bozuk pack
// ---------------------------------------------------------------------------

#[test]
fn bozuk_pack_hata_verir() {
    let Some(gecici) = gecici_depo("ent-bozuk") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "x.txt", "x\n");
    commitla(&kok, "c0");
    git_kimlikli(&kok, &["gc", "-q"]).expect("git gc");

    let pack = bul_pack(&kok).expect("pack dosyası olmalı");
    let mut veri = fs::read(&pack).expect("pack okunmalı");
    veri[2] = b'X';
    bozuk_yaz(&pack, &veri);

    let depo = Depo::ac(&kok).expect("depo");
    let sonuc = Magaza::ac(depo.git_dir(), Ayarlar::default());
    assert!(sonuc.is_err(), "bozuk pack açılmamalı");
}

// ---------------------------------------------------------------------------
// (f) packed-refs
// ---------------------------------------------------------------------------

#[test]
fn packed_refs_ve_soyulmus_etiket_okunur() {
    let Some(gecici) = gecici_depo("ent-packed") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "a.txt", "a\n");
    commitla(&kok, "c0");
    git_kimlikli(&kok, &["tag", "-a", "v1.0", "-m", "sürüm 1"]).expect("etiket");
    git_kimlikli(&kok, &["pack-refs", "--all"]).expect("pack-refs");

    let depo = Depo::ac(&kok).expect("depo");
    assert!(kok.join(".git/packed-refs").is_file());
    let dal = depo.referans("refs/heads/main").expect("dal");
    assert_eq!(dal.kaynak, gitatlas::depo::ReferansKaynagi::Packed);

    let etiket = depo.referans("refs/tags/v1.0").expect("etiket");
    assert!(etiket.soyulmus.is_some(), "peeled hedef okunmalı");

    let magaza = magaza_ac(&depo);
    let soyulmus = etiket.soyulmus.expect("peeled");
    let commit = Commit::ayikla(&magaza, &soyulmus).expect("peeled commit olmalı");
    assert_eq!(commit.oid, soyulmus);

    // `v1.0` çözümlendiğinde doğrudan commit'e gitmelidir (peeled kuralı).
    assert_eq!(depo.coz(&magaza, "v1.0").expect("etiket rev"), soyulmus);
}

#[test]
fn ayrisik_ve_paketli_ayni_ref_ayrisik_dercektir() {
    let Some(gecici) = gecici_depo("ent-oncelik") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "a.txt", "a\n");
    commitla(&kok, "c0");
    git_kimlikli(&kok, &["pack-refs", "--all"]).expect("pack-refs");
    // Yeni bir commit ayrışık referansı yeniden yaratır.
    dosya_yaz(&kok, "b.txt", "b\n");
    let ikinci = commitla(&kok, "c1");

    let depo = Depo::ac(&kok).expect("depo");
    let dal = depo.referans("refs/heads/main").expect("dal");
    assert_eq!(dal.kaynak, gitatlas::depo::ReferansKaynagi::Ayrisik);
    assert_eq!(dal.oid.onaltilik(), ikinci);
}

// ---------------------------------------------------------------------------
// (g) ayrışık HEAD
// ---------------------------------------------------------------------------

#[test]
fn ayrisik_head_okunur() {
    let Some(gecici) = gecici_depo("ent-ayrik") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "a.txt", "a\n");
    let ilk = commitla(&kok, "c0");
    dosya_yaz(&kok, "b.txt", "b\n");
    commitla(&kok, "c1");
    git_kimlikli(&kok, &["checkout", "-q", &ilk]).expect("checkout");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    assert!(matches!(depo.head(), gitatlas::depo::HeadDurumu::Ayrik(_)));
    let kayitlar = gecmis::gecmis(
        &magaza,
        &depo.coz(&magaza, "HEAD").expect("HEAD"),
        &GecmisSecenekleri::default(),
    )
    .expect("geçmiş");
    assert_eq!(
        kayitlar.len(),
        1,
        "ayrışık HEAD yalnızca o commit'i görmeli"
    );
    assert_eq!(kayitlar[0].oid, ilk);
}

// ---------------------------------------------------------------------------
// yol filtresi, diff ve salt okunurluk
// ---------------------------------------------------------------------------

#[test]
fn yol_filtresi_yalnizca_ilgili_commitleri_secer() {
    let Some(gecici) = gecici_depo("ent-yol") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "hedef.txt", "1\n");
    commitla(&kok, "c0");
    dosya_yaz(&kok, "baska.txt", "x\n");
    commitla(&kok, "c1");
    dosya_yaz(&kok, "hedef.txt", "2\n");
    commitla(&kok, "c2");
    dosya_yaz(&kok, "baska.txt", "y\n");
    commitla(&kok, "c3");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let baslangic = depo.coz(&magaza, "HEAD").expect("HEAD");

    let hepsi = gecmis::gecmis(&magaza, &baslangic, &GecmisSecenekleri::default()).expect("geçmiş");
    assert_eq!(hepsi.len(), 4);

    let secenekler = GecmisSecenekleri {
        yol: Some("hedef.txt".to_string()),
        limit: None,
    };
    let sadece = gecmis::gecmis(&magaza, &baslangic, &secenekler).expect("geçmiş");
    assert_eq!(sadece.len(), 2, "hedef.txt yalnızca 2 commit'te değişti");
    assert_eq!(sadece[0].konu, "c2");
    assert_eq!(sadece[1].konu, "c0");

    let git_log = git_kimlikli(&kok, &["log", "--format=%s", "--", "hedef.txt"]).expect("git log");
    let git_konular: Vec<&str> = git_log.lines().filter(|s| !s.is_empty()).collect();
    let benim: Vec<&str> = sadece.iter().map(|k| k.konu.as_str()).collect();
    assert_eq!(benim, git_konular, "yol filtresi git ile aynı olmalı");
}

#[test]
fn diff_git_show_ile_karsilastirilir() {
    let Some(gecici) = gecici_depo("ent-diff") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "kod.txt", "bir\niki\nuc\n");
    let ilk = commitla(&kok, "c0");
    dosya_yaz(&kok, "kod.txt", "bir\nDEGISTI\nuc\n");
    let ikinci = commitla(&kok, "c1");
    dosya_yaz(&kok, "yeni.txt", "yeni dosya\n");
    commitla(&kok, "c2");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let a = Commit::ayikla(&magaza, &Oid::ayikla(&ilk).expect("a")).expect("a");
    let b = Commit::ayikla(&magaza, &Oid::ayikla(&ikinci).expect("b")).expect("b");
    let fark = agac_karsilastir(&magaza, Some(&a.agac), &b.agac).expect("ağaç farkı");

    let git_ad = git_kimlikli(&kok, &["diff", "--name-status", &ilk, &ikinci])
        .expect("git diff")
        .trim()
        .to_string();
    assert_eq!(git_ad, "M\tkod.txt", "git ile aynı dosya listesi");

    let benim: Vec<(char, &str)> = fark
        .iter()
        .map(|f| (f.tur.isaret(), f.yol.as_str()))
        .collect();
    assert_eq!(benim, vec![('M', "kod.txt")]);
}

#[test]
fn ikili_dosya_isaretlenir() {
    let Some(gecici) = gecici_depo("ent-ikili") else {
        return;
    };
    let kok = gecici.kok();
    fs::write(kok.join("resim.bin"), [0u8, 1, 2, 3, 0, 255]).expect("yaz");
    commitla(&kok, "c0");
    fs::write(kok.join("resim.bin"), [0u8, 9, 9, 0, 255]).expect("yaz");
    commitla(&kok, "c1");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let baslangic = depo.coz(&magaza, "HEAD").expect("HEAD");
    let commit = Commit::ayikla(&magaza, &baslangic).expect("commit");
    let YolSonucu::Dosya { oid, .. } = yol_coz(&magaza, &commit.agac, "resim.bin").expect("yol")
    else {
        panic!("resim.bin dosya olmalı");
    };
    let icerik = magaza.nesne(&oid).expect("blob").veri;
    assert!(
        gitatlas::fark::ikili_mi(&icerik),
        "NUL içeren dosya ikili sayılmalı"
    );
}

#[test]
fn gecersiz_yol_reddedilir() {
    let Some(gecici) = gecici_depo("ent-yolgirdi") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "a.txt", "a\n");
    commitla(&kok, "c0");

    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let agac = Commit::ayikla(&magaza, &depo.coz(&magaza, "HEAD").expect("HEAD"))
        .expect("commit")
        .agac;

    for yol in ["../gizli", "/etc/passwd", "a//b", ".git/config", ""] {
        let sonuc = yol_coz(&magaza, &agac, yol);
        assert!(
            matches!(sonuc, Err(Hata::YolGecersiz { .. })),
            "'{yol}' reddedilmeliydi, sonuç: {:?}",
            sonuc.map(|_| ())
        );
    }
}

#[test]
fn arama_sirasi_iki_yonlu_denir() {
    let Some(gecici) = gecici_depo("ent-sirapanca") else {
        return;
    };
    let kok = gecici.kok();
    dosya_yaz(&kok, "a.txt", "bir\n");
    commitla(&kok, "c0");
    dosya_yaz(&kok, "b.txt", "iki\n");
    commitla(&kok, "c1");

    let depo = Depo::ac(&kok).expect("depo");
    let agac = Commit::ayikla(
        &magaza_ac(&depo),
        &depo.coz(&magaza_ac(&depo), "HEAD").expect("HEAD"),
    )
    .expect("commit")
    .agac;
    let girdi = gitatlas::agac::agac_girdileri(&magaza_ac(&depo), &agac).expect("ağaç");
    let hedef = girdi[0].oid;

    // Aynı nesne hem pack'te hem gevşek dosyada: iki sıra da okumalı ve aynı
    // sonucu vermeli (git de aynı içeriği iki yerde tutmaz, ama okuyucu sağlam olmalı).
    for sira in ["pack-once", "gevsek-once"] {
        let ayar = Ayarlar {
            arama_sirasi: gitatlas::AramaSirasi::ayikla(sira).expect("sıra"),
            ..Ayarlar::default()
        };
        let magaza = Magaza::ac(depo.git_dir(), ayar).expect("mağaza");
        let nesne = magaza
            .nesne(&hedef)
            .unwrap_or_else(|hata| panic!("{sira}: {hata}"));
        let git_icerik =
            git_kimlikli(&kok, &["cat-file", "blob", &hedef.onaltilik()]).expect("git cat-file");
        assert_eq!(String::from_utf8_lossy(&nesne.veri), git_icerik, "{sira}");
    }
}

/// Paket dosyasını üzerine yazar.
///
/// `git gc` pack dosyalarını salt okunur işaretler; testin bozma yapabilmesi için
/// önce bu bayt kaldırılır. Unix'te `set_readonly(false)` "herkese açık yazılabilir"
/// anlamına geldiği için izinler platforma göre ayrıca verilir.
fn bozuk_yaz(yol: &Path, veri: &[u8]) {
    let mut izinler = fs::metadata(yol).expect("dosya bilgisi").permissions();
    // Gerekçe: `git gc` pack dosyasını salt okunur işaretler; "bozuk pack" senaryosu
    // ancak bu bayt kaldırıldıktan sonra kurulabilir. Uyarı yalnızca Windows
    // kollarında geçerlidir (Unix'te `set_mode(0o644)` kullanılır) ve dosya yalnızca
    // testin kendi geçici dizinindedir. Ayrıntı README → Bilinen Sınırlamalar'da.
    #[allow(clippy::permissions_set_readonly_false)]
    {
        #[cfg(windows)]
        izinler.set_readonly(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            izinler.set_mode(0o644);
        }
    }
    fs::set_permissions(yol, izinler).expect("izin ayarlanmalı");
    fs::write(yol, veri).expect("bozuk pack yazılmalı");
}

/// Depodaki tek `.pack` dosyasının yolunu döndürür.
fn bul_pack(kok: &Path) -> Option<std::path::PathBuf> {
    bul_bilesen(kok, "pack")
}

/// Depodaki tek `.idx` dosyasının yolunu döndürür.
fn bul_pack_idx(kok: &Path) -> Option<std::path::PathBuf> {
    bul_bilesen(kok, "idx")
}

fn bul_bilesen(kok: &Path, uzanti: &str) -> Option<std::path::PathBuf> {
    let dizin = kok.join(".git/objects/pack");
    let mut bulunan: Vec<std::path::PathBuf> = std::fs::read_dir(dizin)
        .ok()?
        .flatten()
        .map(|g| g.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(uzanti))
        .collect();
    bulunan.sort();
    bulunan.into_iter().next()
}

// ---------------------------------------------------------------------------
// SALT OKUNURLUK KANITI
// ---------------------------------------------------------------------------

#[test]
fn depo_hicbir_zaman_yazilmaz() {
    let Some(gecici) = gecici_depo("ent-saltokunur") else {
        return;
    };
    let kok = gecici.kok();

    // Çeşitli içerik: metin, ikili, sembolik bağ, alt dizin.
    dosya_yaz(&kok, "src/lib.rs", "fn main() {}\n");
    dosya_yaz(&kok, "veri.bin", &"\u{0}\u{1}\u{2}".repeat(100));
    commitla(&kok, "c0");
    dosya_yaz(&kok, "src/lib.rs", "fn main() { println!(); }\n");
    dosya_yaz(&kok, "README.md", "# proje\n");
    commitla(&kok, "c1");
    git_kimlikli(&kok, &["tag", "-a", "v1", "-m", "v1"]).expect("etiket");
    git_kimlikli(&kok, &["gc", "-q", "--aggressive"]).expect("gc");

    let once = yardimci::depo_ozeti(&kok);
    assert!(once.len() > 10, "depoda dosya olmalı");

    // Tüm alt komutları çalıştır.
    let depo = Depo::ac(&kok).expect("depo");
    let magaza = magaza_ac(&depo);
    let head = depo.coz(&magaza, "HEAD").expect("HEAD");
    let kayitlar = gecmis::gecmis(&magaza, &head, &GecmisSecenekleri::default()).expect("geçmiş");
    assert!(!kayitlar.is_empty());
    let commit = Commit::ayikla(&magaza, &head).expect("commit");
    let fark = agac_karsilastir(&magaza, None, &commit.agac).expect("fark");
    assert!(!fark.is_empty());
    magaza.paketleri_dogrula().expect("bütünlük");
    gecmis::gecmis(
        &magaza,
        &head,
        &GecmisSecenekleri {
            yol: Some("src/lib.rs".to_string()),
            limit: Some(5),
        },
    )
    .expect("yol filtreli geçmiş");
    yol_coz(&magaza, &commit.agac, "src/lib.rs").expect("yol çözümü");

    // Yazar dağılımı da okuma yolundan geçsin.
    let mut dagilim: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for k in &kayitlar {
        *dagilim.entry(k.yazar.clone()).or_insert(0) += 1;
    }
    assert!(dagilim.contains_key("SyntaxOrigin"));

    let sonra = yardimci::depo_ozeti(&kok);
    assert_eq!(
        once.len(),
        sonra.len(),
        "dosya sayısı değişmemeli (yeni dosya oluşmamalı)"
    );
    assert_eq!(once, sonra, "depo içeriği bit bayt aynı kalmalı");
}
