//! Nesne erişim birleştiricisi: `sha1 → nesne` sorgusunun tek giri noktası.
//!
//! Bu modül MVP'nin merkezî iç yüzeyidir. Bir nesne adı için şu kaynaklar sırayla
//! denenir:
//!
//! 1. bayt cinsinden sınırlı önbellek,
//! 2. `objects/pack/*.idx` içindeki pack dosyaları,
//! 3. `objects/XX/YYYY…` gevşek nesne dosyası.
//!
//! **Sıralama kuralı ve gerekçesi.** Varsayılan sıra `PackOnce`'tir: git'in kendi
//! `do_oid_object_info_extended` uygulaması önce pack, sonra gevşek nesne arar. Üçüncü
//! taraf okuyucuların çoğu ters sırayı kullanır. İki sıra da `AramaSirasi` ile
//! seçilebilir; fark yalnızca **aynı nesnenin her iki yerde de bulunduğu** durumda
//! görünür olur, o da pratikte yalnızca bozuk bir kopyanın varlığıyla ilgilidir.
//!
//! Delta tabanı her iki yerden gelebilir: `OBJ_REF_DELTA` çözümü `Magaza`'ya geri döner,
//! böylece bir ref-delta tabanı pack'te, kendisi gevşek olarak saklanabilir.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::ayarlar::{AramaSirasi, Ayarlar};
use crate::gevsek;
use crate::hata::Hata;
use crate::nesne::Nesne;
use crate::oid::Oid;
use crate::onbellek::Onbellek;
use crate::paket::PaketDosyasi;

/// Bir depodaki tüm nesne kaynaklarını birleştiren okuyucu.
#[derive(Debug)]
pub struct Magaza {
    objects_dir: PathBuf,
    paketler: Vec<PaketDosyasi>,
    onbellek: Onbellek,
    ayar: Ayarlar,
    cozum_yigini: RefCell<Vec<Oid>>,
    sayaclar: RefCell<Sayaclar>,
}

/// Okuma sırasında tutulan sayaçlar (JSON çıktısında raporlanır).
#[derive(Clone, Copy, Default, Debug)]
pub struct Sayaclar {
    /// Önbellekten karşılanan istek sayısı.
    pub onbellek_isi: u64,
    /// Gevşek nesneden karşılanan istek sayısı.
    pub gevsek_isi: u64,
    /// Pack'ten karşılanan istek sayısı.
    pub paket_isi: u64,
    /// SHA-1 doğrulamasından geçen nesne sayısı.
    pub dogrulanan: u64,
}

/// Bir nesne kaynağından (`pack` ya da `gevşek`) çözme yapan iç fonksiyon imzası.
type Cozucu = fn(&Magaza, &Oid, &Ayarlar, u32) -> Result<Nesne, Hata>;

impl Magaza {
    /// `git_dir` içindeki `objects` dizinini açar, tüm pack çiftlerini yükler.
    ///
    /// Pack dizini yoksa hata verilmez: yalnızca gevşek nesnelerle çalışan bir depo
    /// geçerli bir depodur (ör. `git gc` çalıştırılmamış yeni depo).
    pub fn ac(git_dir: &Path, ayar: Ayarlar) -> Result<Self, Hata> {
        let objects_dir = git_dir.join("objects");
        let mut paketler: Vec<PaketDosyasi> = Vec::new();
        let pack_dir = objects_dir.join("pack");

        if pack_dir.is_dir() {
            let mut girdiler: Vec<PathBuf> = Vec::new();
            let okuma = std::fs::read_dir(&pack_dir).map_err(|hata| Hata::io(&pack_dir, hata))?;
            for giris in okuma {
                let giris = giris.map_err(|hata| Hata::io(&pack_dir, hata))?;
                let yol = giris.path();
                if yol.extension().and_then(|e| e.to_str()) == Some("idx") {
                    girdiler.push(yol);
                }
            }
            // Birden çok pack varsa sıra deterministiktir: ada göre artan.
            girdiler.sort();
            for idx_yolu in girdiler {
                paketler.push(PaketDosyasi::ac(&idx_yolu)?);
            }
        }

        Ok(Magaza {
            objects_dir,
            paketler,
            onbellek: Onbellek::yeni(ayar.onbellek_bayt),
            ayar,
            cozum_yigini: RefCell::new(Vec::new()),
            sayaclar: RefCell::new(Sayaclar::default()),
        })
    }

