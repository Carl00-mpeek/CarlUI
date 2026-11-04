<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/English-3E8BFF?style=for-the-badge" alt="English"></a>
  <a href="README.tr.md"><img src="https://img.shields.io/badge/T%C3%BCrk%C3%A7e-1f2128?style=for-the-badge" alt="Türkçe"></a>
</p>

<h1 align="center">CarlUI</h1>

<p align="center">
  A lightweight, fast desktop shell for Windows 11, written in Rust.<br>
  Floating dock, a top island for the clock, notifications and media, search, clipboard history and a control center.
</p>

<p align="center">
  <a href="https://github.com/Carl00-mpeek/CarlUI/releases/latest"><img src="https://img.shields.io/github/v/release/Carl00-mpeek/CarlUI?style=flat-square&color=3E8BFF" alt="Release"></a>
  <img src="https://img.shields.io/badge/Windows-11-3E8BFF?style=flat-square" alt="Windows 11">
  <img src="https://img.shields.io/badge/Rust-orange?style=flat-square" alt="Rust">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT"></a>
</p>

<p align="center">
  <a href="https://github.com/Carl00-mpeek/CarlUI/releases/latest"><b>⬇ Download CarlUI-Setup.exe</b></a>
  &nbsp;·&nbsp; no admin rights needed &nbsp;·&nbsp; free and open source
</p>

![CarlUI desktop](docs/screenshots/desktop-en.png)

<p align="center">
  <img src="docs/screenshots/island-en.png" alt="Island: download progress and notifications"><br>
  <sub><b>Island</b> · live download progress (name, size, speed, time left), Steam games included, and notifications</sub>
</p>

<p align="center">
  <img src="docs/screenshots/steam-en.png" alt="Steam download on the island"><br>
  <sub><b>Steam download</b> · the game's name and total size come straight from Steam; downloaded amount, speed and time left are estimated from network and disk use</sub>
</p>

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/calc-en.png" alt="Calculator"><br><sub><b>Search</b> · calculates as you type</sub></td>
    <td width="50%"><img src="docs/screenshots/currency-en.png" alt="Currency"><br><sub><b>Currency and unit conversion</b> · daily rates</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/clip-en.png" alt="Clipboard history"><br><sub><b>Clipboard history (Win+V)</b> · text and images, search, editing</sub></td>
    <td><img src="docs/screenshots/control-en.png" alt="Control center"><br><sub><b>Control center</b> · volume, brightness, Wi-Fi, tray icons</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/settings-en.png" alt="Settings"><br><sub><b>Settings</b> · everything in one window, applied instantly</sub></td>
    <td><img src="docs/screenshots/taskmgr-en.png" alt="Task Manager"><br><sub><b>Task Manager</b> · unsigned programs first</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/setup-en.png" alt="Installer"><br><sub><b>Installer</b> · one file, two languages, no admin rights</sub></td>
    <td></td>
  </tr>
</table>

## Features

- **Dock:** Pinned and running apps, live window previews, drag and drop,
  auto-hide, Win+1…9, Recycle Bin.
- **Island:** Clock at the top center; notifications, now playing (cover,
  progress, per-app volume), download progress with file name, size, speed
  and time left (each can be turned off in Settings). Downloads from other
  apps (Steam, Epic, torrent clients…) show up too: estimated from network and
  disk use, with the game's name and progress for Steam.
- **Search:** Windows key or Ctrl+Space. Apps, files, calculator,
  unit/currency conversion, web and AI shortcuts.
- **Win menu:** Pinned apps, folders, power buttons.
- **Clipboard history (Win+V):** Text and images, search, pinning, in-place
  editing, hidden passwords that delete themselves. Opens over fullscreen
  games without minimizing them.
- **Control center:** Tray icons, volume (output device and per-app volume),
  Wi-Fi, Bluetooth, brightness.
- **Alerts:** A notification when the CPU, GPU or an SSD passes the
  temperature you set, or when a drive is almost full. Limits are in
  **Settings → Island and notifications → Alerts**.
- **Game mode:** In fullscreen the UI steps aside, modules go to sleep and
  notifications wait until you're done.
- **Task manager, file converter, screenshot tool** and a **Settings** window
  that controls all of it. The task manager has a **Run as administrator**
  button for processes and startup items that need it.
- **English and Turkish:** Follows the Windows display language; change it in
  **Settings → General → Language**.

## Install

