## Türkçe

**CarlUI 1.1.11: Bluetooth paneli, ağ geçişi ve kar birikintisi.**

**Yenilikler**
- Dock'un durum alanına (Compact ve Full) Bluetooth simgesi eklendi. Tıklayınca bağlı ve eşleştirilebilir cihazlar CarlUI'nin kendi panelinde listelenir; Bluetooth buradan açılıp kapatılabilir ve cihaza bağlanılabilir.
- Ağ panelinde Ethernet satırına sağ tıklayınca Wi-Fi'ye, Wi-Fi'ye sağ tıklayınca Ethernet'e geçilir.
- Minimal ve Full görünümde ağ simgesine sağ tıklamak Ağ ve İnternet ayarlarını, Bluetooth simgesine sağ tıklamak Bluetooth ayarlarını açar.
- Hava durumu efekti: kar yağarken açık pencerelerin, dock'un ve adanın üst kenarında kar birikir; yağmurda aynı kenarlarda ıslak bir parıltı görünür.
- CPU sıcaklığı artık sürücü olmadan da okunur: PawnIO yoksa çalışan HWiNFO'dan ("Shared Memory Support" açıkken, Intel ve AMD), o da yoksa Windows'un ACPI ısı bölgesinden. Hiçbiri yoksa "—" görünür.

**Düzeltmeler**
- Ekranı kaplayan bir pencere küçültülüp dock'tan açılınca küçük boyutta geliyordu; artık Alt+Tab'daki gibi ekranı kaplayarak açılır.
- Tray okunun arkasındaki zemin artık yalnızca panel açıkken görünür; kapalıyken iz kalmaz.

**Diğer**
- Carl Vault'un Linux sürümü kaldırıldı ve artık yayımlanmıyor.

---

## English

**CarlUI 1.1.11: Bluetooth panel, network switching and snow build-up.**

**New**
- A Bluetooth icon was added to the dock's status area (Compact and Full). Clicking it lists connected and pairable devices in CarlUI's own panel, where you can also turn Bluetooth on or off and connect to a device.
- In the network panel, right-click the Ethernet row to switch to Wi-Fi, and right-click Wi-Fi to switch back to Ethernet.
- In Minimal and Full views, right-clicking the network icon opens Network & Internet settings, and right-clicking the Bluetooth icon opens Bluetooth settings.
- Weather effect: while it snows, snow builds up on the top edges of open windows, the dock and the island; in rain the same edges get a wet shimmer.
- CPU temperature now works without a driver: without PawnIO it's read from a running HWiNFO (with "Shared Memory Support" on, Intel and AMD), or else from Windows' ACPI thermal zone. If none is available it shows "—".

**Fixes**
- A maximized window that was minimized came back at its smaller size when clicked in the dock; it now opens maximized, like with Alt+Tab.
- The plate behind the tray arrow now only shows while the panel is open; nothing is left behind when it's closed.

**Other**
- The Linux version of Carl Vault was removed and is no longer published.
