//! Pack okuma uçtan uca testleri: düz nesne, `OBJ_OFS_DELTA`, `OBJ_REF_DELTA`, delta
//! zinciri, derinlik tavanı, döngü koruması, birden çok pack, bozuk pack ve CRC.
//!
//! Fikstürler `gitatlas::paket::yazici` ile üretilir; bağımsız doğrulama
//! (`git cat-file` / `git log` / `git show`) `tests/entegrasyon.rs` içindedir.

mod yardimci;

use gitatlas::ayarlar::{AramaSirasi, Ayarlar};
use gitatlas::hata::Hata;
use gitatlas::magaza::Magaza;
use gitatlas::nesne::NesneTuru;
use gitatlas::oid::Oid;
use gitatlas::paket::yazici::{delta_basligi, delta_ekle, delta_kopya, PaketYazici};
use gitatlas::paket::PaketDosyasi;
use yardimci::GeciciDizin;

fn ayar() -> Ayarlar {
    Ayarlar::default()
}

/// `Ayarlar::nesne_tavani` varsayılanı. `matches!` deseni sabit yol istediği için
/// tekrar edilir; `devasa_hedef_boyutlu_delta_sureci_dusurmez` içinde varsayılanla
/// eşitliği de doğrulanır, böylece iki değer sessizce ayrışamaz.
const NESNE_TAVANI: u64 = 64 * 1024 * 1024;

#[test]
fn duz_nesne_packten_okunur() {
    let gecici = GeciciDizin::yeni("pack-duz").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let mut yazici = PaketYazici::yeni();
    let (oid, _) = yazici.duz(NesneTuru::Blob, b"merhaba dunya").expect("blob");
    yazici
        .yaz(&git.join("objects/pack"), "pack-a")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    assert_eq!(magaza.paket_sayisi(), 1);
    let nesne = magaza.nesne(&oid).expect("nesne okunmalı");
    assert_eq!(nesne.tur, NesneTuru::Blob);
    assert_eq!(nesne.veri, b"merhaba dunya");
}

#[test]
fn ofs_delta_cozulur() {
    let gecici = GeciciDizin::yeni("pack-ofs").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let taban_veri = b"ilk icerik satirlari".to_vec();
    let hedef = b"degistirilmis icerik".to_vec();

    let mut yazici = PaketYazici::yeni();
    let (taban, taban_ofset) = yazici.duz(NesneTuru::Blob, &taban_veri).expect("taban");
    let (delta, _) = yazici
        .ofs_delta(
            taban_ofset,
            NesneTuru::Blob,
            taban_veri.len() as u64,
            &hedef,
        )
        .expect("delta");
    yazici
        .yaz(&git.join("objects/pack"), "pack-ofs")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    assert_eq!(magaza.nesne(&taban).expect("taban").veri, taban_veri);
    assert_eq!(magaza.nesne(&delta).expect("delta").veri, hedef);
}

#[test]
fn ref_delta_cozulur() {
    let gecici = GeciciDizin::yeni("pack-ref").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let taban_veri = b"taban icerik".to_vec();
    let hedef = b"hedef icerik".to_vec();

    let mut yazici = PaketYazici::yeni();
    let (taban, _) = yazici.duz(NesneTuru::Blob, &taban_veri).expect("taban");
    let (delta, _) = yazici
        .ref_delta(taban, NesneTuru::Blob, taban_veri.len() as u64, &hedef)
        .expect("delta");
    yazici
        .yaz(&git.join("objects/pack"), "pack-ref")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    assert_eq!(magaza.nesne(&delta).expect("delta").veri, hedef);
}

#[test]
fn ref_delta_tabani_gevsek_nesneden_cozulur() {
    // Birleştiricinin merkezî iddiası: delta tabanı her iki yerden gelebilir.
    let gecici = GeciciDizin::yeni("pack-ref-loose").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let taban_veri = b"gevsek taban".to_vec();
    let hedef = b"delta sonucu".to_vec();

    let taban = yardimci::gevsek_yaz(&git.join("objects"), "blob", &taban_veri);
    let mut yazici = PaketYazici::yeni();
    let (delta, _) = yazici
        .ref_delta(taban, NesneTuru::Blob, taban_veri.len() as u64, &hedef)
        .expect("delta");
    yazici
        .yaz(&git.join("objects/pack"), "pack-karisik")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    assert_eq!(magaza.nesne(&delta).expect("delta").veri, hedef);
    let sayaclar = magaza.sayaclar();
    assert!(sayaclar.gevsek_isi >= 1, "taban gevşek nesneden gelmeliydi");
    assert!(sayaclar.paket_isi >= 1, "delta pack'ten gelmeliydi");
}