1. Download `CarlUI-Setup.exe` from [Releases](https://github.com/Carl00-mpeek/CarlUI/releases/latest).
2. Run it and click **Install**. No admin rights needed: CarlUI installs to
   `%LOCALAPPDATA%\Programs\CarlUI`, adds itself to the Start menu and starts
   when you sign in.

**"Windows protected your PC":** The installer isn't code-signed, so
SmartScreen shows this warning. Click **More info → Run anyway**. Some
antivirus tools may raise a false alarm too: CarlUI hides the taskbar, uses a
keyboard hook for its shortcuts, and the optional temperature reader runs as
admin. All of this stays on your PC. CarlUI collects no data; it only goes
online for things you ask for (currency rates, translation, web search,
downloads, the daily update check). All of the source code is in this repo.

A [VirusTotal scan](https://www.virustotal.com/gui/file/3563d3310a40bc053c981eb4d83b9694dc96d62a26b3439e282ff4ace31e3597)
of the current installer: 68 of 71 engines, including Microsoft Defender,
found it clean. Only ESET's generic "Kryptik" heuristic, Bkav and SecureAge
flagged it, all common false positives for unsigned installers. You can
compare the SHA-256 of your download with the one on the Releases page.

### Optional parts

- **CPU temperature:** Intel CPUs need the [PawnIO](https://pawnio.eu) driver.
  If it's installed, the installer turns on "CPU temperature"; it asks for
  admin rights once.
- **Browser extension:** Moves downloads started in the browser to the island.
  It ships in `%LOCALAPPDATA%\Programs\CarlUI\extension\carlui-downloads`; turn
  on developer mode on your browser's extensions page and use **Load unpacked**
  on that folder. See **Settings → Search → Browser extension**.
- **Wallpaper:** The installer's "CarlUI wallpaper" option. Files are in
  [assets/wallpaper](assets/wallpaper) (1920×1080, 2560×1440, 3840×2160) and
  can be regenerated with `tools\make-wallpaper.ps1`.

### Updates

Once a day CarlUI checks GitHub for a new version and, if there is one, shows
a notification on the island. Nothing happens until you click it: then CarlUI
downloads the new installer, checks its SHA-256 against the release, installs
it silently and restarts. Your settings are kept. If something goes wrong, the
notification links to the release page so you can update by hand. Turn the
check off in **Settings → General → Updates**.

### Uninstall

From **Windows Settings → Apps → Installed apps**, or the **Uninstall CarlUI**
shortcut in the Start menu. The taskbar comes back and installed files go to
the Recycle Bin. You choose whether to keep your settings.

**Emergency:** If the taskbar somehow doesn't come back:

```powershell
& "$env:LOCALAPPDATA\Programs\CarlUI\carlui.exe" --restore-taskbar
```

## Settings and data

| What | Where |
|---|---|
| Program | `%LOCALAPPDATA%\Programs\CarlUI\` |
| Settings | `%APPDATA%\CarlUI\config.toml` (edited from the Settings window, applied instantly) |
| Clipboard history, cache | `%LOCALAPPDATA%\CarlUI\` |
| Logs | `%LOCALAPPDATA%\CarlUI\logs\` (kept for 7 days) |

## Building from source

You need [Rust](https://rustup.rs) and Visual Studio Build Tools (C++).

```powershell
cargo build --release
.\target\release\carlui.exe          # run
.\target\release\carlui.exe --quit   # quit
```

To build the single-file installer:

```powershell
powershell -ExecutionPolicy Bypass -File tools\build-setup.ps1   # → dist\CarlUI-Setup.exe
```

Architecture: every module (dock, tray, island, Win menu…) is its own
process; if one crashes, the main process restarts it. Modules talk over a
named pipe. UI strings live side by side in both languages:
`t!("Kapat", "Close")`. Development notes (in Turkish):
[docs/GELISTIRME-NOTLARI.md](docs/GELISTIRME-NOTLARI.md).

## Code signing policy

Windows installers are to be signed for free through
[SignPath.io](https://about.signpath.io), with a certificate from
[SignPath Foundation](https://signpath.org) (application in progress).

- Only `CarlUI-Setup.exe` built by the [Release workflow](.github/workflows/release.yml)
  from this repository's source gets signed; nothing built on a personal
  machine.
- Committers, reviewers and approvers: [Carl00-mpeek](https://github.com/Carl00-mpeek)
  (the only maintainer). Every release is approved by hand.

### Privacy

CarlUI collects no data and has no telemetry. It only connects to other
systems when you ask it to (currency rates, translation, web search,
downloads) and for the daily update check against the GitHub API, which can be
turned off in **Settings → General → Updates**.

## License

MIT © 2026 Demhat Dayan. See [LICENSE](LICENSE). Third-party parts (PawnIO
IntelMSR module, LGPL-2.1, and Rust crates):
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

Bugs and ideas: [Issues](https://github.com/Carl00-mpeek/CarlUI/issues).
Contact: maildemhat@gmail.com
