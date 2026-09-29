//! Depo katmanı testleri: `.git` keşfi, HEAD (ayrışık/dogmamış), ayrışık referanslar,
//! `packed-refs`, soyulmuş (peeled) işaretli etiketler, öncelik kuralı ve revizyon çözümü.
//!
//! Bu testler `git` çalıştırmaz: `.git` iskeleti doğrudan yazılır, böylece sonuç
//! tamamen deterministiktir. Gerçek `git` depolarıyla karşılaştırma
//! `tests/entegrasyon.rs` içindedir.

mod yardimci;

use std::fs;

use gitatlas::ayarlar::Ayarlar;
use gitatlas::depo::{Depo, HeadDurumu, ReferansKaynagi};
use gitatlas::hata::Hata;
use gitatlas::magaza::Magaza;
use gitatlas::oid::Oid;
use yardimci::GeciciDizin;

fn oid(s: &str) -> Oid {
    let mut tam = s.repeat(40);
    tam.truncate(40);
    Oid::ayikla(&tam).expect("geçerli nesne adı")
}

#[test]
fn depo_acma_calisma_agacindan_basar() {
    let gecici = GeciciDizin::yeni("depo-agac").expect("geçici dizin");
    let _git = gecici.iskelet().expect("iskelet");
    let depo = Depo::ac(gecici.kok().as_path()).expect("depo açılmalı");
    assert!(!depo.ham_mi());
    // `Depo::ac` Windows'un `\\?\` önekini temizler; gösterilen yol okunabilir kalmalı.
    assert!(depo.kok().ends_with(gecici.yol().file_name().expect("ad")));
    assert!(!depo.kok().to_string_lossy().starts_with(r"\\?\"));
    assert!(depo.git_dir().ends_with(".git"));
}

#[test]
fn depo_acma_git_dizininden_basar() {
    let gecici = GeciciDizin::yeni("depo-gitdir").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let depo = Depo::ac(&git).expect("depo açılmalı");
    assert!(depo.git_dir().ends_with(".git"));
    assert!(depo.git_dir().is_dir());
}

#[test]
fn depo_acma_ciplak_repo_basar() {
    let gecici = GeciciDizin::yeni("depo-bare").expect("geçici dizin");
    fs::create_dir_all(gecici.yol().join("objects/pack")).expect("nesne dizini");
    fs::create_dir_all(gecici.yol().join("refs/heads")).expect("refs");
    fs::write(gecici.yol().join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
    let depo = Depo::ac(gecici.yol()).expect("çıplak depo açılmalı");
    assert!(depo.ham_mi());
}

#[test]
fn depo_acma_yonlendirme_dosyasi_basar() {
    let gecici = GeciciDizin::yeni("depo-yonlendirme").expect("geçici dizin");
    let alt = gecici.yol().join("gercek-depo");
    fs::create_dir_all(alt.join("objects")).expect("objects");
    fs::create_dir_all(alt.join("refs")).expect("refs");
    fs::write(alt.join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
    fs::create_dir_all(gecici.yol().join("is")).expect("iş ağacı");
    fs::write(
        gecici.yol().join("is").join(".git"),
        "gitdir: ../gercek-depo\n",
    )
    .expect("yönlendirme");

    let depo = Depo::ac(&gecici.yol().join("is")).expect("yönlendirme çözülmeli");
    assert!(depo.git_dir().ends_with("gercek-depo"));
}

#[test]
fn depo_bulunamazsa_hata_verir() {
    let gecici = GeciciDizin::yeni("depo-yok").expect("geçici dizin");
    let hata = Depo::ac(gecici.yol()).expect_err("depo bulunmamalı");
    assert!(matches!(hata, Hata::DepoBulunamadi { .. }));
}

#[test]
fn head_dal_durumu_cozulur() {
    let gecici = GeciciDizin::yeni("depo-dal").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let depo = Depo::ac(&git).expect("depo");
    match depo.head() {
        HeadDurumu::Dogmadi(ad) => assert_eq!(ad, "refs/heads/main"),
        diger => panic!("beklenmeyen HEAD durumu: {diger:?}"),
    }
}

#[test]
fn head_ayrik_durum_cozulur() {
    let gecici = GeciciDizin::yeni("depo-ayrik").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let ad = oid("c");
    fs::write(git.join("HEAD"), format!("{}\n", ad.onaltilik())).expect("HEAD");
    let depo = Depo::ac(&git).expect("depo");
    assert!(matches!(depo.head(), HeadDurumu::Ayrik(_)));
    assert_eq!(depo.head_ayrik(), Some(ad));
}

#[test]
fn head_bozuk_icerik_reddedilir() {
    let gecici = GeciciDizin::yeni("depo-headbozuk").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    fs::write(git.join("HEAD"), "bu bir referans degil\n").expect("HEAD");
    let hata = Depo::ac(&git).expect_err("bozuk HEAD reddedilmeli");
    assert!(matches!(hata, Hata::HeadOkunamadi { .. }));
}

#[test]
fn ayrisik_referanslar_okunur() {
    let gecici = GeciciDizin::yeni("depo-ayrisik").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let ana = oid("a");
    let ic = oid("b");
    fs::write(
        git.join("refs/heads/main"),
        format!("{}\n", ana.onaltilik()),
    )
    .expect("main");
    fs::create_dir_all(git.join("refs/heads/ozel")).expect("alt dal");
    fs::write(
        git.join("refs/heads/ozel/deneme"),
        format!("{}\n", ic.onaltilik()),
    )
    .expect("dal");

    let depo = Depo::ac(&git).expect("depo");
    assert_eq!(depo.dallar().count(), 2);
    let dal = depo.referans("refs/heads/ozel/deneme").expect("iç içe dal");
    assert_eq!(dal.oid, ic);
    assert_eq!(dal.kaynak, ReferansKaynagi::Ayrisik);
    assert_eq!(dal.kisa_ad(), "ozel/deneme");
}

#[test]
fn packed_refs_okunur() {
    let gecici = GeciciDizin::yeni("depo-packed").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let ana = oid("a");
    let etiket = oid("b");
    fs::write(
        git.join("packed-refs"),
        format!(
            "# pack-refs with: peeled fully-peeled sorted \n{} refs/heads/main\n{} refs/tags/v1\n",
            ana.onaltilik(),
            etiket.onaltilik()
        ),
    )
    .expect("packed-refs");

    let depo = Depo::ac(&git).expect("depo");
    let dal = depo.referans("refs/heads/main").expect("paketli dal");
    assert_eq!(dal.oid, ana);
    assert_eq!(dal.kaynak, ReferansKaynagi::Packed);
    assert_eq!(depo.dallar().count(), 1);
    assert_eq!(depo.etiketler().count(), 1);
}

#[test]
fn packed_refs_soyulmus_etiket_cozulur() {
    let gecici = GeciciDizin::yeni("depo-soyulmus").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let etiket = oid("1");
    let hedef = oid("2");
    fs::write(
        git.join("packed-refs"),
        format!(
            "# pack-refs with: peeled fully-peeled sorted \n{} refs/tags/v1\n^{}\n",
            etiket.onaltilik(),
            hedef.onaltilik()
        ),
    )
    .expect("packed-refs");

    let depo = Depo::ac(&git).expect("depo");
    let kayit = depo.referans("refs/tags/v1").expect("etiket");
    assert_eq!(kayit.oid, etiket);
    assert_eq!(kayit.soyulmus, Some(hedef));
}

#[test]
fn ayrisik_referans_paketliden_onceliklidir() {
    let gecici = GeciciDizin::yeni("depo-oncelik").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let paketli = oid("a");
    let ayrisik = oid("b");
    fs::write(
        git.join("packed-refs"),
        format!("{} refs/heads/main\n", paketli.onaltilik()),
    )
    .expect("packed-refs");
    fs::write(
        git.join("refs/heads/main"),
        format!("{}\n", ayrisik.onaltilik()),
    )
    .expect("ayrışık dal");

    let depo = Depo::ac(&git).expect("depo");
    let dal = depo.referans("refs/heads/main").expect("dal");
    assert_eq!(dal.oid, ayrisik, "ayrışık dosya kazanmalı");
    assert_eq!(dal.kaynak, ReferansKaynagi::Ayrisik);
}

#[test]
fn ayrisik_dal_soyulmus_hedefi_korur() {
    let gecici = GeciciDizin::yeni("depo-soyulmus-koru").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let etiket = oid("1");
    let hedef = oid("2");
    let yeni = oid("3");
    fs::write(
        git.join("packed-refs"),
        format!(
            "{} refs/tags/v1\n^{}\n",
            etiket.onaltilik(),
            hedef.onaltilik()
        ),
    )
    .expect("packed-refs");
    fs::write(git.join("refs/tags/v1"), format!("{}\n", yeni.onaltilik())).expect("ayrışık etiket");

    let depo = Depo::ac(&git).expect("depo");
    let kayit = depo.referans("refs/tags/v1").expect("etiket");
    assert_eq!(kayit.oid, yeni);
    assert_eq!(kayit.soyulmus, Some(hedef), "peeled kayıt ezilmemeli");
}

#[test]
fn bozuk_packed_refs_reddedilir() {
    let gecici = GeciciDizin::yeni("depo-packedbozuk").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    fs::write(git.join("packed-refs"), "notanoid refs/heads/main\n").expect("packed-refs");
    let hata = Depo::ac(&git).expect_err("bozuk packed-refs reddedilmeli");
    assert!(matches!(hata, Hata::PackedRefsBozuk { .. }));
}

#[test]
fn alternates_bildirilir_ama_izlenmez() {
    let gecici = GeciciDizin::yeni("depo-alternates").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    fs::create_dir_all(git.join("objects/info")).expect("info");
    fs::write(
        git.join("objects/info/alternates"),
        "/diger/depo/objects\n# yorum\n",
    )
    .expect("alternates");

    let depo = Depo::ac(&git).expect("depo");
    assert_eq!(depo.alternates().len(), 1);
    assert!(depo.alternates()[0].ends_with("objects"));
}

#[test]
fn revizyon_cozumleme_calisir() {
    let gecici = GeciciDizin::yeni("depo-rev").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let etiket_hedef = oid("c");

    let agac = yardimci::agac_yaz(&git.join("objects"), &[]);
    let ilk = yardimci::commit_yaz(&git.join("objects"), agac, &[], "Ada", 100, "ilk");
    let ikinci = yardimci::commit_yaz(&git.join("objects"), agac, &[ilk], "Ada", 200, "ikinci");
    fs::write(
        git.join("refs/heads/main"),
        format!("{}\n", ikinci.onaltilik()),
    )
    .expect("main");
    fs::write(
        git.join("refs/tags/v1"),
        format!("{}\n", etiket_hedef.onaltilik()),
    )
    .expect("etiket");

    let depo = Depo::ac(&git).expect("depo");
    let magaza = Magaza::ac(&git, Ayarlar::default()).expect("mağaza");
    assert_eq!(depo.coz(&magaza, "HEAD").expect("HEAD"), ikinci);
    assert_eq!(depo.coz(&magaza, "main").expect("dal"), ikinci);
    assert_eq!(depo.coz(&magaza, "v1").expect("etiket"), etiket_hedef);
    assert_eq!(depo.coz(&magaza, "HEAD~1").expect("HEAD~1"), ilk);
    assert_eq!(
        depo.coz(&magaza, &ikinci.onaltilik())
            .expect("tam nesne adı"),
        ikinci
    );
    assert!(matches!(depo.head(), HeadDurumu::Dal(ad) if ad == "refs/heads/main"));
}

#[test]
fn bilinmeyen_revizyon_hata_verir() {
    let gecici = GeciciDizin::yeni("depo-rev-yok").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let depo = Depo::ac(&git).expect("depo");
    let magaza = Magaza::ac(&git, Ayarlar::default()).expect("mağaza");
    let hata = depo
        .coz(&magaza, "olmayan-dal")
        .expect_err("bilinmeyen revizyon hata vermeli");
    assert!(matches!(hata, Hata::RevCozulemedi { .. }));
}

#[test]
fn kisaltilmis_nesne_adi_cozulmez() {
    let gecici = GeciciDizin::yeni("depo-kisa").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let depo = Depo::ac(&git).expect("depo");
    let magaza = Magaza::ac(&git, Ayarlar::default()).expect("mağaza");
    assert!(depo.coz(&magaza, "abc1234").is_err());
}
