//! Gevşek (loose) nesne okuma: `.git/objects/XX/YYYY…` yolu, zlib inflate, başlık
//! ayrıştırma ve **SHA-1 doğrulaması**.
//!
//! Kapsam: yalnızca gevşek nesneler. Pack içindeki nesneler [`crate::paket`] modülündedir.
//! Bu modül hiçbir zaman diske yazmaz; dosyalar yalnızca `File::open` ile açılır.

use std::fs;
use std::path::{Path, PathBuf};

use crate::hata::Hata;
use crate::nesne::{baslik_ayikla, Nesne};
use crate::oid::Oid;
use crate::zlib::inflate;

/// Bir nesnenin gevşek olarak saklandığı yolu üretir: `objects/ab/cdef…`.
///
/// Nesne adı geçersizse hata verir; böylece yol geçiş saldırısı (`../`) mümkün olmaz.
pub fn nesne_yolu(objects_dir: &Path, oid: &Oid) -> Result<PathBuf, Hata> {
    let onaltilik = oid.onaltilik();
    Ok(objects_dir.join(&onaltilik[..2]).join(&onaltilik[2..]))
}

/// Bir nesnenin gevşek olarak var olup olmadığını bildirir.
pub fn var_mi(objects_dir: &Path, oid: &Oid) -> bool {
    nesne_yolu(objects_dir, oid)
        .map(|yol| yol.is_file())
        .unwrap_or(false)
}

/// Gevşek nesneyi okur, zlib ile çözer, başlığı ayrıştırır ve SHA-1'ini doğrular.
///
/// `boyut_tavani` çözülen yükün üst sınırıdır; aşılırsa [`Hata::CozmeSiniriAsildi`].
pub fn oku(objects_dir: &Path, oid: &Oid, boyut_tavani: u64) -> Result<Nesne, Hata> {
    let yol = nesne_yolu(objects_dir, oid)?;
    let ham = fs::read(&yol).map_err(|hata| Hata::io(&yol, hata))?;
    let cozulmus = inflate(&ham, boyut_tavani.saturating_add(64))?;
    ayikla_ve_dogrula(oid, &cozulmus, boyut_tavani)
}

