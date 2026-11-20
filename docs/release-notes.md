## Türkçe

**CarlUI 1.3.0: Pencere bölgeleri, ekran geneli yağmur, kasa için Windows Hello ve daha fazlası.**

**Yenilikler**
- **Pencere bölgeleri:** Bir pencereyi başlığından sürüklerken Shift'e basılı tutun; ekranda bölgeler belirir, bırakınca pencere imlecin altındaki bölgeye oturur. Ctrl+Shift ile bölgeler birleşir (2×2 ızgarada yarım ekran gibi). Düzen Ayarlar → Araçlar'dan seçilir.
- **Ekran geneli yağmur:** Yağmur sahnesi (ya da gerçek hava yağmurluyken) artık yalnızca dock'ta değil, masaüstünün tamamına yağar ve arada yıldırım çakar. Yoğunluk Hafif / Normal / Yoğun seçilebilir.
- **Arama eklentileri:** `%APPDATA%\CarlUI\plugins` klasörüne koyulan `.toml` dosyalarıyla kendi arama komutlarınızı ekleyin (ör. "gh carlui" → GitHub araması). Adres şablonu, program çalıştırma ya da sonuç listesi döndüren betik desteklenir.
- **Hızlı not:** Aramaya "not süt al" yazıp Enter'a basın; not tarihiyle `notes.md` dosyasına eklenir. "notlar" son notları listeler.
- **Carl Vault – Windows Hello:** Kasayı yüz, parmak izi ya da PIN ile açın. Ana parola hiçbir yere kaydedilmez.
- **Carl Vault – Sızıntı denetimi:** Kayıtlı parolaların bilinen veri sızıntılarında geçip geçmediğine bakar (Have I Been Pwned). Parola gönderilmez; yalnızca özetinin ilk 5 karakteri gider.
- **Görev Yöneticisi – Ağ sütunu:** Uygulama başına ağ hızı (yönetici olarak açıldığında).
- **Linux:** CachyOS/KDE için "Carl Paket Açıcı" (`linux/paket-acici`): `.deb`, `.rpm`, `.AppImage`, Flatpak ve Arch paketlerini çift tıkla kurar.

**Düzeltmeler**
- **Simge durumu animasyonu:** Dock görünürken pencere küçültülünce hem dock'a uçuş hem de Windows'un görev çubuğuna giden animasyonu oynuyordu. Artık yalnızca dock'a uçuş görünür.
- **Alan kaydı:** Durdurma kısayolu ve hap için ek günlük kayıtları.

---

## English

**CarlUI 1.3.0: Window zones, full-screen rain, Windows Hello for the vault and more.**

**New**
- **Window zones:** Hold Shift while dragging a window by its title bar; zones appear on screen and the window snaps into the one under the cursor when released. Ctrl+Shift merges zones (half screen on a 2×2 grid, for example). Pick the layout in Settings → Tools.
- **Full-screen rain:** The rain scene (or real rainy weather) now falls across the whole desktop, not just the dock, with occasional lightning. Choose Light / Normal / Heavy.
- **Search plugins:** Add your own search commands with `.toml` files in `%APPDATA%\CarlUI\plugins` (e.g. "gh carlui" → GitHub search). URL templates, running a program, or a script that returns a result list are supported.
- **Quick note:** Type "not buy milk" in search and press Enter; the note is appended with a date to `notes.md`. "notlar" lists recent notes.
- **Carl Vault – Windows Hello:** Unlock the vault with face, fingerprint or PIN. The master password is never stored.
- **Carl Vault – Leak check:** Checks whether saved passwords appear in known data breaches (Have I Been Pwned). Passwords are never sent; only the first 5 characters of their hash are.
- **Task Manager – Network column:** Per-app network speed (when run as administrator).
- **Linux:** "Carl Package Opener" for CachyOS/KDE (`linux/paket-acici`): installs `.deb`, `.rpm`, `.AppImage`, Flatpak and Arch packages with a double click.

**Fixes**
- **Minimize animation:** With the dock visible, minimizing a window played both the fly-to-dock animation and Windows' own animation toward the taskbar. Now only the dock animation shows.
- **Area recording:** Extra log entries for the stop shortcut and the pill.
