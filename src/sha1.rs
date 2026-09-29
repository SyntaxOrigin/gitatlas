//! SHA-1 (FIPS 180-4) gerçeklemesi.
//!
//! Git nesne adı, nesnenin `<tür> <uzunluk>\0<payload>` biçimindeki baytlarının SHA-1
//! değeridir; bu yüzden depoyu okumak nesne kimliğini **doğrulamak** anlamına gelir.
//!
//! Kapsam: yalnızca mesaj özeti üretimi. Anahtar türetme, imza veya şifreleme amacıyla
//! kullanılmaz — Git'in nesne kimliği kriptografik kimlik değil, bir adrestir.
//!
//! Test vektörleri FIPS 180-2'den ve RFC 3174'ten alınmıştır; bkz. `tests/birim.rs`.

/// Onaltılık dize üretiminde kullanılan sabit karakter tablosu.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// SHA-1 özet üreten artımlı hesaplayıcı.
#[derive(Clone)]
pub struct Sha1 {
    durum: [u32; 5],
    islenen: u64,
    tampon: [u8; 64],
    tampon_uzunluk: usize,
}

impl Default for Sha1 {
    fn default() -> Self {
        Sha1::yeni()
    }
}

impl Sha1 {
    /// FIPS 180-4 başlangıç değerleriyle yeni bir hesaplayıcı üretir.
    pub fn yeni() -> Self {
        Sha1 {
            durum: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            islenen: 0,
            tampon: [0; 64],
            tampon_uzunluk: 0,
        }
    }

    /// Girdiye bayt ekler; art arda çağrılabilir.
    pub fn ekle(&mut self, veri: &[u8]) {
        self.islenen = self.islenen.wrapping_add(veri.len() as u64);

        if self.tampon_uzunluk > 0 {
            let alinan = std::cmp::min(64 - self.tampon_uzunluk, veri.len());
            self.tampon[self.tampon_uzunluk..self.tampon_uzunluk + alinan]
                .copy_from_slice(&veri[..alinan]);
            self.tampon_uzunluk += alinan;
            if self.tampon_uzunluk < 64 {
                return;
            }
            let blok = self.tampon;
            self.blok_isle(&blok);
            self.tampon_uzunluk = 0;
        }

        let tam_blok = veri.len() / 64;
        for i in 0..tam_blok {
            let mut blok = [0u8; 64];
            blok.copy_from_slice(&veri[i * 64..i * 64 + 64]);
            self.blok_isle(&blok);
        }

        let artan = &veri[tam_blok * 64..];
        if !artan.is_empty() {
            self.tampon[..artan.len()].copy_from_slice(artan);
            self.tampon_uzunluk = artan.len();
        }
    }

    /// Özeti 20 bayt olarak döndürür.
    pub fn bitir(self) -> [u8; 20] {
        let bit_uzunluk = self.islenen.wrapping_mul(8);
        let mut durum = self.durum;
        self.dolgu_ekle(bit_uzunluk, &mut durum);

        let mut ozet = [0u8; 20];
        for (i, parca) in durum.iter().enumerate() {
            ozet[i * 4..i * 4 + 4].copy_from_slice(&parca.to_be_bytes());
        }
        ozet
    }

    /// Özeti onaltılık küçük harf dize olarak döndürür.
    pub fn onaltilik(self) -> String {
        let ozet = self.bitir();
        let mut s = String::with_capacity(40);
        for bayt in ozet {
            s.push(HEX[(bayt >> 4) as usize] as char);
            s.push(HEX[(bayt & 0x0f) as usize] as char);
        }
        s
    }

    /// 0x80 dolgu baytını ve 64 bitlik bit uzunluğunu ekleyerek son bloğu işler.
    fn dolgu_ekle(&self, bit_uzunluk: u64, durum: &mut [u32; 5]) {
        let mut tampon = self.tampon;
        let mut konum = self.tampon_uzunluk;

        tampon[konum] = 0x80;
        konum += 1;

        if konum > 56 {
            while konum < 64 {
                tampon[konum] = 0;
                konum += 1;
            }
            blok_isle_ile(durum, &tampon);
            konum = 0;
        }
        while konum < 56 {
            tampon[konum] = 0;
            konum += 1;
        }
        tampon[56..64].copy_from_slice(&bit_uzunluk.to_be_bytes());
        blok_isle_ile(durum, &tampon);
    }

    fn blok_isle(&mut self, blok: &[u8; 64]) {
        blok_isle_ile(&mut self.durum, blok);
    }
}

/// Tek bir 64 baytlık bloğu durum üzerinde işler (FIPS 180-4 §6.1.2).
fn blok_isle_ile(durum: &mut [u32; 5], blok: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (i, kelime) in w.iter_mut().take(16).enumerate() {
        *kelime = u32::from_be_bytes([
            blok[i * 4],
            blok[i * 4 + 1],
            blok[i * 4 + 2],
            blok[i * 4 + 3],
        ]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let mut a = durum[0];
    let mut b = durum[1];
    let mut c = durum[2];
    let mut d = durum[3];
    let mut e = durum[4];

    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let gecici = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = gecici;
    }

    durum[0] = durum[0].wrapping_add(a);
    durum[1] = durum[1].wrapping_add(b);
    durum[2] = durum[2].wrapping_add(c);
    durum[3] = durum[3].wrapping_add(d);
    durum[4] = durum[4].wrapping_add(e);
}

/// Tek seferlik SHA-1 özeti üretir.
pub fn sha1(veri: &[u8]) -> [u8; 20] {
    let mut h = Sha1::yeni();
    h.ekle(veri);
    h.bitir()
}

/// Tek seferlik SHA-1 özetini onaltılık dize olarak üretir.
pub fn sha1_onaltilik(veri: &[u8]) -> String {
    let mut h = Sha1::yeni();
    h.ekle(veri);
    h.onaltilik()
}
