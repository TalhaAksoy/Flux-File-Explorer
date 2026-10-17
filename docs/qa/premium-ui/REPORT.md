# Flux Files: yerel arayüz yenileme ve doğrulama raporu

## Durum

Bu dal çalışan Rust/egui uygulamasında kapsamlı iyileştirmeler içerir. **İlk istekteki tam kabul matrisi henüz tamamlanmış değildir.** Bu nedenle PR #5 taslaktır; `master` ile birleştirilmemiştir. Aşağıdaki ölçümler ve görüntüler gerçektir; tarayıcı veya arayüz maketi kullanılmadı.

Başlangıç: güncel `origin/master`, `0be3d65`. Dal: `design/premium-ui-overhaul`.
PR: https://github.com/TalhaAksoy/Flux-File-Explorer/pull/5

## A. Tasarım denetimi ve görsel dil

Üç sağlanan görüntü ve kaynak kod incelendi. Güçlü yönler: semantik palet, ortak widget altyapısı, sanallaştırılmış dosya görünümleri, asenkron metadata/yeniden adlandırma ve birleşik çalışma alanı sekmeleri. Sorunlar: ekran bazında küçük yazı boyutları, zayıf metadata kontrastı, sıkışık kontroller, çok geniş karşılaştırma sütunları, içerikten sonra yukarıda kalan işlem çubuğu ve gezinmeden uzaklaşan Ayarlar içeriği.

Yeni ortak ölçüler: 14pt ana metin, 12,5pt yardımcı metin, 16pt bölüm başlığı, 22pt sayfa başlığı; 34pt standart düğmeler, 32pt sekmeler/segmentler. Bazı özel kontroller yerel boyutlarını korur. Dosya satırları 24/30/36pt yoğunluklarına sahiptir. Renkler nötr yüzeyler ve ölçülü mavi seçim etrafında düzenlendi. Açık temanın düşük kontrastlı yardımcı metinleri koyulaştırıldı. Normal metin renklerinin beş ana yüzeyde AA kontrastı otomatik test ediliyor.

Yeni dekoratif animasyon veya sürekli yeniden çizim döngüsü eklenmedi. Mevcut anahtar animasyonu kısa tutuldu. Sistem genelinde azaltılmış hareket tercihi entegrasyonu eklenmedi.

## B. Ekran bazında uygulananlar

- **Explorer/araç çubuğu:** daha rahat hit alanları, uyarlanabilir arama genişliği ve buna göre sınırlanan adres/breadcrumb alanı. Yerel pencere dekorasyonları değiştirilmedi.
- **Sekmeler:** uzun başlıklar piksel genişliğine göre kısalır; tam başlık tooltip ile erişilebilir. Çok sayıda sekme yatay kaydırılır, yeni sekme düğmesi görünür kalır. Etkin sekmeye kaydırma eklendi. 40 sekmeyle gerçek egui işaretçi olayı testi mevcut.
- **Kenar çubuğu:** daha okunabilir bölüm başlıkları ve satırlar; uzun adların taşması giderildi. Yeniden boyutlandırma, bookmark işlemleri ve mevcut sürükle-bırak kodları korundu.
- **Details/Compact:** yoğunluk ölçüleri, daha rahat sütun başlıkları, sağa hizalı boyutlar ve dosya adlarında güvenli ellipsis. Görünür satır sanallaştırması korundu.
- **Grid/Gallery:** okunabilir etiketler, daha dengeli ikon/etiket oranı, doğru en-boy oranı ve tam ad tooltip'leri. Gallery artık 64px görüntüleri büyük fotoğraf alanına büyütmüyor: ayrı, tembel oluşturulan 256px önbellek kullanıyor. Fotoğraf olmayan ikonlar fotoğraf boyutuna büyütülmüyor.
- **Önizleme:** daha belirgin kimlik ve bölüm başlıkları, hizalı metadata, dar panelde yığılan satırlar ve sınırlanmış görüntü yüksekliği. Kopyalama düğmelerinin paneli her karede genişletmesi düzeltildi; üç genişlikte 60 karelik test eklendi. Metadata, hash ve klasör boyutu motorları korunuyor.
- **Bulk Rename:** 340pt kural alanı, en fazla 840pt karşılaştırma alanı, ortak sütun geometrisi, 34pt satırlar ve alta sabit eylemler. Dar görünümde kural açıklığı kullanılıyor. Başarılı yeni adlar yeşile boyanmıyor; durum açık metinle ayrı gösteriliyor. Filtre indeksleri ve sayımlar yalnızca plan/filtre/sorgu değiştiğinde yenileniyor. Gerçek egui tıklamasıyla geçici dosyaları yeniden adlandıran test mevcut.
- **Ayarlar:** 216pt kategori gezinmesi, gezinmeye yakın en fazla 840pt içerik sütunu, 40pt kategori satırları ve dar alanda yığılan tercih satırları. Çalışma alanlarında ikinci bir Places kenar çubuğu gösterilmiyor. Tema, yoğunluk, arama, font seçimi ve kalıcılık mantığı korundu.
- **Menüler/diyaloglar:** kısayol alanı etiket için ayrılan genişlikten düşülüyor; uzun etiketler kısayolla üst üste binmiyor. Menü düğmeleri ortak düğme kitine bağlandı. Diyalog başlıkları ve eylem boyutları iyileştirildi. Her diyalog ayrı ayrı yeniden tasarlanmadı veya görsel olarak doğrulanmadı.
- **Terminal:** daha rahat başlık alanı, kısalan uzun çalışma yolu, açıklayıcı “Follow folder” eylemi. PTY, giriş ve terminal renk motoru değiştirilmedi.