#[test]
fn delta_zinciri_cozulur() {
    let gecici = GeciciDizin::yeni("pack-zincir").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");

    let mut yazici = PaketYazici::yeni();
    let mut icerik = b"baslangic".to_vec();
    let (_, mut ofset) = yazici.duz(NesneTuru::Blob, &icerik).expect("ilk nesne");
    let mut zincir = vec![yazici.nesne_adlari()[0]];
    for adim in 1..5 {
        let taban_boyut = icerik.len() as u64;
        icerik = format!("adim-{adim}-icerik").into_bytes();
        let (yeni_oid, yeni_ofset) = yazici
            .ofs_delta(ofset, NesneTuru::Blob, taban_boyut, &icerik)
            .expect("delta");
        zincir.push(yeni_oid);
        ofset = yeni_ofset;
    }
    yazici
        .yaz(&git.join("objects/pack"), "pack-zincir")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    for adim in (0..5).rev() {
        let beklenen = if adim == 0 {
            b"baslangic".to_vec()
        } else {
            format!("adim-{adim}-icerik").into_bytes()
        };
        let okunan = magaza.nesne(&zincir[adim]).expect("delta çözülmeli").veri;
        assert_eq!(okunan, beklenen, "adım {adim} yanlış çözüldü");
    }
}

#[test]
fn delta_derinligi_tavani_asilir() {
    let gecici = GeciciDizin::yeni("pack-derinlik").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");

    let mut yazici = PaketYazici::yeni();
    let mut icerik = b"x".to_vec();
    let (_, mut ofset) = yazici.duz(NesneTuru::Blob, &icerik).expect("ilk");
    let mut zincir = vec![yazici.nesne_adlari()[0]];
    for adim in 0..6 {
        let taban_boyut = icerik.len() as u64;
        icerik = format!("v{adim:02}{}", "y".repeat(adim)).into_bytes();
        let (yeni_oid, yeni_ofset) = yazici
            .ofs_delta(ofset, NesneTuru::Blob, taban_boyut, &icerik)
            .expect("delta");
        zincir.push(yeni_oid);
        ofset = yeni_ofset;
    }
    yazici
        .yaz(&git.join("objects/pack"), "pack-derinlik")
        .expect("yaz");

    let sığan = Magaza::ac(&git, ayar()).expect("mağaza");
    assert!(
        sığan.nesne(&zincir[6]).is_ok(),
        "varsayılan tavan (64) yeterli olmalı"
    );

    let dar = Magaza::ac(
        &git,
        Ayarlar {
            delta_derinligi: 2,
            ..ayar()
        },
    )
    .expect("mağaza");
    let hata = dar.nesne(&zincir[6]).expect_err("derinlik tavanı aşılmalı");
    assert!(
        matches!(hata, Hata::DeltaZinciriAsildi { .. }),
        "beklenmeyen hata: {hata}"
    );
}

#[test]
fn ref_delta_dongusu_tespit_edilir() {
    let gecici = GeciciDizin::yeni("pack-dongu").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");

    // A → B → A: gerçek git'in asla üretmediği, yalnızca bozuklukta oluşan bir durum.
    let a = Oid::ayikla("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").expect("a");
    let b = Oid::ayikla("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").expect("b");

    let mut yazici = PaketYazici::yeni();
    let mut ham_a = delta_basligi(4, 4);
    ham_a.extend_from_slice(&delta_ekle(b"aaaa"));
    yazici
        .sahte_giris(a, 7, b.baytlar(), &ham_a)
        .expect("A girdisi");
    let mut ham_b = delta_basligi(4, 4);
    ham_b.extend_from_slice(&delta_ekle(b"bbbb"));
    yazici
        .sahte_giris(b, 7, a.baytlar(), &ham_b)
        .expect("B girdisi");
    yazici
        .yaz(&git.join("objects/pack"), "pack-dongu")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    let hata = magaza.nesne(&a).expect_err("döngü hata vermeli");
    assert!(
        matches!(hata, Hata::DeltaDongusu { .. }),
        "beklenmeyen hata: {hata}"
    );
}

