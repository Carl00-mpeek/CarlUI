## Türkçe

**CarlUI 1.3.2: Tam Kaldırıcı artık daha çok uygulama görüyor ve geride çok daha az iz bırakıyor.**

**Yenilikler**
- **Kayıtsız programlar:** Kurulum kaydı olmayan (taşınabilir ya da kaydı silinmiş) programlar, Başlat menüsü ve masaüstü kısayollarından bulunup listede "Kayıtsız program" olarak görünür. Kendi kaldırıcıları olmadığı için "Kaldır" doğrudan klasörü ve izleri tarar.
- **Daha derin artık taraması:** Uygulamanın kurulum klasörünü gösteren her şey bulunur: Başlat menüsü alt klasörlerindeki, masaüstündeki ve görev çubuğuna sabitlenmiş kısayollar; açılışta çalışma kayıtları (Run / RunOnce) ve Görev Yöneticisi'ndeki başlangıç izleri; "uygulama yolu" ve "Birlikte aç" kayıtları; Windows hizmetleri ve zamanlanmış görevler.
- **Hizmet ve görev temizliği:** Uygulamanın bıraktığı hizmetler önce durdurulup sonra silinir, zamanlanmış görevleri kaldırılır. Silme sırası çalışan şeylerden dosyalara doğrudur; kilitli dosya kalmaz.
- **Mağaza uygulamaları:** Kaldırıldıktan sonra da artıklar taranır; geride kalan `Packages` verisi bulunur.
- Kurulum klasörünü yazmayan kurucularda konum, simgenin ya da kaldırıcının klasöründen bulunur; bu uygulamaların artıkları da artık yakalanır. `%TEMP%` ve kullanıcı klasöründeki `.uygulama` ayar klasörleri de taranır.

**Güvenlik**
- Yeni öğeler de aynı korumadan geçer: başlangıç kayıtları yalnızca izinli anahtarlardan silinir, dosyası Windows klasöründe olan hizmetlere ve Windows'un kendi görevlerine dokunulmaz. Dosyalar yine Geri Dönüşüm Kutusu'na gider, kayıtlar önce yedeklenir.

---

## English

**CarlUI 1.3.2: The Uninstaller now finds more apps and leaves far fewer traces behind.**

**New**
- **Unregistered programs:** Programs with no install record (portable, or with a deleted record) are found through Start menu and desktop shortcuts and listed as "Unregistered program". They have no uninstaller, so "Uninstall" goes straight to scanning their folder and traces.
- **Deeper leftover scan:** Anything that points into the app's install folder is found: shortcuts in Start menu subfolders, on the desktop and pinned to the taskbar; Run / RunOnce startup entries and their Task Manager startup records; App Paths and "Open with" registrations; Windows services and scheduled tasks.
- **Service and task cleanup:** Services the app left behind are stopped, then deleted; its scheduled tasks are removed. Items are removed from running things down to files, so nothing stays locked.
- **Store apps:** Leftovers are scanned after removal too, including leftover `Packages` data.
- For installers that don't record their install folder, the location is worked out from the icon or uninstaller folder, so those apps' leftovers are caught as well. `%TEMP%` and `.app` settings folders in the user folder are scanned too.

**Safety**
- New items go through the same protection: startup entries are only removed from allowed keys, and services whose file lives in the Windows folder and Windows' own tasks are never touched. Files still go to the Recycle Bin and registry entries are backed up first.
