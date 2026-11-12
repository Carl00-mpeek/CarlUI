## Türkçe

**CarlUI 1.1.1: kurucu kapanış düzeltmesi.**

**Düzeltmeler**
- Kurulum bittikten sonra `CarlUI-Setup.exe` açık kalıp silinemeyebiliyordu; kurucu artık düzgün kapanıyor.
- Kurucunun başlattığı CarlUI artık kurucunun bulunduğu klasörü (ör. İndirilenler) kilitli tutmuyor.
- Kaldırma sırasında geçici kopya kurulum klasörünü kilitleyebiliyordu; düzeltildi.

---

## English

**CarlUI 1.1.1: installer exit fix.**

**Fixes**
- After installation `CarlUI-Setup.exe` could stay open and could not be deleted; the installer now exits cleanly.
- CarlUI started by the installer no longer keeps the installer's folder (e.g. Downloads) locked.
- During uninstall the temporary copy could lock the install folder; fixed.