#[test]
fn birden_cok_pack_dosyasi_okunur() {
    let gecici = GeciciDizin::yeni("pack-coklu").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let pack = git.join("objects/pack");

    let mut ilk = PaketYazici::yeni();
    let (a, _) = ilk.duz(NesneTuru::Blob, b"birinci pack").expect("a");
    ilk.yaz(&pack, "pack-001").expect("yaz");

    let mut ikinci = PaketYazici::yeni();
    let (b, _) = ikinci.duz(NesneTuru::Blob, b"ikinci pack").expect("b");
    ikinci.yaz(&pack, "pack-002").expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    assert_eq!(magaza.paket_sayisi(), 2);
    assert_eq!(magaza.nesne(&a).expect("a").veri, b"birinci pack");
    assert_eq!(magaza.nesne(&b).expect("b").veri, b"ikinci pack");
}

#[test]
fn bozuk_pack_imzasi_reddedilir() {
    let gecici = GeciciDizin::yeni("pack-bozuk").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let pack = git.join("objects/pack");

    let mut yazici = PaketYazici::yeni();
    let (oid, _) = yazici.duz(NesneTuru::Blob, b"icerik").expect("blob");
    let (pack_yolu, _) = yazici.yaz(&pack, "pack-bozuk").expect("yaz");
    let boyut = std::fs::metadata(&pack_yolu).expect("meta").len();

    // 1) PACK imzasını boz: pack açılmaz.
    let mut veri = std::fs::read(&pack_yolu).expect("okunmalı");
    veri[0] = b'X';
    std::fs::write(&pack_yolu, &veri).expect("bozuk pack yazılmalı");
    let hata = Magaza::ac(&git, ayar()).expect_err("bozuk imza reddedilmeli");
    assert!(matches!(hata, Hata::PaketBozuk { .. }));

    // 2) Özet baytını boz: idx'deki paket özetiyle eşleşmez.
    let mut veri = std::fs::read(&pack_yolu).expect("okunmalı");
    veri[0] = b'P';
    veri[boyut as usize - 5] ^= 0xFF;
    std::fs::write(&pack_yolu, &veri).expect("bozuk özet yazılmalı");
    let hata = Magaza::ac(&git, ayar()).expect_err("özet uyuşmazlığı reddedilmeli");
    assert!(matches!(hata, Hata::PaketBozuk { .. }));
    assert!(hata.to_string().contains("özet"));

    // 3) Gövde baytını boz: imza ve özet sağlam kalır, ama tam bütünlük denetimi yakalar.
    let mut yazici = PaketYazici::yeni();
    let (oid2, _) = yazici.duz(NesneTuru::Blob, b"diger").expect("blob2");
    let (pack_yolu, _) = yazici.yaz(&pack, "pack-govde2").expect("yaz");
    let mut veri = std::fs::read(&pack_yolu).expect("okunmalı");
    veri[20] ^= 0xFF;
    std::fs::write(&pack_yolu, &veri).expect("bozuk gövde yazılmalı");
    let indeks =
        gitatlas::paket::indeks::PaketIndeksi::ac(&pack.join("pack-govde2.idx")).expect("idx");
    let paket = gitatlas::paket::PaketDosyasi::ac(indeks.yol()).expect("pack açılır");
    let hata = paket
        .butunluk_denetle()
        .expect_err("bütünlük denetimi yakalamalı");
    assert!(hata.to_string().contains("pack özeti"));
    let _ = (oid, oid2);
}

