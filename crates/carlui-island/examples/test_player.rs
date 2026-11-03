//! Test için: bir ses dosyasını Windows'un oynatıcısıyla çalar ve sistem medya
//! kontrollerine (Spotify'ın yaptığı gibi) başlık, sanatçı ve kapak bildirir.
//! Kullanım: test_player <ses dosyası> [kapak.png] [saniye]
use windows::{
    core::*,
    Foundation::Uri,
    Media::{Core::MediaSource, MediaPlaybackType, Playback::{MediaPlaybackItem, MediaPlayer}},
    Storage::Streams::RandomAccessStreamReference,
    Win32::{System::WinRT::*, UI::WindowsAndMessaging::*},
};

fn uri(path: &str) -> Result<Uri> {
    let p = std::fs::canonicalize(path).map_err(|_| Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
    let s = p.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    Uri::CreateUri(&HSTRING::from(format!("file:///{s}")))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };
    let p = MediaPlayer::new()?;
    // Komut yöneticisi açıkken başlık bilgisi oynatma öğesinden alınır.
    let item = MediaPlaybackItem::Create(&MediaSource::CreateFromUri(&uri(&args[1])?)?)?;
    let props = item.GetDisplayProperties()?;
    props.SetType(MediaPlaybackType::Music)?;
    let mp = props.MusicProperties()?;
    mp.SetTitle(h!("CarlUI Test Şarkısı"))?;
    mp.SetArtist(h!("Carl"))?;
    if let Some(art) = args.get(2) {
        let full = std::fs::canonicalize(art).map_err(|_| Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
        let path = full.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let file = windows::Storage::StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?.get()?;
        props.SetThumbnail(&RandomAccessStreamReference::CreateFromFile(&file)?)?;
    }
    item.ApplyDisplayProperties(&props)?;
    p.SetSource(&item)?;
    p.CommandManager()?.SetIsEnabled(true)?;
    p.SetVolume(0.3)?;
    p.Play()?;
    let secs: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(360);
    unsafe {
        SetTimer(None, 0, secs * 1000, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER {
                break;
            }
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
