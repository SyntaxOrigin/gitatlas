# GitAtlas (DepoHaritası)

`git` kurulu **olmayan** bir makinede, yalnızca `.git` dizinini okuyup bir deponun
hikâyesini çıkaran, **salt okunur** terminal aracı.

GitAtlas klonlama yapmaz, ağa çıkmaz, kimlik doğrulamaz ve depoya hiçbir koşulda
yazmaz. Paketlenmiş (pack) depoları, delta zincirlerini ve gevşek nesneleri kendi
ayrıştırıcısıyla okur; her nesnenin SHA-1'ini **doğrular**.

```console
$ gitatlas ozet /path/to/repo --butunluk
$ gitatlas gecmis /path/to/repo --yol src/lib.rs
$ gitatlas diff /path/to/repo HEAD~1 HEAD --yol src/lib.rs
```

---

## Özellikler

MVP kapsamı `MANIFEST.md` kart 29 ve karar **D-009** (pack desteği MVP'ye alındı)
ile birebir uygulanmıştır.

1. **Salt okunur depo açma.** `.git/HEAD` sembolik (dal) ya da ayrışık (detached)
   olarak okunur; `refs/heads/**` ve `refs/tags/**` özyinelemeli taranır;
   `packed-refs` okunur ve `^` ile başlayan **peeled** (işaretli etiket) satırları
   ayrıştırılır. Depoya hiçbir yazma yapılmaz; kanıt aşağıda "Test" bölümündedir.
2. **Gevşek (loose) nesne okuma.** `objects/XX/YYYY…` yolu üretilir, zlib akışı
   inflate edilir, `<tür> <uzunluk>\0<payload>` başlığı ayrıştırılır ve içeriğin
   SHA-1'i nesne adıyla karşılaştırılır. Uyuşmazlık sessizce geçmez, hata üretir.
3. **Pack indeksi v2.** `.idx` sihirli değeri, sürüm alanı, 256 girişlik fanout
   tablosu, `sha1 → offset` ve `offset → crc32` tabloları okunur; büyük offset
   tablosu (MSB set kayıtlar) desteklenir. **Sürüm 1 bilinçli olarak reddedilir.**
4. **Pack nesne çözümleme.** Düz nesne, `OBJ_OFS_DELTA` (taban ofsetle) ve
   `OBJ_REF_DELTA` (20 baytlık taban adıyla) yollarının üçü de uygulanır. Delta
   başlığı (kaynak/hedef boyutu) ve iki komut türü (ekle, kopyala) ayrıştırılır.
   **Zincir derinliği tavanı** (varsayılan 64) ve **döngü koruması** vardır.
5. **Nesne erişim birleştiricisi.** `sha1 → nesne` sorgusu tek giri noktasıdır;
   loose ve pack birleştirilir, delta tabanı her iki yerden gelebilir.
6. **Commit ağacı ve yol çözümleme.** `tree` girdileri (`100644`, `100755`, `120000`,
   `40000`, `160000`) ayrıştırılır; ara ağaçlar yönlendirilir. `..`, `.`, mutlak yol,
   `//`, geri eğik çizgi, kontrol karakteri ve `.git` bileşeni **reddedilir**.
7. **`git log` benzeri geçmiş.** HEAD'den başlayan yürüyüş committer zamanına göre
   yeniden eskiye sıralanır (git'in `--date-order` davranışı; eşit zamanda keşif
   sırası korunur, yani çıktı deterministiktir). `--yol <dosya>` filtresi uygulanır.
8. **Diff.** İki ağaç özyinelemeli karşılaştırılır; iki tarafta da bulunan blob'lar
   arasında **Myers'in O(ND) en kısa düzenleme yolu** algoritmasıyle satır bazlı
   unified diff üretilir. İkili (NUL içeren) dosyalar işaretlenir.
9. **Boru hattına uygun çıktı.** `ozet`, `gecmis`, `diff` alt komutları; her biri
   `--format json`, `--format md` ve (diff'te) `--format duz` destekler.

Ek güvence katmanları:

- **SHA-1 doğrulaması** her nesnede zorunludur; `ozet --butunluk` ayrıca her pack
  nesnesinin `.idx` CRC-32 değerini ve pack dosyasının bütünlük özetini doğrular.
- **Gizlilik:** commit yazarlarının e-posta adresleri hiçbir çıktıda yer almaz;
  yalnızca kayıttaki ad gösterilir.
- **Bellek:** nesne önbelleği bayt cinsinden sınırlıdır (varsayılan 32 MB) ve
  sınırsız büyüyemez; tek nesne çözme tavanı 64 MB'dır.

---

## Kurulum

### Gereksinimler

- **Rust 1.74 veya üstü** (MSRV). Geliştirme ortamında `rustc 1.98.1` ile derlendi.
- **`git` kurulu olmak GEREKMEZ.** GitAtlas çalışma zamanında git'e hiçbir şekilde
  bağımlı değildir ve git çağırmaz. (Testlerde `git` yalnızca *fikstür üreticisi*
  olarak kullanılır; bkz. `tests/yardimci/mod.rs`.)

### Derleme

```console
$ cd projects/29-gitatlas
$ cargo build --release
   Compiling gitatlas v0.1.0 (%USERPROFILE%\Desktop\Projeler\projects\29-gitatlas)
    Finished `release` profile [optimized] target(s) in 10.01s
```

### Kurulum (cargo install)

```console
$ cargo install --path .
  Finished `release` profile [optimized] target(s) in 2.08s
  Installing %USERPROFILE%\.cargo\bin\gitatlas.exe
   Installed package `gitatlas v0.1.0 (%USERPROFILE%\Desktop\Projeler\projects\29-gitatlas)` (executable `gitatlas.exe`)
$ gitatlas --version
gitatlas 0.1.0
```

### Çalıştırma

```console
$ gitatlas --help
GitAtlas, yalnızca `.git` dizinini okuyarak deponun özetini, geçmişini ve dosya farklarını üretir. Hedef makinede `git` kurulu olması gerekmez; depo hiçbir koşulda yazılmaz.

Usage: gitatlas.exe <COMMAND>

Commands:
  ozet    Deponun genel özetini üretir: HEAD, commit sayısı, yazar dağılımı, etiketler, pack'ler
  gecmis  `git log` benzeri geçmiş listesi
  diff    İki revizyon arasındaki ağaç ve satır farkı
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

---

## Kullanım

Aşağıdaki bütün komutlar ve çıktılar **gerçekten çalıştırılmıştır**.

### Örnek depo

`%TEMP%\opencode\demo-depo` altında şu içerikle iki commit'lik, `git gc` ile
paketlenmiş bir depo üretildi:

```console
$ git init -b main -q .
$ printf '# Ornek Proje\n\nKisa bir aciklama.\n' > README.md
$ printf 'bir\niki\nuc\ndort\nbes\nalti\nyedi\nsekiz\ndokuz\non\n' > src.txt
$ mkdir src && printf 'fn main() {}\n' > src/main.rs
$ git add -A && git commit -q -m "chore: ilk surum"
$ printf 'bir\niki\nUC\ndort\nbes\nalti\nyedi\nsekiz\ndokuz\non\n' > src.txt
$ printf 'yeni dosya\n' > yeni.txt
$ git add -A && git commit -q -m "fix: src.txt duzeltildi, yeni.txt eklendi"
$ git gc -q
```

### 1. `ozet` — Markdown

```console
$ gitatlas ozet demo-depo --format md
# GitAtlas — Depo Özeti

- **Depo**: `%USERPROFILE%\AppData\Local\Temp\opencode\demo-depo`
- **HEAD**: dal `main` → `e7ba5bf`
- **Commit sayısı**: 2
- **Yazar sayısı**: 1
- **Pack dosyası**: 1 (10 nesne)
- **Erişim**: salt okunur (depo hiçbir yerde yazılmadı)

## Yazarlar

| Yazar | Commit |
|---|---:|
| SyntaxOrigin | 2 |
```

### 2. `gecmis` — Markdown tablosu

```console
$ gitatlas gecmis demo-depo --format md
# GitAtlas — Geçmiş

Başlangıç: `e7ba5bf2350b4131403545819564123cf2223b36` · 2 kayıt

| Commit | Yazar | Zaman (UTC) | Konu | Dosya |
|---|---|---|---|---:|
| `e7ba5bf` | SyntaxOrigin | 2026-01-02T10:00:00Z | fix: src.txt duzeltildi, yeni.txt eklendi | 2 |
| `62af396` | SyntaxOrigin | 2026-01-01T10:00:00Z | chore: ilk surum | 3 |
```

### 3. `gecmis --yol` — dosya filtresi (JSON)

```console
$ gitatlas gecmis demo-depo --yol src.txt --format json
{
  "adet": 2,
  "arac": "gitatlas",
  "baslangic": "e7ba5bf2350b4131403545819564123cf2223b36",
  "kayitlar": [
    {
      "bolge": "+0000",
      "dosya_sayisi": 2,
      "ebeveyn_sayisi": 1,
      "kisa": "e7ba5bf",
      "konu": "fix: src.txt duzeltildi, yeni.txt eklendi",
      "oid": "e7ba5bf2350b4131403545819564123cf2223b36",
      "yazar": "SyntaxOrigin",
      "zaman": 1767348000
    },
    {
      "bolge": "+0000",
      "dosya_sayisi": 3,
      "ebeveyn_sayisi": 0,
      "kisa": "62af396",
      "konu": "chore: ilk surum",
      "oid": "62af3960181b91e3dacb206a9e68b53f98ffdf2e",
      "yazar": "SyntaxOrigin",
      "zaman": 1767261600
    }
  ],
  "komut": "gecmis",
  "surum": "0.1.0",
  "yol_filtresi": "src.txt"
}
```

> `62af396` commit'i `src.txt` dosyasını **ilk kez oluşturduğu** için listede
> görünür; filtre "değiştirdiği" commit'leri arar, "dosyanın var olduğu tüm
> commit'ler"i değil.

### 4. `diff` — unified diff (varsayılan `--format duz`)

```console
$ gitatlas diff demo-depo HEAD~1 HEAD
--- a/62af3960181b91e3dacb206a9e68b53f98ffdf2e
+++ b/e7ba5bf2350b4131403545819564123cf2223b36
diff --git a/src.txt b/src.txt
@@ -1,6 +1,6 @@
 bir
 iki
-uc
+UC
 dort
 bes
 alti
```

Aynı pencere `git diff HEAD~1 HEAD` ile karşılaştırıldığında **hunk başlığı ve
satır içeriği birebir aynıdır**. Git ek olarak `index …` satırı ve **yeni eklenen
dosyanın tam içeriğini** gösterir; GitAtlas yeni/silinen dosyalar için yalnızca
yol durumunu raporlar (bkz. *Bilinen Sınırlamalar*).

### 5. `diff --yol` ile tek dosya (JSON)

```console
$ gitatlas diff demo-depo HEAD~1 HEAD --yol src.txt --format json
{
  "arac": "gitatlas",
  "dosya_sayisi": 1,
  "dosyalar": [
    {
      "durum": "degisti",
      "eski_mod": "100644",
      "eski_oid": "5e8fa1dfb692c40786e8591138fa23aed6912471",
      "isaret": "M",
      "modul": false,
      "satir": {
        "eklenen": 1,
        "hunk_sayisi": 1,
        "hunklar": [
          {
            "eklenen": 1,
            "eski_baslangic": 1,
            "satirlar": [
              { "islem": "esit", "metin": "bir" },
              { "islem": "esit", "metin": "iki" },
              { "islem": "silinen", "metin": "uc" },
              { "islem": "eklenen", "metin": "UC" },
              { "islem": "esit", "metin": "dort" },
              { "islem": "esit", "metin": "bes" },
              { "islem": "esit", "metin": "alti" }
            ],
            "silinen": 1,
            "yeni_baslangic": 1
          }
        ],
        "kaba": false,
        "silinen": 1
      },
      "yeni_mod": "100644",
      "yeni_oid": "bd126694cdea81cbfb3a9bf3902c1872fbd2ab92",
      "yol": "src.txt"
    }
  ],
  "komut": "diff",
  "sag": "e7ba5bf2350b4131403545819564123cf2223b36",
  "sol": "62af3960181b91e3dacb206a9e68b53f98ffdf2e",
  "surum": "0.1.0",
  "yol_filtresi": "src.txt"
}
```

> Yukarıdaki `satirlar` dizisi okunabilirlik için tek satıra toplandı; gerçek
> çıktı her nesneyi iki satırda (`"islem"` ve `"metin"` ayrı satırlarda) basar.
> `kaba: false` alanı, farkın gerçekten **minimal** olduğunu belgeler.

---

## Test

```console
$ cargo test
   Compiling gitatlas v0.1.0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.31s
     Running unittests src\lib.rs (target\debug\deps\gitatlas-….exe)

running 91 tests
test result: ok. 91 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

     Running tests\depo.rs (target\debug\deps\depo-….exe)

running 18 tests
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s

     Running tests\entegrasyon.rs (target\debug\deps\entegrasyon-….exe)

running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.28s

     Running tests\pack.rs (target\debug\deps\pack-….exe)

running 17 tests
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.85s

     Running tests\sema.rs (target\debug\deps\sema-….exe)

running 8 tests
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

     Running unittests src\main.rs (target\debug\deps\gitatlas-….exe)

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests gitatlas

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**test sonucu: okunan 149; geçen 149; başarısız 0** (91 birim + 58 entegrasyon).
Bunların 7'si güvenlik regresyonudur: `delta.rs` içinde devasa `hedef_boyut`
(`u64::MAX`, `2^63`, `2^40`) için tahsis yapmadan hata üreten testler, blok blok
büyümenin birebir doğru sonuç verdiğini kanıtlayan test ve tavan içindeki 4 MiB'lık
meşru deltanın okunduğunu gösteren test; `tests/pack.rs` içinde ise aynı saldırgan
pack'i uçtan uca kuran `devasa_hedef_boyutlu_delta_sureci_dusurmez` ve 8 MiB'lık
meşru deltanın çözüldüğünü gösteren `tavan_icindeki_buyuk_delta_cozulur`.

### Test depoları — nasıl üretiliyor

Test verisi **iki bağımsız kaynaktan** gelir; bu, "yazar ile okuyucu aynı
belirtimden yazıldığı için birbirini doğrulamaz" riskini ortadan kaldırır.

**(1) `git` ile üretilen gerçek depolar** (`tests/entegrasyon.rs`, 15 test).
`git` yalnızca *fikstür üreticisidir*; her komut `git -c user.name=… -c
user.email=…` ile kimlik alır, global git config'e dokunulmaz. Üretilen türler:

| # | Tür | Nasıl üretiliyor | Doğrulama |
|---|---|---|---|
| a | yalnızca gevşek nesne | `git init` + commit (pack yok) | `git rev-parse HEAD:a.txt` |
| b | paketlenmiş | `git gc -q` | tüm blob'lar `git cat-file` ile satır satır |
| c | birden fazla pack | `git repack -a -d` (iki kez) | commit listesi + nesne okuma |
| d | delta içeren | `git gc -q --aggressive` | **`tur_dagilimi` ile en az 1 `ofs-delta` olduğu doğrulanır**, sonra içerik `git cat-file` ile |
| e | bozuk pack | pack imzası/gövdesi bayt çevrilir | `Magaza::ac` hata vermeli |
| f | `packed-refs` | `git pack-refs --all` | peeled etiket hedefi okunmalı |
| g | işaretli etiket | `git tag -a v1.0 -m …` | `v1.0` doğrudan commit'e çözülmeli |
| h | ayrışık HEAD | `git checkout <sha>` | yalnızca o commit görünmeli |
| i | SHA-256 olmayan, çok dosyalı | 8 sürümlü 400 satırlık dosya | `git cat-file` içerik eşitliği |

Ayrıca `butun_pack_nesneleri_git_ile_esit` testi, depodaki **her nesneyi**
`git cat-file --batch-all-objects` listesiyle tür ve içerik olarak birebir
karşılaştırır.

**(2) Kendi sentetik pack yazıcımız** (`src/paket/yazici.rs`).
`git` olmadan da çalışan, deterministik delta zinciri / bozuk pack / CRC
uyuşmazlığı senaryoları kurar. **Bu yazıcı çalışma zamanı yolundan hiç
çağrılmaz** — yalnızca testler kullanır; salt okunur sözleşmesini bozmaz.
Bağımsızlık (1) ile sağlanır.

### Kapsanan senaryolar

- Başlık varyasyonları: `blob 5\0…`, `tree 0\0`, büyük boyut, NUL yok, bilinmeyen tür,
  sayı olmayan boyut, kesik ağaç girdisi, NUL içeren boş blob.
- `refs/`, `packed-refs`, peeled etiket, ayrışık referansın önceliği, ayrışık
  referansın peeled hedefi ezmediği, iç içe dal adları, `alternates` bildirimi.
- HEAD: dal, ayrışık (detached), doğmamış dal, bozuk içerik.
- SHA-1 uyuşmazlığı tespiti (aynı ada yazılmış farklı içerik).
- Bozuk zlib, kesik zlib, boyut tavanı aşımı.
- `.idx` **v1 reddi**, fanout azalanlığı, kesik dosya, indeks özeti uyuşmazlığı,
  sırasız nesne adları.
- Pack: düz nesne, `OBJ_OFS_DELTA`, `OBJ_REF_DELTA`, ref-delta tabanının **gevşek
  nesneden** gelmesi, 5 basamaklı zincir, **derinlik tavanı aşımı**, **döngü
  koruması**, birden çok pack, bozuk PACK imzası, bozuk özet, bozuk gövde, CRC-32
  uyuşmazlığı, tüm pack bütünlük denetimi.
- Delta komutları: ekle, kopyala (ofset baytlarıyla), boyut 0 → 0x10000,
  kaynak dışı kopyalama, sıfır bayt ekleme, eklemenin aktışı aşması, hedef boyut
  uyuşmazlığı, yarıda kesik varint.
- Ağaç: iç içe girdiler, alt modül (gitlink), sembolik bağ modu, yol reddi
  (`..`, `.`, mutlak, `//`, `\`, `.git`, kontrol karakteri, boş).
- `git log` yürüyüşü (zaman sırası, keşif sırası), yol filtresi (`git log --`
  ile birebir aynı sonuç), limit, kök commit karşılaştırması.
- Diff: değişiklik, ekleme, silme, bağlam genişliği, satır taşıma, ikili dosya,
  **unified diff hunk başlığında bağlam satırlarının sayılması**.
- Arama sırası: `pack-once` ve `gevsek-once` **her iki yönde de** test edilir
  (aynı nesnenin bozuk gevşek kopyası ile, sıranın gözle görülür farkı).
- Çıktı şeması: `ozet`/`gecmis`/`diff` alan adları, Markdown biçimi, boş depo,
  UTC dönüşümü (`utc_iso`), e-posta sızmazlığı.
- **Salt okunurluk kanıtı** (aşağıda).

### Salt okunurluk kanıtı

`tests/entegrasyon.rs::depo_hicbir_zaman_yazilmaz` testi, `git gc
--aggressive` çalıştırılmış gerçek bir depoda üç alt komutun **tamamını**
(özet + bütünlük denetimi, geçmiş + yol filtreli geçmiş, ağaç farkı, yol çözümü)
çalıştırır ve depo klasörünün her dosyasının (göreli yol, bayt boyutu, içerik
SHA-1'i) listesini işlemden önce ve sonra karşılaştırır. Liste birebir aynıysa
(ve dosya sayısı değişmediyse) depo **hiçbir bayt değiştirilmemiş** demektir.

---

## Proje Yapısı

```text
projects/29-gitatlas/
├── Cargo.toml
├── Cargo.lock
├── LICENSE.txt                 MIT
├── README.md
├── .gitignore
├── src/
│   ├── main.rs                 CLI kabuğu: ozet / gecmis / diff
│   ├── lib.rs                  modül ağacı ve crate nitelikleri
│   ├── hata.rs                 tek hata tipi + elle Display/Error
│   ├── ayarlar.rs              Ayarlar, AramaSirasi, okuma politikası
│   ├── sha1.rs                 FIPS 180-4 SHA-1
│   ├── crc32.rs                IEEE 802.3 CRC-32
│   ├── oid.rs                  20 baytlık nesne adı
│   ├── zlib.rs                 RFC 1950 inflate sarmalayıcısı
│   ├── nesne.rs                NesneTuru, Nesne, `<tür> <uzunluk>\0` başlığı
│   ├── onbellek.rs             bayt cinsinden sınırlı LRU önbellek
│   ├── gevsek.rs               loose nesne okuma + SHA-1 doğrulama
│   ├── paket/
│   │   ├── mod.rs              .pack okuma, nesne başlığı, çözümleme
│   │   ├── indeks.rs           .idx v2 (fanout, sha1→offset, offset→crc32)
│   │   ├── delta.rs            delta başlığı + ekle/kopyala komutları
│   │   └── yazici.rs           sentetik pack/idx YAZICI (yalnızca testler)
│   ├── magaza.rs               nesne erişim birleştiricisi + sayaçlar
│   ├── depo.rs                 .git keşfi, HEAD, refs, packed-refs
│   ├── commit.rs               commit/etiket kaydı, Kimlik
│   ├── agac.rs                 ağaç girdileri + yol çözümleme
│   ├── gecmis.rs               commit yürüyüşü + yol filtresi
│   ├── fark.rs                 ağaç farkı + Myers satır farkı
│   └── cikti.rs                JSON / Markdown / unified diff
└── tests/
    ├── yardimci/mod.rs          GeciciDizin, git fikstür üreticisi, depo özeti
    ├── entegrasyon.rs          gerçek git depoları + salt okunurluk kanıtı
    ├── pack.rs                 pack/delta/CRC uçtan uca
    ├── depo.rs                 HEAD/refs/packed-refs uçtan uca
    └── sema.rs                 çıktı şeması ve gizlilik
```

Satır sayıları (kod + test): **7.136 satır**.

---

## Yapılandırma

Yapılandırma dosyası yoktur; her ayar komut satırından veya `Ayarlar`
varsayılanlarından gelir. `ozet --format json` çıktısındaki `ayarlar` nesnesi
etkin değerleri her zaman raporlar.

### `diff` bayrakları

| Bayrak | Varsayılan | Etkisi |
|---|---|---|
| `--yol <YOL>` | — | Yalnızca bu depo içi yolu karşılaştırır. `..`, `.`, mutlak yol, `//`, `\`, `.git` reddedilir. |
| `--baglam <N>` | `3` | Unified diff başlığındaki `N` bağlam satırı. |
| `--arama-sirasi <SIRA>` | `pack-once` | `sha1 → nesne` arama sırası: `pack-once` (git'in davranışı) veya `gevsek-once`. |
| `--nesne-tavani <BAYT>` | `67108864` (64 MB) | Tek bir nesnenin çözülmüş boyut üst sınırı. Aşılırsa hata verilir. |
| `--format <BICIM>` | `duz` | `json`, `md` veya `duz` (yalnızca unified diff gövdesi). |

### `gecmis` bayrakları

| Bayrak | Varsayılan | Etkisi |
|---|---|---|
| `--rev <REV>` | `HEAD` | Başlangıç revizyonu: `HEAD`, `HEAD~N`, tam `refs/…` adı, dal/etiket kısa adı veya 40 onaltılık nesne adı. |
| `--yol <YOL>` | — | Yalnızca bu dosya yoluna dokunan commit'leri listeler. |
| `--limit <N>` | `0` | En fazla N kayıt. `0` = sınırsız. |
| `--format <BICIM>` | `json` | `json`, `md` veya `duz` (`<zaman> <sha7> <yazar>\t<konu>`). |

### `ozet` bayrakları

| Bayrak | Varsayılan | Etkisi |
|---|---|---|
| `--butunluk` | kapalı | Her pack nesnesinin `.idx` CRC-32'sini ve pack özetini doğrular (büyük depoda yavaştır). |
| `--format <BICIM>` | `json` | `json` veya `md`. |

### Salt kod içi ayarlar (`src/ayarlar.rs`)

| Alan | Varsayılan | Gerekçe |
|---|---|---|
| `nesne_tavani` | 64 MB | Zip-bomb benzeri şişirilmiş pack'e karşı tek nesne sınırı; delta başlığında *bildirilen* hedef boyut da bu tavana göre sınırlanır (bkz. madde 20). |
| `onbellek_bayt` | 32 MB | Bayt cinsinden sınırlı LRU; 260 MB tepe RSS bütçesinin anahtarı. |
| `delta_derinligi` | 64 | Delta zinciri sonsuz özyinelemesin; bozuk pack'te bellek şişmesin. |
| `EN_COK_D` (fark) | 600 | Myers iz büyüklüğü ≈ 5,8 MB; aşılırsa "kaba" blok diff'i üretilir ve `kaba: true` bildirilir. |

---

## Bilinen Sınırlamalar

**Zorunlu ve dürüst liste — MANIFEST.md kart 29 "Ertelenen" bölümünün tamamı:**

1. **Ortak değişim matrisi** yok (raporun merkezî fikri; v1'e ertelendi).
2. **Satır sahipliği (blame) ve yaşam döngüsü analizi** yok (v1'e ertelendi).
3. **İçerik arama** yok (v1'e ertelendi).
4. **`refs` yazma** yok — referanslar yalnızca okunur.
5. **Pack üretme yok** — `git gc` / `index-pack` uygulanmamıştır.
6. **HTML dışa aktarım** yok (v2'ye ertelendi).
7. **Diskte indeks** yok (v1'e ertelendi) — tüm hesap bellekte, akış hâlindedir.
8. **Grafik arayüz yok** — terminal + JSON/Markdown çıktısı.
9. **SHA-256 nesil desteği yok.** Depolar SHA-256 nesil kullanıyorsa `.idx`
   sihirli değeri eşleşmez ve araç açılışta açık bir hata verir. Bu, "desteklenmeyen
   nesilin sessizce yanlış okunmasından" bilinçli olarak yeğdir.

**Ek sınırlar (bu uygulamanın kendi kararları):**

10. **Yeni eklenen ve silinen dosyaların tam içeriği gösterilmez.** `diff` yalnızca
    yol durumunu (`eklendi` / `silindi`) raporlar; `git` tüm içeriği `+` satırı
    olarak basar. Yeni dosyalarda "tüm satırlar yeni" demek bilgi taşımaz.
11. **Kısaltılmış nesne adları çözülmez.** 7–39 karakterlik SHA-1 önekleri
    belirsizlik üretebileceğinden bilinçli olarak desteklenmez; 40 karakterlik tam
    ad gerekir.
12. **Myers tavanı.** Ortak ön/son ek ayrıldıktan sonra kalan blokta düzenleme
    uzaklığı 600'ü aşarsa fark "tümünü sil + tümünü ekle" olarak üretilir ve
    çıktıda `kaba: true` ile işaretlenir. Bu, "minimal" olmayan ama **doğru** bir
    farktır; sessizce yanlış fark üretilmez.
13. **`refs/remotes` okunmaz.** MVP kapsamı yalnızca `refs/heads` ve `refs/tags`
    ile sınırlıdır.
14. **Gitlink (alt modül) içeriği okunmaz.** `160000` girdileri yalnızca adıyla
    listelenir; alt depo hiçbir koşulda açılmaz.
15. **`objects/info/alternates` izlenmez.** Uzak nesne deposu bildirilir ve
    `ozet` çıktısında görünür, ancak takip edilmez (dosya sistemi dışına çıkmamak
    içindir).
16. **`.rev` dosyaları ve pack köküzlü "cruft pack"ler** okunmaz.
17. **Eşzamanlılık yok.** Tek iş parçacığı çalışır; b08 raporundaki paralel okuma
    ölçümü yapılmamıştır.
18. **`#[allow]` kullanımı.** Yalnızca bir yerde vardır:
    `tests/entegrasyon.rs::bozuk_yaz` içinde
    `#[allow(clippy::permissions_set_readonly_false)]` — gerekçe: `git gc` pack
    dosyasını salt okunur işaretler, "bozuk pack" testi ancak bu bayt kaldırıldıktan
    sonra kurulabilir. Dosya yalnızca testin kendi geçici dizinindedir.
19. **Ölçülmemiş iddialar.** Rapor b08'deki bellek ve süre bütçeleri (260 MB tepe
    RSS, 200.000 commit) **bu depoda ölçülmemiştir**; bunlar hedef değil, kartta
    kayıtlı tahminlerdir. README'de hiçbir yerde "ölçüldü" denmez.
20. **Güvenilmeyen `.pack` dosyalarında bildirilen nesne boyutları `nesne_tavani`
    ile sınırlanır.** Delta başlığındaki hedef boyut, 10 baytlık varint ile
    teorik olarak `u64::MAX`'e kadar yazılabilir. Bu sayıya **doğrudan** güvenilse
    (`Vec::with_capacity`) `capacity overflow` paniği doğar ve profil
    `panic = "abort"` olduğu için araç kontrollü bir hata yerine **düşer**
    (`0xC0000409`). Bu yüzden başlıktaki boyut, tahsis yapılmadan önce
    `nesne_tavani` (varsayılan 64 MB) ile karşılaştırılır; hedef, komut başına blok
    blok büyür. Aynı politika zlib çözme katmanında da geçerlidir: tavan orada
    *şişirilmiş akışın* uzunluğunu, burada ise *bildirilen nesne boyutunu* sınırlar.
    Meşru ve tavan içinde kalan büyük deltalar (8 MiB'a kadar ölçülmüş) etkilenmez;
    tavan aşımı `çözme sınırı aşıldı: N bayt > M bayt` hatasıyla bildirilir.

---

## Gelecek Geliştirmeler

- Ortak değişim matrisi ve satır sahipliği (raporun v1 aşaması).
- SHA-256 nesil desteği (`.idx` v3 / farklı nesne adı uzunluğu).
- Kısaltılmış nesne adı çözümü (pack fanout üzerinden aralık araması).
- Yeni/silinen dosyalar için isteğe bağlı "tam içerik" diff görünümü.
- Diskte indeks (rapor b06'daki indeks katmanı) — bellek bütçesini daha da
  düşürmek için.
- Ölçüm: gerçek bir 200.000 commit'lik depoda tepe RSS ve süre ölçümü.

---

## Troubleshooting

### 1. `... desteklenmiyor (sürüm v1 veya bilinmeyen imza; yalnızca v2 okunur)`

**Belirti:** `gitatlas ozet <depo>` bu hatayla çıkar.
**Neden:** Depodaki `.idx` dosyası sürüm 1 biçimindedir. Bu biçim başlıksızdır ve
bugün artık neredeyse hiç üretilmez; `git repack` veya `git gc` ile yeniden
yazılması gerekir.
**Çözüm:**
```console
$ cd <depo> && git repack -a -d
```
Depo yazılabilir değilse (salt okunur USB gibi), `git` **kurulu değilse bile**
alternatif yoktur: araç idx'i yeniden yazamaz (salt okunur sözleşmesi). Bu durum
bilinçli bir sınırdır.

### 2. `... bir git deposu değil (.git bulunamadı)`

**Belirti:** Açılış hatası.
**Neden:** Verilen yol çalışma ağacı kökü değil; içinde `.git` dizini yok.
**Çözüm:** Depo kökünü verin, ya da doğrudan `.git` dizinini (veya çıplak depo
dizinini) verin. Her üç biçim de kabul edilir:
```console
$ gitatlas ozet C:\projeler\uygulama
$ gitatlas ozet C:\projeler\uygulama\.git
$ gitatlas ozet C:\sunucular\uygulama.git
```

### 3. `nesne bozuk <oid>: içerik özeti uyuşmuyor: beklenen …, hesaplanan …`

**Belirti:** Tek bir nesne okunurken hata.
**Neden:** Depo bozuk (kısmi kopyalama, kesik indirme, disk hatası).
**Çözüm:** Git kuruluysa `git fsck` ile teyit edin. Git kurulu değilse araç
zaten hatayı en temiz yerden (SHA-1 doğrulaması) bildiriyor; nesne atlanmaz.
`ozet --butunluk` ile pack'in bütün nesnelerini topluca denetleyebilirsiniz.

### 4. `delta zincir derinliği tavanı (64) aşıldı`

**Belirti:** Bir nesne okunurken hata.
**Neden:** Pack'te 64'ten derin bir delta zinciri var — normalde git'in üretmediği
olağandışı bir durum (ağ kirliliği veya bozulma).
**Çözüm:** Depoyu `git gc` ile yeniden paketleyin. Araç bu hatada **sessizce devam
etmez**; E-004'ün "sessizce yanlış veri" yasağı burada uygulanır.

### 5. `geçersiz yol '../gizli': '..' bileşeni kullanılamaz`

**Belirti:** `--yol` ile geçersiz bir yol verildiğinde hata.
**Neden:** Yol güvenlik gereği katı doğrulanır: `..`, `.`, mutlak yol, `//`, geri
eğik çizgi, kontrol karakteri ve `.git` bileşeni reddedilir. Bu, dışarıdan gelen bir
yolun depo dışına çıkmasını veya `HEAD` gibi özel bir referansın yanlışlıkla
okunmasını engeller.
**Çözüm:** Depo içi göreli yol verin: `--yol src/lib.rs`, `--yol docs/README.md`.

---

## Gerçek Paketlenmiş Depo Kanıtı

Git kurulu olmayan bir makinede çalışma iddiası, gerçek bir paketlenmiş depoyla
doğrulanmıştır. GitHub'daki özel repolardan biri geçici dizine klonlanmış ve
GitAtlas ile okunmuştur (klon **proje klasörüne konulmamıştır**):

```console
$ git clone --quiet https://github.com/SyntaxOrigin/clipforge.git %TEMP%\gercek-depo\clipforge
$ gitatlas ozet %TEMP%\gercek-depo\clipforge --butunluk
```

Çıktı (özet):

```json
{
  "arac": "gitatlas",
  "komut": "ozet",
  "head": { "durum": "dal `main` → `ddf703b`" },
  "istatistik": {
    "commit_sayisi": 5,
    "yazar_sayisi": 1,
    "paket_sayisi": 1,
    "paket_nesne_sayisi": 41
  },
  "paketler": [
    {
      "dosya_boyutu": 106340,
      "nesne_sayisi": 41,
      "paket_ozeti": "099fe9b51ff8dd5c944ba3bd2ffdfdc267a9e728",
      "tur_dagilimi": {
        "blob": 23,
        "commit": 5,
        "ofs-delta": 8,
        "tree": 5
      }
    }
  ],
  "butunluk": { "paket_sayisi": 1, "dogrulanan_nesne": 41, "durum": "gecerli" },
  "sayaclar": {
    "onbellek_ten": 32, "gevsek_ten": 0, "pack_ten": 41, "sha1_dogrulanan": 41
  },
  "yazarlar": [ { "ad": "SyntaxOrigin", "commit": 5 } ]
}
```

Bu çıktı şunları **kanıtlar**:

- Depo tek bir `.pack` içinde, **8 adet `OBJ_OFS_DELTA`** ile saklanıyordu; yani
  sadece düz nesneler okuyan bir araç burada 8 nesneyi hiç göremezdi.
- 41 nesnenin **tamamı** `.idx` CRC-32'si, pack bütünlük özeti ve içerik SHA-1'i
  ile doğrulandı (`butunluk.durum: "gecerli"`).
- `paket_ten: 41` — hiçbir nesne gevşek dosyadan gelmedi, yani okuma yolu tamamen
  pack + delta üzerinden yürüdü.

`gecmis` çıktısı `git log --format="%h %s"` ile **birebir** aynıdır:

```console
$ gitatlas gecmis clipforge --format json | ...  →   gitatlas
$ git log --format="%h %s"
ddf703b docs: Turkce README, calistirilmis komutlar ve kapsam belgeleri
2ad247a test: sentetik kapsul ureticisi ve 182 test
2aa7c6a feat: toplu is kuyrugu, JSON belgeleri ve CLI alt komutlari
4d3ddba feat: platform profilleri, kadraj ve kip plani hesabi
c90099c feat: ISO BMFF ve Matroska kapsul ayristiricilari
```

> **Not:** Bu kanıtta `git` *klonlamak* için kullanılmıştır. GitAtlas'in kendisi
> klonlama yapmaz ve git çağırmaz; aynı depoyu yalnızca `.git` dizinini
> kopyalayarak (`git` olmadan) okuyabilir. `git`'in kurulu olmadığı makinede
> çalışması, "git'e hiçbir bağımlılığı yok" ilkesinin doğrudan sonucudur ve
> `src/` altında hiçbir yerde `Command::new("git")` çağrısı yoktur
> (`tests/yardimci/mod.rs` hariç — o yalnızca test fikstürü üreticisidir).

---

## Atıflar

### Spesifikasyonlar ve format belgeleri

- Git nesne ve paket biçimi — <https://git-scm.com/docs/gitformat-pack>
- Depo dizin düzeni — <https://git-scm.com/docs/gitrepository-layout>
- `git log` yürüyüş seçenekleri — <https://git-scm.com/docs/git-log>
- `git blame` (satır sahipliği kavramı, ertelenen özellik) — <https://git-scm.com/docs/git-blame>
- RFC 1950 — zlib veri sıkıştırma biçimi, <https://www.rfc-editor.org/rfc/rfc1950>
- RFC 1951 — DEFLATE, <https://www.rfc-editor.org/rfc/rfc1951>
- RFC 3174 — SHA-1 (doğrulama test vektörleri), <https://www.rfc-editor.org/rfc/rfc3174>
- FIPS 180-4 — Secure Hash Standard (SHA-1),
  <https://csrc.nist.gov/publications/detail/fips/180/4/final>
- Eugene W. Myers, *An O(ND) Difference Algorithm and Its Variations*,
  <https://www.cs.arizona.edu/people/gene/PAPERS/diff.ps> — satır farkı algoritması
- Howard Hinnant, `chrono`-suz takvim dönüşümü, <https://howardhinnant.github.io/date_algorithms.html>
- IEEE 802.3 / ISO 3309 — CRC-32 (`.idx` tablosundaki sağlama değerleri)

### Kullanılan crate'ler

- `flate2` — zlib/DEFLATE arayüzü, <https://docs.rs/flate2/>
- `miniz_oxide` — `flate2`nin saf Rust arka ucu, <https://docs.rs/miniz_oxide/>
- `serde_json` — JSON üretimi, <https://docs.rs/serde_json/>
- `clap` — CLI argüman türetimi, <https://docs.rs/clap/>

### Modellenen ve karşılaştırılan açık kaynak projeler

- Gitoxide (`gix`) — saf Rust git kütüphanesi; **kullanılmadı** (yasağı), yalnızca
  mimari referans, <https://gitoxide.rs/>
- libgit2 — C tabanlı git kütüphanesi; **kullanılmadı** (yasağı ve statik link
  zorluğu), <https://libgit2.org/>
- Dulwich — Python tabanlı git uygulama kütüphanesi; davranış referansı,
  <https://www.dulwich.io/>
- Git — sürüm kontrol sisteminin resmî sitesi ve belge dizini, <https://git-scm.com/>

### Rust ekosistemi

- Rust standart kütüphane belgeleri — <https://doc.rust-lang.org/std/>
- Rust edition rehberi (2021) — <https://doc.rust-lang.org/edition-guide/edition-2021/>
- `cargo` yerel rehberi — <https://doc.rust-lang.org/cargo/>

### Rapor dosyası

- Tasarım raporu: `%USERPROFILE%\Desktop\Fikirler\29-depo-haritasi-git-goruntuleyici.html`
  (yerel dosya; URL değil). Bu belge iç tasarımın kaynağıdır.
- Uygulama kararları: `%USERPROFILE%\Desktop\Projeler\MANIFEST.md` kart 29 ve
  `%USERPROFILE%\Desktop\Projeler\PROJECT_STATUS.md` karar **D-009** (pack desteğinin
  MVP'ye alınması) ile **E-004** (pack/delta çözümlemesinin en yüksek teknik risk
  olarak kaydı).

### Doğrudan kopyalanan kod

**Yok.** Tüm nesne/pack/delta/SHA-1 mantığı belirtimlerden sıfırdan yazılmıştır.
Alıntılanan projeler yalnızca davranış referansıdır; hiçbirinden satır alınmamıştır.

---

## Lisans

MIT — bkz. [`LICENSE.txt`](LICENSE.txt).

`Copyright (c) 2026 GitAtlas contributors`