#[test]
fn bozuk_pack_govdesi_zlib_hatasi_verir() {
    let gecici = GeciciDizin::yeni("pack-govde").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let pack = git.join("objects/pack");

    let mut yazici = PaketYazici::yeni();
    let (oid, _) = yazici.duz(NesneTuru::Blob, &vec![b'z'; 600]).expect("blob");
    let (pack_yolu, _) = yazici.yaz(&pack, "pack-govde").expect("yaz");

    // Sıkıştırılmış gövdeyi boz, sonra pack özetini yeniden hesapla ki açma geçsin.
    let mut veri = std::fs::read(&pack_yolu).expect("okunmalı");
    for bayt in veri.iter_mut().take(40).skip(16) {
        *bayt ^= 0xA5;
    }
    let son = veri.len() - 20;
    let yeni_ozet = gitatlas::sha1::sha1(&veri[..son]);
    veri[son..].copy_from_slice(&yeni_ozet);
    std::fs::write(&pack_yolu, &veri).expect("bozuk gövde yazılmalı");

    // idx içindeki paket özeti artık tutmuyor; onu da güncelle.
    guncelle_idx_paket_ozeti(&pack, "pack-govde");
    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    let sonuc = magaza.nesne(&oid);
    assert!(sonuc.is_err(), "bozuk gövde hata vermeli");
}

#[test]
fn crc_uyusmazligi_tespit_edilir() {
    let gecici = GeciciDizin::yeni("pack-crc").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let pack = git.join("objects/pack");

    let mut yazici = PaketYazici::yeni();
    let (oid, _) = yazici.duz(NesneTuru::Blob, b"crc icin").expect("blob");
    yazici.yaz(&pack, "pack-crc").expect("yaz");

    // idx'deki CRC alanını boz, sonra indeks özetini yeniden hesapla.
    let idx_yolu = pack.join("pack-crc.idx");
    let mut veri = std::fs::read(&idx_yolu).expect("okunmalı");
    let crc_tab = 8 + 1024 + 20; // 1 nesne
    veri[crc_tab] ^= 0xFF;
    let son = veri.len();
    let yeni = gitatlas::sha1::sha1(&veri[..son - 20]);
    veri[son - 20..].copy_from_slice(&yeni);
    std::fs::write(&idx_yolu, &veri).expect("bozuk idx yazılmalı");

    let paket = PaketDosyasi::ac(&idx_yolu).expect("pack açılmalı");
    let hata = paket
        .crc_dogrula(&oid, &ayar())
        .expect_err("CRC uyuşmazlığı hata vermeli");
    assert!(matches!(hata, Hata::PaketBozuk { .. }));
    assert!(hata.to_string().contains("CRC"));
}

#[test]
fn butunluk_denetle_gecerli_pack_i_onaylar() {
    let gecici = GeciciDizin::yeni("pack-butunluk").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let pack = git.join("objects/pack");

    let mut yazici = PaketYazici::yeni();
    let (a, _) = yazici.duz(NesneTuru::Blob, b"bir").expect("a");
    let (b, _) = yazici.duz(NesneTuru::Agac, b"40000 x\0").expect("b");
    yazici.yaz(&pack, "pack-ok").expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    let rapor = magaza.paketleri_dogrula().expect("bütünlük denetimi");
    assert_eq!(rapor.paket_sayisi, 1);
    assert_eq!(rapor.dogrulanan_nesne, 2);
    assert!(magaza.nesne(&a).is_ok() && magaza.nesne(&b).is_ok());
}

#[test]
fn arama_sirasi_pack_once_bozuk_gevsegi_atlar() {
    let gecici = GeciciDizin::yeni("pack-sira-pack").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let (oid, _) = ikiz_kopya_olustur(&git, "pack-sira-pack");

    let once = Magaza::ac(
        &git,
        Ayarlar {
            arama_sirasi: AramaSirasi::PackOnce,
            ..ayar()
        },
    )
    .expect("mağaza");
    assert_eq!(
        once.nesne(&oid).expect("pack kopyası okunmalı").veri,
        b"icerik"
    );

    let sonra = Magaza::ac(
        &git,
        Ayarlar {
            arama_sirasi: AramaSirasi::GevsekOnce,
            ..ayar()
        },
    )
    .expect("mağaza");
    let hata = sonra
        .nesne(&oid)
        .expect_err("bozuk gevşek kopya hata vermeli");
    assert!(
        matches!(hata, Hata::NesneBozuk { .. } | Hata::ZlibHatasi { .. }),
        "beklenmeyen hata: {hata}"
    );
}