### Görsel QA sırasında bulunan işlevsel hata

Son metadata grubu snapshot sürümünü değiştirerek ilk sıralama hesaplamasını geçersiz kılabiliyordu. Yalnızca Size/Modified için son hesaplama istenmesi Name sıralamasını bazen geliş sırasında bırakıyordu. `finish()` artık ilk hesaplamadan önce çağrılıyor; metadata tamamlanınca tüm sıralama türleri için son görünüm isteniyor. Görünürlük bayrakları da bu son hesaplamaya giriyor. 512 gerçek dosyayla son doğal sıralamanın kararlı kalması test ediliyor. Dosya motoru yeniden yazılmadı.

## C. Gerçek ekran görüntüleri

Aynı 13 girdilik test klasörü: dört dizin, üç gerçek görüntü, uzun adlar ve Türkçe/Japonca dosya adları. Görüntüler makinedeki KDE Altai, Autumn ve BytheWater duvar kâğıtlarından test girdisi olarak alındı; bunlar Flux arayüz çizimleri değildir. PDF/XLSX uzantılı girdiler içeriksiz test dosyalarıdır.

Varsayılan çekimler 1440×900 mantıksal boyut, masaüstü ölçeği %125 nedeniyle 1800×1125 fiziksel pikseldir. Küçük çekimler 1024×768 mantıksal boyuttadır. Ölçekli Grid çekimi %200 uygulama ölçeği kullanır. Son çekimler kullanıcı egui yerleşiminden ve tercihlerinden yalıtılmıştır. Önce çekimleri eski ikili dosyadan alınmıştır; eski aracın egui yerleşim yalıtımı bulunmadığından önce/sonra panel durumları birebir aynı kabul edilmemelidir.

| Ekran | Önce | Sonra |
|---|---|---|
| Explorer, koyu | [Önce](screens/before/explorer.png) | [Sonra](screens/after/explorer-dark.png) |
| Bulk Rename | [Önce](screens/before/rename.png) | [Sonra](screens/after/rename-dark.png) |
| Ayarlar | [Önce](screens/before/settings.png) | [Sonra](screens/after/settings-dark.png) |

Diğer çekimler: [açık Explorer](screens/after/explorer-light.png), [Grid](screens/after/grid-dark.png), [açık Gallery](screens/after/gallery-light.png), [önizleme](screens/after/preview-dark.png), [terminal](screens/after/terminal-dark.png), [açık Ayarlar](screens/after/settings-light.png), [tipografi](screens/after/typography-dark.png), [koyu menü](screens/after/menu-dark.png), [açık menü](screens/after/menu-light.png), [küçük Rename](screens/after/small-rename.png), [küçük Ayarlar](screens/after/small-settings.png), [%200 Grid](screens/after/scaled-grid.png).

Ek kategoriler: [dosyalar](screens/after/settings-files.png), [görünümler](screens/after/settings-views.png), [klavye](screens/after/settings-keys.png), [gelişmiş](screens/after/settings-advanced.png), [hakkında](screens/after/settings-about.png). Toplam 20 son durum çekimi kaydedildi. Büyük ekranlar, çalışma alanları, menüler ve kategoriler görsel olarak incelendi; bu işlem tüm etkileşimlerin test edildiği anlamına gelmez.

Yeniden üretim:

```sh
cargo build --release -p flux-app
python3 scripts/native-visual-qa.py /path/to/fixture /path/to/output
```

Önizleme örneği için test klasöründe `Autumn.jpg` bulunmalıdır.

## D. Performans

