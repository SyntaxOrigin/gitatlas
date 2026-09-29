//! GitAtlas kütüphanesi: git deposunu **klonlamadan, salt okunur** okuyan çekirdek.
//!
//! Bu crate hiçbir koşulda depo klasörüne yazmaz. Yazma yeteneği [`paket::yazici`]
//! modülünde yalnızca **test fikstürü üretmek** için vardır ve çalışma zamanı yolundan
//! hiç çağrılmaz; `refs` yazma ve `index-pack` bilinçli olarak uygulanmamıştır.
//!
//! Katmanlar (aşağıdan yukarıya):
//!
//! - [`sha1`], [`crc32`], [`oid`], [`zlib`]: belirtim düzeyi primitifler.
//! - [`nesne`], [`onbellek`], [`ayarlar`]: biçim tanımı, bellek disiplini, okuma politikası.
//! - [`gevsek`], [`paket`]: iki nesne kaynağı — gevşek dosyalar ve `.pack` + `.idx`.
//! - [`magaza`]: nesne erişim birleştiricisi (`sha1 → nesne` tek giri noktası).
//! - [`depo`], [`commit`], [`agac`], [`gecmis`], [`fark`]: depo anlamı katmanı.
//! - [`cikti`]: JSON ve Markdown çıktı üretimi.
//!
//! Git'in nesne ve pack biçimi `gitformat-pack(5)` ve `gitrepository-layout(5)` ile
//! tanımlıdır; sıkıştırma tarafı RFC 1950/1951'dir.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
// Sözleşme § 4.2: `unwrap`/`expect` **üretim kodunda** yasaktır, testlerde gerekçeyle
// kullanılabilir. Bu yüzden uyarı yalnızca `cfg(test)` dışında etkindir; test
// modüllerindeki `expect(...)` çağrıları kasıtlı ve gerekçelidir.
#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod agac;
pub mod ayarlar;
pub mod cikti;
pub mod commit;
pub mod crc32;
pub mod depo;
pub mod fark;
pub mod gecmis;
pub mod gevsek;
pub mod hata;
pub mod magaza;
pub mod nesne;
pub mod oid;
pub mod onbellek;
pub mod paket;
pub mod sha1;
pub mod zlib;

pub use ayarlar::{AramaSirasi, Ayarlar};
pub use depo::Depo;
pub use hata::Hata;
pub use magaza::Magaza;
pub use oid::Oid;

/// Kütüphanenin sürümü (`Cargo.toml`'daki değerle aynıdır).
pub const SURUM: &str = env!("CARGO_PKG_VERSION");

/// Araç adı.
pub const ARAC_ADI: &str = "GitAtlas";