#[test]
fn arama_sirasi_gevsek_once_bir_gevsigi_atlar() {
    let gecici = GeciciDizin::yeni("pack-sira-gevsek").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let (oid, _) = ikiz_kopya_olustur(&git, "pack-sira-gevsek");

    let sonra = Magaza::ac(
        &git,
        Ayarlar {
            arama_sirasi: AramaSirasi::GevsekOnce,
            ..ayar()
        },
    )
    .expect("mağaza");
    assert!(sonra.nesne(&oid).is_err(), "bozuk gevşek kopya okunamaz");

    let once = Magaza::ac(
        &git,
        Ayarlar {
            arama_sirasi: AramaSirasi::PackOnce,
            ..ayar()
        },
    )
    .expect("mağaza");
    assert_eq!(once.nesne(&oid).expect("pack kopyası").veri, b"icerik");
}

#[test]
fn bulunmayan_nesne_hata_verir() {
    let gecici = GeciciDizin::yeni("pack-yok").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");
    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    let yok = Oid::ayikla("00000000000000000000000000000000000000ff").expect("ad");
    let hata = magaza.nesne(&yok).expect_err("hata vermeli");
    assert!(matches!(hata, Hata::NesneBulunamadi { .. }));
}

/// Regresyon (DoS): delta başlığında bildirilen hedef boyut 10 baytlık varint ile
/// `u64::MAX`'e kadar yazılabilir. Bu sayı doğrudan `Vec::with_capacity` girdiğinde
/// `capacity overflow` paniği doğurur; profil `panic = "abort"` olduğu için bu bir hata
/// değil **sürecin düşmesidir** (`0xC0000409`). Test üç ayrı devasa değerle, süreç
/// ayakta kalırken kontrollü `CozmeSiniriAsildi` hatası üretildiğini doğrular.
#[test]
fn devasa_hedef_boyutlu_delta_sureci_dusurmez() {
    for bildirilen in [u64::MAX, 1u64 << 63, 1u64 << 40] {
        let etiket = format!("pack-dos-{bildirilen}");
        let gecici = GeciciDizin::yeni(&etiket).expect("geçici dizin");
        let git = gecici.iskelet().expect("iskelet");
        assert_eq!(ayar().nesne_tavani, NESNE_TAVANI);

        let taban_veri = b"taban".to_vec();
        let mut yazici = PaketYazici::yeni();
        let (_, taban_ofset) = yazici.duz(NesneTuru::Blob, &taban_veri).expect("taban");
        let (kotu, _) = yazici
            .ofs_delta_bildirilen(
                taban_ofset,
                NesneTuru::Blob,
                taban_veri.len() as u64,
                b"aaaa",
                bildirilen,
                &delta_ekle(b"aaaa"),
            )
            .expect("saldırgan delta");
        yazici
            .yaz(&git.join("objects/pack"), "pack-dos")
            .expect("yaz");

        // 1) Nesne çözümü kontrollü hata verir (panik yok).
        let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
        let hata = magaza.nesne(&kotu).expect_err("tavan aşımı hata vermeli");
        assert!(
            matches!(
                hata,
                Hata::CozmeSiniriAsildi {
                    bayt,
                    tav: NESNE_TAVANI
                } if bayt == bildirilen
            ),
            "beklenmeyen hata ({bildirilen}): {hata}"
        );

        // 2) `ozet --butunluk` yolunun karşılığı olan bütünlük denetimi de
        //    düşmez: her nesne çözülürken aynı hataya çarpar.
        let tam = Magaza::ac(&git, ayar()).expect("mağaza");
        let rapor = tam.paketleri_dogrula();
        assert!(
            rapor.is_err(),
            "bütünlük denetimi de saldırgan nesneyi reddetmeli"
        );
    }
}