Ortam: Fedora Linux 7.2.9 x86_64, Wayland, Intel i7-13650HX (20 mantıksal CPU), yaklaşık 30 GiB RAM. Sistemde NVIDIA RTX 4060 Laptop GPU bulunuyor; etkin GL aygıtı ayrıca doğrulanmadı. Depo Btrfs üzerinde; benchmark girdileri `/tmp` tmpfs üzerinde. Release: opt-level 3, thin LTO, tek codegen birimi. Makine paylaşımlı ve bellek baskısı altında; disk/soğuk önbellek performansı için bu sonuçlar genellenmemelidir.

### Dosya motoru, üç eşlenmiş koşunun medyanı

| Ölçüt | Önce | Sonra | Fark |
|---|---:|---:|---:|
| 1k tam listeleme | 1,1 ms | 1,1 ms | 0 |
| 10k tam listeleme | 3,2 ms | 3,2 ms | 0 |
| 100k tam listeleme | 22,3 ms | 21,3 ms | −1,0 ms |
| 100k görünüm hesaplama | 147,4 ms | 138,4 ms | −9,0 ms |
| İlk grup, tüm boyutlar | 1,1 ms | 1,1 ms | 0 |
| 100k snapshot modeli | 7,5 MiB | 7,5 MiB | 0 |

Dosya motoru değişmedi; bu farklar bir motor optimizasyonu iddiası değildir. Harness 1ms aralıkla olay yoklar; özellikle 1k sonuçları bu çözünürlükle sınırlıdır. İlk ölçüm turu ve eşlenmiş tekrarların ham çıktıları saklandı. 1 milyon girdi üretimi tmpfs inode sınırına takıldı; tamamlanmış benchmark olarak raporlanmadı. Oluşan geçici girdiler temizlendi.

### egui CPU yerleşim/çizim-komutu maliyeti, 100k girdi

1280×720 mantıksal viewport; 60 ısınma + 300 örnek, sentetik kaydırma girdisi, üç eşlenmiş koşu. Her sütun koşu bazındaki istatistiklerin medyanıdır. Bu probe **GPU sunum süresi, gerçek masaüstü kare gecikmesi veya FPS ölçmez**. Önce ikilisi temiz derlenmiş başlangıç worktree'sinden alındı; şekil sayıları eski/yeni geometriyi doğrular.

| Görünüm | Medyan önce → sonra | p95 önce → sonra | p99 önce → sonra |
|---|---:|---:|---:|
| Details | 0,155 → 0,141 ms | 0,184 → 0,170 ms | 0,364 → 0,272 ms |
| Compact | 0,039 → 0,034 ms | 0,044 → 0,037 ms | 0,054 → 0,047 ms |
| Grid | 0,061 → 0,073 ms | 0,067 → 0,080 ms | 0,101 → 0,093 ms |
| Gallery | 0,026 → 0,032 ms | 0,028 → 0,037 ms | 0,033 → 0,044 ms |
| Ayarlar | 0,026 → 0,027 ms | 0,027 → 0,030 ms | 0,029 → 0,034 ms |

Grid/Gallery'deki küçük CPU artışı saklanmıyor; en büyük medyan fark 0,012ms. Fotoğraf çözme stresi veya 60Hz uçtan uca akıcılığı bu probe ile kanıtlanamaz. Details'ta en fazla 221 → 186 çizim şekli, Grid'de 259 → 247 şekil üretildi; 100k girdinin tamamı widget'a dönüştürülmedi. Daha yüksek varsayılan satırlar da görünür öğe sayısını azaltır.

### Boşta kaynak kullanımı

100k klasör, 8s bekleme ardından 10s `/proc` örneği, üç eşlenmiş koşu: CPU her iki sürümde %0,0 örneklenmiş tek-çekirdek kullanımı; RSS medyanı 168,625 → 168,152 MiB (−0,473 MiB). Bu fark bellek iyileştirmesi olarak yorumlanmıyor. Sayaç çözünürlüğü ve kısa pencere nedeniyle “hiç CPU kullanmaz” iddiası değildir. İlk, kararlı olmayan ölçümler karşılaştırma tablosuna alınmadı.

Gallery önbelleği en çok 64 adet 256² RGBA doku taşır: texel hesabıyla 16 MiB üst sınır, gerçek sürücü tahsisi ölçümü değil. Details/Grid 64px önbelleğini korur. Native ikon önbelleği 512 girdilik mevcut sınırını korur. İş kuyruğu/in-flight sınırları korunur. Büyük Gallery fotoğraf yükünde tepe RSS ayrıca ölçülmedi.