/// Ham (inflate edilmiş) nesne baytlarını başlığa ayırır ve SHA-1'ini doğrular.
///
/// Ayrı bir fonksiyon olarak tutulur; hem gevşek nesne hem de "zaten çözülmüş" paket
/// nesnesi aynı doğrulamadan geçsin diye.
pub fn ayikla_ve_dogrula(oid: &Oid, cozulmus: &[u8], boyut_tavani: u64) -> Result<Nesne, Hata> {
    let baslik = baslik_ayikla(cozulmus).map_err(|hata| Hata::NesneBozuk {
        oid: oid.onaltilik(),
        ayrinti: hata.to_string(),
    })?;

    if baslik.boyut > boyut_tavani {
        return Err(Hata::CozmeSiniriAsildi {
            bayt: baslik.boyut,
            tav: boyut_tavani,
        });
    }

    let yol = &cozulmus[baslik.yol_basi..];
    if yol.len() as u64 != baslik.boyut {
        return Err(Hata::NesneBozuk {
            oid: oid.onaltilik(),
            ayrinti: format!(
                "başlık {} bayt diyor, yolda {} bayt var",
                baslik.boyut,
                yol.len()
            ),
        });
    }

    let nesne = Nesne::yeni(baslik.tur, yol.to_vec());
    let hesaplanan = crate::sha1::sha1(&nesne.ham_icerik());
    if hesaplanan != *oid.baytlar() {
        return Err(Hata::NesneBozuk {
            oid: oid.onaltilik(),
            ayrinti: format!(
                "içerik özeti uyuşmuyor: dosya adı {}, hesaplanan {}",
                oid.onaltilik(),
                crate::oid::Oid::baytlardan(hesaplanan).onaltilik()
            ),
        });
    }
    Ok(nesne)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn sikistir(veri: &[u8]) -> Vec<u8> {
        let mut kodlayici = ZlibEncoder::new(Vec::new(), Compression::new(6));
        kodlayici.write_all(veri).expect("sıkıştırma yazmalı");
        kodlayici.finish().expect("sıkıştırma bitmeli")
    }

    fn nesne_yaz(objects_dir: &Path, tur: &str, yol: &[u8]) -> Oid {
        let ham = format!("{tur} {}\0", yol.len());
        let mut icerik = ham.into_bytes();
        icerik.extend_from_slice(yol);
        let oid = Oid::baytlardan(crate::sha1::sha1(&icerik));
        let dizi = objects_dir.join(&oid.onaltilik()[..2]);
        fs::create_dir_all(&dizi).expect("nesne dizini oluşturulmalı");
        fs::write(dizi.join(&oid.onaltilik()[2..]), sikistir(&icerik)).expect("nesne yazılmalı");
        oid
    }

    fn gecici(etiket: &str) -> PathBuf {
        let yol =
            std::env::temp_dir().join(format!("gitatlas-loose-{etiket}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&yol);
        fs::create_dir_all(&yol).expect("geçici dizin oluşturulmalı");
        yol
    }

    #[test]
    fn nesne_yolu_bolumlu_dogru_uretilir() {
        let oid = Oid::ayikla("0123456789abcdef0123456789abcdef01234567").expect("geçerli ad");
        let yol = nesne_yolu(Path::new("/tmp/objects"), &oid).expect("yol üretilmeli");
        let son = yol
            .file_name()
            .expect("dosya adı olmalı")
            .to_string_lossy()
            .to_string();
        assert_eq!(yol.parent().expect("üst dizin").file_name().unwrap(), "01");
        assert_eq!(son, "23456789abcdef0123456789abcdef01234567");
    }

    #[test]
    fn gevsek_blob_okunur_ve_dogrulanir() {
        let dizin = gecici("blob");
        let oid = nesne_yaz(&dizin, "blob", b"merhaba");
        let nesne = oku(&dizin, &oid, 1 << 20).expect("nesne okunmalı");
        assert_eq!(nesne.veri, b"merhaba");
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn var_mi_dogru_bildirir() {
        let dizin = gecici("varmi");
        let oid = nesne_yaz(&dizin, "blob", b"x");
        assert!(var_mi(&dizin, &oid));
        let yok = Oid::ayikla("0000000000000000000000000000000000000001").expect("geçerli ad");
        assert!(!var_mi(&dizin, &yok));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn sha1_uyusmazligi_tespit_edilir() {
        let dizin = gecici("uyusmazlik");
        let oid = nesne_yaz(&dizin, "blob", b"gercek icerik");
        // Aynı yola aynı uzunlukta ama farklı bir içerik yaz: ad artık içerikle uyuşmuyor.
        let sahte = b"blob 12\0sahte icerik";
        fs::write(nesne_yolu(&dizin, &oid).expect("yol"), sikistir(sahte))
            .expect("bozuk nesne yazılmalı");
        let hata = oku(&dizin, &oid, 1 << 20).expect_err("SHA-1 uyuşmazlığı hata vermeli");
        assert!(matches!(hata, Hata::NesneBozuk { .. }));
        assert!(hata.to_string().contains("özet"));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn bozuk_zlib_hata_verir() {
        let dizin = gecici("bozukzlib");
        let oid = nesne_yaz(&dizin, "blob", b"x");
        let yol = nesne_yolu(&dizin, &oid).expect("yol");
        let veri = fs::read(&yol).expect("okunmalı");
        let mut bozuk = veri.clone();
        bozuk[2] = 0xFF;
        bozuk[3] = 0xFF;
        fs::write(&yol, &bozuk).expect("bozuk veri yazılmalı");
        let hata = oku(&dizin, &oid, 1 << 20).expect_err("bozuk zlib hata vermeli");
        assert!(
            matches!(hata, Hata::ZlibHatasi { .. } | Hata::NesneBozuk { .. }),
            "beklenmeyen hata: {hata}"
        );
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn kesik_zlib_hata_verir() {
        let dizin = gecici("kesikzlib");
        let oid = nesne_yaz(&dizin, "blob", &[b'x'; 400]);
        let yol = nesne_yolu(&dizin, &oid).expect("yol");
        let veri = fs::read(&yol).expect("okunmalı");
        fs::write(&yol, &veri[..veri.len() / 2]).expect("kesik veri yazılmalı");
        assert!(oku(&dizin, &oid, 1 << 20).is_err());
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn boyut_tavani_aser() {
        let dizin = gecici("tavan");
        let oid = nesne_yaz(&dizin, "blob", &[b'y'; 128]);
        let hata = oku(&dizin, &oid, 16).expect_err("tavan aşılmalı");
        assert!(matches!(hata, Hata::CozmeSiniriAsildi { .. }));
        let _ = fs::remove_dir_all(&dizin);
    }

    #[test]
    fn baslik_boyutu_yolla_uyusmazsa_hata_verir() {
        let oid = Oid::ayikla("1111111111111111111111111111111111111111").expect("geçerli ad");
        let hata = ayikla_ve_dogrula(&oid, b"blob 99\0kisa", 1 << 20)
            .expect_err("boyut uyuşmazlığı hata vermeli");
        assert!(matches!(hata, Hata::NesneBozuk { .. }));
    }

    #[test]
    fn bos_blob_okunur() {
        let dizin = gecici("bosblob");
        let oid = nesne_yaz(&dizin, "blob", b"");
        let nesne = oku(&dizin, &oid, 1 << 20).expect("boş blob okunmalı");
        assert!(nesne.veri.is_empty());
        let _ = fs::remove_dir_all(&dizin);
    }
}
