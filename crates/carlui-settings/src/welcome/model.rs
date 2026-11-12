//! Karşılama akışının pencereden bağımsız kısmı: adımlar arası geçiş (ileri,
//! geri, atla, bitir) ve her seçimin ayarda tam olarak hangi alanı
//! değiştirdiği. Çizim ve pencere bu dosyaya bakar; testler de buraya.

use carlui_core::config::{Config, DockPosition, IslandStyle, TaskbarSize};

/// Akışın adımları; son adım özet ekranıdır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Look,
    Dock,
    Island,
    Modules,
    Finish,
}

impl Step {
    pub const ALL: [Step; 5] = [Step::Look, Step::Dock, Step::Island, Step::Modules, Step::Finish];
    /// İlerleme göstergesinde sayılan adım sayısı (özet hariç).
    pub const COUNT: usize = 4;

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn title(self) -> &'static str {
        match self {
            Step::Look => t!("Bir görünüm seç", "Pick a look"),
            Step::Dock => t!("Dock nerede dursun?", "Where should the dock go?"),
            Step::Island => t!("Adanın biçimi", "Island style"),
            Step::Modules => t!("Neler açık olsun?", "What should be on?"),
            Step::Finish => t!("Hazırsın", "You're all set"),
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Step::Look => t!("Tema, renk, cam ve köşeler birlikte değişir. Sonra Ayarlar'dan tek tek de değiştirebilirsin.", "Theme, color, glass and corners change together. You can tweak each one later in Settings."),
            Step::Dock => t!("Uygulamaların durduğu çubuk. Yüzen dock ya da görev çubuğu olabilir.", "The bar your apps live in. A floating dock or a taskbar."),
            Step::Island => t!("Ekranın üstündeki ada saati, çalan müziği ve bildirimleri gösterir.", "The island at the top shows the clock, what's playing and notifications."),
            Step::Modules => t!("İstemediğini kapat; hepsi sonra Ayarlar'dan açılabilir.", "Turn off what you don't want; everything can be turned on later in Settings."),
            Step::Finish => t!("Seçtiklerin uygulandı. İşine yarayacak birkaç kısayol:", "Your choices are applied. A few shortcuts worth knowing:"),
        }
    }
}

/// Gezinme isteği.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Next,
    Back,
    /// Tüm akışı atla (yalnızca karşılama işareti yazılır).
    Skip,
    /// Özet ekranındaki "Bitti".
    Finish,
    /// İlerleme noktasına tıklama.
    Go(Step),
}

/// Gezinmenin sonucu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay(Step),
    Close,
}

/// Adım makinesi: ileri/geri sınırda durur, atla ve bitir pencereyi kapatır.
pub fn navigate(step: Step, nav: Nav) -> Outcome {
    let i = step.index();
    match nav {
        Nav::Next if step == Step::Finish => Outcome::Close,
        Nav::Next => Outcome::Stay(Step::ALL[i + 1]),
        Nav::Back => Outcome::Stay(Step::ALL[i.saturating_sub(1)]),
        Nav::Skip | Nav::Finish => Outcome::Close,
        Nav::Go(s) => Outcome::Stay(s),
    }
}

/// Karşılama işaretini yazar; atla/kapat yalnızca bunu değiştirir.
pub fn mark(c: &mut Config) {
    c.general.welcomed = true;
}

/// Dock adımındaki seçimler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockChoice {
    Position(DockPosition),
    /// Yüzen dock (görev çubuğu kapalı).
    Minimal,
    Taskbar(TaskbarSize),
}

/// Dock seçimini uygular. Görev çubuğu yalnızca altta olabildiği için onu
/// seçmek konumu "Alt" yapar; yana almak da görev çubuğunu kapatır.
pub fn set_dock(c: &mut Config, d: DockChoice) {
    match d {
        DockChoice::Position(p) => {
            c.dock.position = p;
            if p.vertical() {
                c.dock.taskbar = false;
            }
        }
        DockChoice::Minimal => c.dock.taskbar = false,
        DockChoice::Taskbar(s) => {
            c.dock.taskbar = true;
            c.dock.taskbar_size = s;
            c.dock.position = DockPosition::Bottom;
        }
    }
}

/// Dock seçicilerinin şu anki seçili sırası: (konum, biçim).
pub fn dock_selection(c: &Config) -> (usize, usize) {
    let pos = match c.dock.position {
        DockPosition::Bottom => 0,
        DockPosition::Left => 1,
        DockPosition::Right => 2,
    };
    let style = match (c.dock.taskbar, c.dock.taskbar_size) {
        (false, _) => 0,
        (true, TaskbarSize::Compact) => 1,
        (true, TaskbarSize::Full) => 2,
    };
    (pos, style)
}

