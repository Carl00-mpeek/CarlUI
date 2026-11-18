## Türkçe

**CarlUI 1.1.8: Windows 10 desteği ve Carl Vault yedeği.**

**Yeni**
- CarlUI artık Windows 10'da da çalışır.
- Carl Vault yedeği (Ayarlar → Güvenlik): ana parola sorulduktan sonra kasa şifreli `.carlvault` dosyasına yedeklenir. Yedek ana parolayla ya da bir kez gösterilen 24 karakterlik güçlü bir parolayla şifrelenir.
- İstenirse CSV ya da JSON olarak dışa aktarma; bu dosyaların şifresiz olduğu açıkça belirtilir.
- `.carlvault` yedeği İçe aktar'dan, ek dosyalarıyla birlikte geri yüklenir.

**Düzeltmeler**
- Windows 10'da ethernet ve ses düğmelerinin ve ayarlardaki birçok simgenin görünmemesi düzeltildi: Windows 11 yazı tipleri yoksa Segoe MDL2 Assets ve Segoe UI kullanılır.
- Windows 10'da cam efekti yerine düz, hafif saydam bir katman kullanılır.

---

## English

**CarlUI 1.1.8: Windows 10 support and Carl Vault backup.**

**New**
- CarlUI now runs on Windows 10 too.
- Carl Vault backup (Settings → Security): after asking for the master password, the vault is backed up to an encrypted `.carlvault` file, sealed with your master password or a generated 24-character password that's shown once.
- Optional CSV or JSON export, with a clear warning that these files aren't encrypted.
- Restore a `.carlvault` backup from Import, attachments included.

**Fixes**
- On Windows 10, the Ethernet and volume buttons and many icons in Settings were missing; CarlUI now falls back to Segoe MDL2 Assets and Segoe UI when the Windows 11 fonts aren't installed.
- On Windows 10 the glass effect is replaced by a solid, slightly transparent layer.