/// Meşru ve tavan içinde kalan büyük delta hâlâ okunmalı: blok blok büyüme
/// kaldırılan ön tahsisi telafi etmiyor, yani bu bir işlevsel bozulma değil.
#[test]
fn tavan_icindeki_buyuk_delta_cozulur() {
    const BOYUT: usize = 8 * 1024 * 1024;
    let gecici = GeciciDizin::yeni("pack-buyuk-delta").expect("geçici dizin");
    let git = gecici.iskelet().expect("iskelet");

    // Taban bilinçli olarak küçük tutulur: `yazici::mesafe_kodla` ofs-delta
    // mesafesini tek bayta sığdıracak biçimde kodlar (bilinen yazıcı sınırı,
    // gerçek git'in 7-bit zincir kodlaması değil). Mesafe 128 baytı aşarsa bu
    // fikstür geçersiz bir pack üretir; 64 baytlık taban mesafeyi tek baytta tutar.
    let taban_veri = vec![b'g'; 64];
    let hedef_veri = vec![b'g'; BOYUT];
    // Tabanı 64 baytlık bloklar hâlinde kopyala: 131.072 komut, 8 MiB hedef.
    let kopya = delta_kopya(0, 64);
    let tekrar = BOYUT / 64;
    let mut komutlar = Vec::with_capacity(tekrar * kopya.len());
    for _ in 0..tekrar {
        komutlar.extend_from_slice(&kopya);
    }

    let mut yazici = PaketYazici::yeni();
    let (taban, taban_ofset) = yazici.duz(NesneTuru::Blob, &taban_veri).expect("taban");
    let (delta, _) = yazici
        .ofs_delta_ozel(
            taban_ofset,
            NesneTuru::Blob,
            taban_veri.len() as u64,
            &hedef_veri,
            &komutlar,
        )
        .expect("büyük delta");
    yazici
        .yaz(&git.join("objects/pack"), "pack-buyuk")
        .expect("yaz");

    let magaza = Magaza::ac(&git, ayar()).expect("mağaza");
    let okunan = magaza.nesne(&delta).expect("büyük delta okunmalı").veri;
    assert_eq!(okunan.len(), BOYUT);
    assert_eq!(okunan, hedef_veri);
    // Taban da bozulmadan okunabilmeli.
    assert_eq!(magaza.nesne(&taban).expect("taban").veri, taban_veri);
}

/// Aynı nesneyi hem pack'e hem gevşek dosyaya yazar; gevşek kopya **bozuktur**.
fn ikiz_kopya_olustur(git: &std::path::Path, etiket: &str) -> (Oid, ()) {
    let mut yazici = PaketYazici::yeni();
    let (oid, _) = yazici.duz(NesneTuru::Blob, b"icerik").expect("blob");
    yazici
        .yaz(&git.join("objects/pack"), etiket)
        .expect("pack yazılmalı");

    // Aynı ada sahip, ama içeriği uyuşmayan bir gevşek dosya.
    let onaltilik = oid.onaltilik();
    let dizi = git.join("objects").join(&onaltilik[..2]);
    std::fs::create_dir_all(&dizi).expect("dizin");
    std::fs::write(dizi.join(&onaltilik[2..]), b"bu bir zlib akisi degil").expect("yaz");
    (oid, ())
}

/// idx dosyasındaki paket özetini pack dosyasınınkiyle eşitler.
fn guncelle_idx_paket_ozeti(pack_dir: &std::path::Path, ad: &str) {
    let pack = std::fs::read(pack_dir.join(format!("{ad}.pack"))).expect("pack okunmalı");
    let son = pack.len();
    let paket_ozeti = gitatlas::sha1::sha1(&pack[..son - 20]);

    let idx_yolu = pack_dir.join(format!("{ad}.idx"));
    let mut veri = std::fs::read(&idx_yolu).expect("idx okunmalı");
    // Paket özeti, indeks özetinin 20 bayt öncesinde durur.
    let konum = veri.len() - 40;
    veri[konum..konum + 20].copy_from_slice(&paket_ozeti);
    let yeni = gitatlas::sha1::sha1(&veri[..veri.len() - 20]);
    let son = veri.len();
    veri[son - 20..].copy_from_slice(&yeni);
    std::fs::write(&idx_yolu, &veri).expect("idx yazılmalı");
}