pub fn set_island(c: &mut Config, s: IslandStyle) {
    c.island.style = s;
}

/// Özet ekranındaki kısayollar: ayardaki gerçek tuşlar; kapalı modülün ya
/// da boş (atanmamış) kısayolun satırı çıkmaz. En çok üç tane.
pub fn tips(c: &Config) -> Vec<(&'static str, String)> {
    let s = &c.shortcuts;
    [
        (c.launcher.enabled, t!("Ara", "Search"), &s.search),
        (c.clipboard.enabled, t!("Pano geçmişi", "Clipboard history"), &s.clipboard),
        (true, t!("Ekran görüntüsü", "Screenshot"), &s.screenshot),
        (true, "Carl Vault", &s.vault),
        (c.ai.assistant, t!("Sesli asistan (basılı tut)", "Voice assistant (hold)"), &s.assist),
    ]
    .into_iter()
    .filter(|(on, _, key)| *on && !key.trim().is_empty())
    .map(|(_, label, key)| (label, key.trim().to_string()))
    .take(3)
    .collect()
}

/// Son adımdaki modül anahtarları. Hepsi gerçek bir "açık" bayrağıdır;
/// açmak indirme ya da ağır iş başlatmaz (sesli asistanın modeli ilk
/// kullanımda, onayla iner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    Search,
    Clipboard,
    Notify,
    Expander,
    WindowMemory,
    Cursor,
    Radial,
    Assistant,
}

impl Module {
    pub const ALL: [Module; 8] = [Module::Search, Module::Clipboard, Module::Notify, Module::Expander, Module::WindowMemory, Module::Cursor, Module::Radial, Module::Assistant];

    pub fn get(self, c: &Config) -> bool {
        match self {
            Module::Search => c.launcher.enabled,
            Module::Clipboard => c.clipboard.enabled,
            Module::Notify => c.notify.enabled,
            Module::Expander => c.tools.expander,
            Module::WindowMemory => c.tools.window_memory,
            Module::Cursor => c.cursor.enabled,
            Module::Radial => c.dock.radial,
            Module::Assistant => c.ai.assistant,
        }
    }

    fn slot(self, c: &mut Config) -> &mut bool {
        match self {
            Module::Search => &mut c.launcher.enabled,
            Module::Clipboard => &mut c.clipboard.enabled,
            Module::Notify => &mut c.notify.enabled,
            Module::Expander => &mut c.tools.expander,
            Module::WindowMemory => &mut c.tools.window_memory,
            Module::Cursor => &mut c.cursor.enabled,
            Module::Radial => &mut c.dock.radial,
            Module::Assistant => &mut c.ai.assistant,
        }
    }

    /// Yalnızca bu modülün bayrağını çevirir.
    pub fn toggle(self, c: &mut Config) {
        let s = self.slot(c);
        *s = !*s;
    }

    pub fn title(self) -> &'static str {
        match self {
            Module::Search => t!("Arama", "Search"),
            Module::Clipboard => t!("Pano geçmişi", "Clipboard history"),
            Module::Notify => t!("Adada bildirimler", "Notifications on the island"),
            Module::Expander => t!("Metin genişletici", "Text expander"),
            Module::WindowMemory => t!("Pencere yerlerini hatırla", "Remember window spots"),
            Module::Cursor => t!("CarlUI imleci", "CarlUI cursor"),
            Module::Radial => t!("Çark dock", "Radial dock"),
            Module::Assistant => t!("Sesli asistan", "Voice assistant"),
        }
    }

    /// Tek satırlık açıklama.
    pub fn desc(self) -> &'static str {
        match self {
            Module::Search => t!("Uygulama, dosya ve web'i tek kutudan ara.", "Find apps, files and the web from one box."),
            Module::Clipboard => t!("Kopyaladıkların listede kalır, geri yapıştırılır.", "What you copy stays in a list to paste again."),
            Module::Notify => t!("Windows bildirimleri adada görünür.", "Windows notifications show on the island."),
            Module::Expander => t!("Kısaltma yazınca uzun metne dönüşür.", "Typing an abbreviation expands it to text."),
            Module::WindowMemory => t!("Uygulamalar son kaldıkları yerde açılır.", "Apps reopen where you left them."),
            Module::Cursor => t!("Windows imleci yerine görünüme uyan imleç.", "A pointer that matches your look instead of Windows'."),
            Module::Radial => t!("Kısayolla imlecin çevresinde uygulamalar açılır.", "A shortcut opens apps around the pointer."),
            Module::Assistant => t!("Basılı tut, konuş; model ilk kullanımda iner.", "Hold, speak; the model downloads on first use."),
        }
    }
}
