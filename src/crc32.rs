//! CRC-32 (IEEE 802.3, ISO 3309) hesabı.
//!
//! Git'in `.idx` v2 dosyası her nesne için sıkıştırılmış baytlarının CRC-32 değerini
//! saklar. Bu değer, okunan baytların gerçekten o nesneye ait olduğunu doğrulamak için
//! kullanılır. Git'in kendi testi de aynı amacı taşır (`index-pack --verify`).
//!
//! Kapsam: yalnızca bütünlük sağlaması; güvenlik amaçlı kullanılmaz.

const POLINOM: u32 = 0xEDB8_8320;

/// `veri` baytlarının CRC-32 değerini döndürür.
pub fn crc32(veri: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for bayt in veri {
        crc ^= u32::from(*bayt);
        for _ in 0..8 {
            // En düşük bit 1 ise bir kaydırma ve polinom bölmesi yapılır.
            let maske = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (POLINOM & maske);
        }
    }
    !crc
}