    /// Uygulanan ayarların kopyasını döndürür.
    pub fn ayar(&self) -> &Ayarlar {
        &self.ayar
    }

    /// Yüklenen pack dosyalarının sayısını döndürür.
    pub fn paket_sayisi(&self) -> usize {
        self.paketler.len()
    }

    /// Yüklenen pack dosyalarının toplam nesne sayısını döndürür.
    pub fn paket_nesne_sayisi(&self) -> usize {
        self.paketler.iter().map(PaketDosyasi::nesne_sayisi).sum()
    }

    /// Pack dosyalarının özetlerini döndürür (`ozet` çıktısında listelenir).
    pub fn paket_ozetleri(&self) -> Vec<serde_json::Value> {
        self.paketler
            .iter()
            .map(|p| {
                serde_json::json!({
                    "pack": p.paket_yolu().display().to_string(),
                    "nesne_sayisi": p.nesne_sayisi(),
                    "dosya_boyutu": p.dosya_boyutu(),
                    "paket_ozeti": Oid::baytlardan(p.ozet()).onaltilik(),
                    "tur_dagilimi": p.tur_sayilari_json(),
                })
            })
            .collect()
    }

    /// `oid` nesnesini bulur, açar, doğrular ve döndürür.
    pub fn nesne(&self, oid: &Oid) -> Result<Nesne, Hata> {
        self.coz(oid, &self.ayar, 0)
    }

    /// `oid` nesnesini belirtilen ayarlarla çözer.
    pub fn coz(&self, oid: &Oid, ayar: &Ayarlar, derinlik: u32) -> Result<Nesne, Hata> {
        if let Some(veri) = self.onbellek.al(oid) {
            let baslik = crate::nesne::baslik_ayikla(&veri).map_err(|hata| Hata::NesneBozuk {
                oid: oid.onaltilik(),
                ayrinti: hata.to_string(),
            })?;
            self.sayaclar.borrow_mut().onbellek_isi += 1;
            return Ok(Nesne::yeni(baslik.tur, veri[baslik.yol_basi..].to_vec()));
        }

        let nesne = self.coz_ten_cisim(oid, ayar, derinlik)?;
        let ham = nesne.ham_icerik();
        let hesaplanan = crate::sha1::sha1(&ham);
        if hesaplanan != *oid.baytlar() {
            return Err(Hata::NesneBozuk {
                oid: oid.onaltilik(),
                ayrinti: format!(
                    "içerik özeti uyuşmuyor: beklenen {}, hesaplanan {}",
                    oid.onaltilik(),
                    Oid::baytlardan(hesaplanan).onaltilik()
                ),
            });
        }
        self.sayaclar.borrow_mut().dogrulanan += 1;
        self.onbellek.ekle(*oid, ham);
        Ok(nesne)
    }

    fn coz_ten_cisim(&self, oid: &Oid, ayar: &Ayarlar, derinlik: u32) -> Result<Nesne, Hata> {
        let sira: [Cozucu; 2] = match ayar.arama_sirasi {
            AramaSirasi::PackOnce => [Magaza::pakete_bak, Magaza::gevsege_bak],
            AramaSirasi::GevsekOnce => [Magaza::gevsege_bak, Magaza::pakete_bak],
        };

        match sira[0](self, oid, ayar, derinlik) {
            Ok(nesne) => Ok(nesne),
            // Yalnızca "bulunamadı" durumunda ikinci kaynağa geçilir. Bir kaynakta
            // bulunan ama BOZUK nesne sessizce atlanmaz: bozukluk, ikinci bir kopyayla
            // örtüşerek gizlenmemelidir.
            Err(Hata::NesneBulunamadi { .. }) => sira[1](self, oid, ayar, derinlik),
            Err(hata) => Err(hata),
        }
    }

