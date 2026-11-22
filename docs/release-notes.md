## Türkçe

**CarlUI 1.3.3: Carl Vault artık Discord'un giriş ekranını tanıyor ve giriş bilgilerini kendisi dolduruyor.**

**Düzeltmeler**
- **Discord'da Vault düğmesi:** Discord giriş sayfasına başlık vermediği için pencere başlığı " - Discord" ya da sayfa adresi ("discord.com/app… - Discord") oluyordu; düğme açılış penceresinde görünüp giriş ekranı gelince kayboluyordu. Artık bu başlıklar tanınıyor ve giriş ekranı parola kutusuna bakılarak doğrulanıyor. Discord PTB ve Canary de destekleniyor.
- **Düğme kutunun yanında:** Chromium tabanlı uygulamalarda (Discord, Steam…) parola kutusu bulunduğunda Vault düğmesi pencerenin üstü yerine doğrudan kutunun yanında çıkıyor.
- **Tıklamadan doldurma:** Kasa bu uygulamalarda da kullanıcı adı ve parola kutularını kendisi buluyor; önce kutuya tıklamak gerekmiyor. Kutular bulunamazsa eskisi gibi odaktaki kutuya yazılıyor.

**Güvenlik**
- Erişilebilirlik sorgusu yalnızca giriş ekranına benzeyen pencerelere yapılır; diğer Chromium uygulamaları yavaşlatılmaz. Yazmadan önce pencerenin hâlâ önde ve aynı uygulama olduğuna bakılmaya devam eder.

---

## English

**CarlUI 1.3.3: Carl Vault now recognizes Discord's login screen and fills in your credentials by itself.**

**Fixes**
- **Vault button in Discord:** Discord gives its login page no title, so the window title was " - Discord" or the page address ("discord.com/app… - Discord"); the button showed on the splash window and disappeared once the login screen appeared. These titles are now recognized and the login screen is confirmed by looking for the password field. Discord PTB and Canary are supported too.
- **Button next to the field:** In Chromium-based apps (Discord, Steam…), when the password field is found the Vault button appears right next to it instead of at the top of the window.
- **Filling without clicking:** The vault now finds the username and password fields in these apps as well, so there's no need to click into a field first. If the fields can't be found, it types into the focused field as before.

**Safety**
- Accessibility queries are only made to windows that look like a login screen, so other Chromium apps aren't slowed down. Before typing, it still checks that the window is in front and is the same app.