Ham sonuçlar: [CPU probe](results/final-ui-profile.txt), [eşlenmiş motor ölçümleri](results/paired-core-final.txt), [idle](results/paired-idle.txt), [ilk baseline](results/baseline-bench.txt), [ilk son durum ölçümü](results/after-bench.txt), [Criterion](results/criterion.log).

## E. Mimari ve güvenlik

`theme::metrics` ortak geometriyi, `Palette` semantik renkleri, `widgets::elided_text` piksel bazlı Unicode-güvenli kısaltmayı merkezileştirir. Ayarlar ve Rename mevcut çalışma alanı durumlarını kullanır. Rename türetilmiş önizleme önbelleği yeni plan kabulünde geçersiz kılınır. Gallery önbelleği yalnızca ihtiyaç halinde oluşturulur; var olan decode boyut/dosya/iş sınırları sürer. Yeni bağımlılık yoktur. Dosya işlemi, arşiv, çöp kutusu, metadata ve PTY motorları değiştirilmedi.

## F. Doğrulama

Yerel son kaynakta:

- `cargo fmt --all -- --check`: geçti.
- `cargo clippy --workspace --all-targets -- -D warnings`: geçti.
- `cargo test --workspace`: 138 test geçti; başarısız test yok.
- `cargo build --release -p flux-app`: geçti.
- `cargo bench -p flux-bench`: mevcut Criterion benchmark'ları çalıştırıldı, sonuçlar yukarıda.
- Native Wayland uygulaması: 20 ekran çekimi, birden fazla düzeltme turu.

Yeni/kuvvetlendirilmiş testler: tema/yoğunluk bağımsızlığı, doğrudan Enter etkinleştirmesi, dar Ayarlar segment sınırları, Rename footer konumu, karşılaştırma geometrisi, önizleme önbellek geçersizliği, metadata panelinin genişlememesi, Gallery çözünürlük sınırı, taşan sekme çubuğunda gerçek tıklama, metadata sonrası son sıralama ve gerçek dosyalara işaretçiyle Rename uygulama.

1 milyon girdilik başarısız üretim sırasında bir ilk workspace testi de inode yokluğu nedeniyle başarısız oldu. Ortam temizlendikten sonra tam testler yeniden çalıştırıldı; kod uyarıları bastırılmadı.

## G. Platformlar

Yerel etkileşim/görsel kontrol: Linux Wayland. GitHub Actions Ubuntu, Windows ve macOS için fmt/clippy/test/release işlerini çalıştırıyor. CI derlemesi görsel doğrulama yerine geçmez. KDE/GNOME/Cinnamon, X11, Windows ve macOS üzerinde tam interaktif kabul matrisi tamamlanmadı. Son commit'in CI durumu için PR kontrolleri esas alınmalıdır.

## H. Kalan kabul işleri ve sınırlar

- Japonca örnek ad mevcut font zincirinde kutu gliflerle görünüyor. Türkçe örnekler doğru; tam Unicode kapsamı iddia edilmiyor.
- Tüm çözünürlük/ölçek kombinasyonları, özellikle dar Explorer + Preview ve 4K/karma DPI matrisi bitmedi.
- Alt menülerin gerçek işaretçi geçişleri ve tüm diyaloglar için ayrı çekimler/etkileşim testi tamamlanmadı. Çok kısa pencerede uzun menülerin erişilebilirliği ayrıca denetlenmeli.
- Kopyala/kes/yapıştır, çöp kutusu, kalıcı silme, arşiv, sürükle-bırak, bookmark sıralama ve terminal girişinin tamamı bu oturumda GUI üzerinden tek tek denenmedi. Mevcut motor testlerinin geçmesi bu manuel kabulün yerine konmuyor.
- Başlangıç/ilk görünür dosya gecikmesi, yerel kare sunumu, resize/tab/preview p95/p99 gecikmeleri ve büyük fotoğraf iş yükü tepe belleği ölçülmedi.
- Font arama ve yeni azaltılmış-hareket tercihi eklenmedi; gelişmiş font fallback işi açık.

Bu maddeler nedeniyle ilk talebin **tamamlandı** beyanı verilmez ve PR taslak olarak tutulur. Doğrulanmamış davranışlar için `master` entegrasyonu yapılmadı.

## I. Git teslimi

İlgili commit'ler: `1fd3625` tasarım, `4458a3e` ortak sistem/Explorer, `867da3c` çalışma alanları/Gallery, `68bb5a9` screenshot yalıtımı/sekme testi, `7869639` metadata sonrası sıralama. Ek test/görüntü/rapor commit'i bu raporla birlikte gelir. Nihai `master` başlangıç SHA'sında kalır: `0be3d65` (uzak dal ayrıca kontrol edilmelidir). Force-push veya koruma atlama yapılmadı.
