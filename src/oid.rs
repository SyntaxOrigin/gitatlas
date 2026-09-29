//! Git nesne adı (object id): 20 baytlık SHA-1 değeri.
//!
//! Kapsam: adın tutulması, onaltılık dönüşümü ve ayrıştırma. Depo okuma mantığı bu
//! modülde değildir.

use std::fmt;

use crate::hata::Hata;

/// 20 baytlık nesne adı. `Copy` ve `Ord` uygulanır; Git de nesne adlarını sözlük sırasıyla
/// karşılaştırdığı için aynı sıralama burada da geçerlidir.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Oid([u8; 20]);

/// Onaltılık dize üretiminde kullanılan sabit karakter tablosu.
const HEX: &[u8; 16] = b"0123456789abcdef";

impl Oid {
    /// 20 bayttan nesne adı üretir.
    pub fn baytlardan(baytlar: [u8; 20]) -> Self {
        Oid(baytlar)
    }

    /// 20 bayta işaretçi olarak erişir.
    pub fn baytlar(&self) -> &[u8; 20] {
        &self.0
    }

    /// Nesne adını 40 karakterlik onaltılık dizeye çevirir.
    pub fn onaltilik(&self) -> String {
        let mut s = String::with_capacity(40);
        for bayt in self.0.iter() {
            s.push(HEX[(bayt >> 4) as usize] as char);
            s.push(HEX[(bayt & 0x0f) as usize] as char);
        }
        s
    }

    /// 40 onaltılık karakterlik dizgeden nesne adı üretir.
    ///
    /// Büyük harf kabul edilir. Başka uzunluk veya geçersiz karakter hata verir.
    pub fn ayikla(dize: &str) -> Result<Self, Hata> {
        if dize.len() != 40 {
            return Err(Hata::RevCozulemedi {
                rev: dize.to_string(),
            });
        }
        let baytlar = dize.as_bytes();
        let mut sonuc = [0u8; 20];
        for (i, bayt) in baytlar.chunks(2).enumerate() {
            let y = hece(bayt[0], dize)?;
            let d = hece(bayt[1], dize)?;
            sonuc[i] = (y << 4) | d;
        }
        Ok(Oid(sonuc))
    }

    /// 40 onaltılık karakterlik olup olmadığını bildirir (ayrıştırma yapmadan).
    pub fn gecerli_mi(dize: &str) -> bool {
        dize.len() == 40 && dize.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

fn hece(bayt: u8, kaynak: &str) -> Result<u8, Hata> {
    match bayt {
        b'0'..=b'9' => Ok(bayt - b'0'),
        b'a'..=b'f' => Ok(bayt - b'a' + 10),
        b'A'..=b'F' => Ok(bayt - b'A' + 10),
        _ => Err(Hata::RevCozulemedi {
            rev: kaynak.to_string(),
        }),
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.onaltilik())
    }
}