    fn gevsege_bak(&self, oid: &Oid, ayar: &Ayarlar, _derinlik: u32) -> Result<Nesne, Hata> {
        if !gevsek::var_mi(&self.objects_dir, oid) {
            return Err(Hata::NesneBulunamadi {
                oid: oid.onaltilik(),
            });
        }
        self.sayaclar.borrow_mut().gevsek_isi += 1;
        gevsek::oku(&self.objects_dir, oid, ayar.nesne_tavani)
    }

    fn pakete_bak(&self, oid: &Oid, ayar: &Ayarlar, _derinlik: u32) -> Result<Nesne, Hata> {
        for paket in &self.paketler {
            if paket.ofset_bul(oid).is_some() {
                self.sayaclar.borrow_mut().paket_isi += 1;
                let (tur, veri) = paket.nesne(self, oid, ayar)?;
                return Ok(Nesne::yeni(tur, veri));
            }
        }
        Err(Hata::NesneBulunamadi {
            oid: oid.onaltilik(),
        })
    }

    /// Tüm pack dosyalarının CRC-32 ve SHA-1 bütünlüğünü doğrular.
    ///
    /// Her nesne için: pack'teki sıkıştırılmış baytlar `.idx` CRC değeriyle, çözülmüş
    /// içerik ise nesne adıyla karşılaştırılır. Bu, "okuduğumuz her şey gerçek" iddiasının
    /// mekanik kanıtıdır; `ozet --butunluk` ile çağrılır.
    pub fn paketleri_dogrula(&self) -> Result<DogurmaRaporu, Hata> {
        let mut dogrulanan = 0usize;
        for paket in &self.paketler {
            paket.butunluk_denetle()?;
            for oid in paket.nesne_adlari().collect::<Vec<_>>() {
                paket.crc_dogrula(&oid, &self.ayar)?;
                self.coz(&oid, &self.ayar, 0)?;
                dogrulanan += 1;
            }
        }
        Ok(DogurmaRaporu {
            paket_sayisi: self.paketler.len(),
            dogrulanan_nesne: dogrulanan,
        })
    }

    /// Bir ref-delta tabanını çözerken döngü koruması uygular.
    ///
    /// Çağıran, dönen kilidi elde tuttuğu sürece nesne adı "şu an çözülüyor" sayılır.
    /// Aynı ad yeniden istenirse [`Hata::DeltaDongusu`] üretilir; sonsuz özyineleme
    /// tek bir hata mesajıyla kesilir.
    pub(crate) fn cozum_kilidi(&self, oid: &Oid) -> Result<DeltaKilidi<'_>, Hata> {
        let mut yigin = self.cozum_yigini.borrow_mut();
        if yigin.contains(oid) {
            return Err(Hata::DeltaDongusu {
                oid: oid.onaltilik(),
            });
        }
        yigin.push(*oid);
        Ok(DeltaKilidi { magaza: self })
    }

    /// Okuma sayaçlarının kopyasını döndürür.
    pub fn sayaclar(&self) -> Sayaclar {
        *self.sayaclar.borrow()
    }
}

/// Ref-delta çözüm yığınına eklenen adı bırakıldığında yığından çıkaran kilit.
pub(crate) struct DeltaKilidi<'a> {
    magaza: &'a Magaza,
}

impl Drop for DeltaKilidi<'_> {
    fn drop(&mut self) {
        self.magaza.cozum_yigini.borrow_mut().pop();
    }
}

/// Pack bütünlük denetiminin sonucu.
#[derive(Clone, Copy, Debug)]
pub struct DogurmaRaporu {
    /// Denetlenen pack dosyası sayısı.
    pub paket_sayisi: usize,
    /// Denetlenen nesne sayısı.
    pub dogrulanan_nesne: usize,
}
