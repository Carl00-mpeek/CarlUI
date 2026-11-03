//! Tanı: bildirim dinleyicisi ve medya oturumları paketlenmemiş bir exe'den çalışıyor mu?
use windows::{
    Media::Control::GlobalSystemMediaTransportControlsSessionManager as Mgr,
    UI::Notifications::{Management::UserNotificationListener, NotificationKinds},
    Win32::System::WinRT::*,
};

fn volumes() {
    use windows::Win32::{
        Media::Audio::{Endpoints::IAudioEndpointVolume, *},
        System::Com::*,
    };
    use windows::core::Interface;
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
        let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia).unwrap();
        let ep: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).unwrap();
        println!("sistem ana sesi: {:.2}", ep.GetMasterVolumeLevelScalar().unwrap_or(-1.0));
        let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None).unwrap();
        let list = mgr.GetSessionEnumerator().unwrap();
        for i in 0..list.GetCount().unwrap_or(0) {
            let c = list.GetSession(i).unwrap();
            let pid = c.cast::<IAudioSessionControl2>().and_then(|c| c.GetProcessId()).unwrap_or(0);
            let v = c.cast::<ISimpleAudioVolume>().and_then(|v| v.GetMasterVolume()).unwrap_or(-1.0);
            println!("  ses oturumu pid {pid}: {v:.2}");
        }
    }
}

fn main() {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    volumes();
    match UserNotificationListener::Current() {
        Ok(l) => {
            println!("listener ok, access: {:?}", l.GetAccessStatus().map(|s| s.0));
            match l.RequestAccessAsync().and_then(|op| op.get()) {
                Ok(s) => println!("request: {}", s.0),
                Err(e) => println!("request err: {e}"),
            }
            match l.GetNotificationsAsync(NotificationKinds::Toast).and_then(|op| op.get()) {
                Ok(list) => {
                    println!("bildirim sayısı: {}", list.Size().unwrap_or(0));
                    for n in list {
                        let app = n.AppInfo().and_then(|a| a.DisplayInfo()).and_then(|d| d.DisplayName()).unwrap_or_default();
                        println!("  - {} ({})", app, n.Id().unwrap_or(0));
                    }
                }
                Err(e) => println!("GetNotifications err: {e}"),
            }
        }
        Err(e) => println!("listener err: {e}"),
    }
    match Mgr::RequestAsync().and_then(|op| op.get()) {
        Ok(m) => {
            let sessions = m.GetSessions().unwrap();
            println!("medya oturumları: {}", sessions.Size().unwrap_or(0));
            for s in sessions {
                let id = s.SourceAppUserModelId().unwrap_or_default();
                let p = s.TryGetMediaPropertiesAsync().and_then(|op| op.get());
                if let Ok(props) = &p {
                    match props.Thumbnail() {
                        Ok(t) => match t.OpenReadAsync().and_then(|op| op.get()) {
                            Ok(st) => println!("  kapak akışı: {} bayt, tür {:?}", st.Size().unwrap_or(0), st.ContentType()),
                            Err(e) => println!("  kapak açılamadı: {e}"),
                        },
                        Err(e) => println!("  kapak yok: {e}"),
                    }
                }
                let (t, a) = p.map(|p| (p.Title().unwrap_or_default(), p.Artist().unwrap_or_default())).unwrap_or_default();
                let st = s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).map(|x| x.0).unwrap_or(-1);
                let tl = s.GetTimelineProperties().ok();
                let pos = tl.as_ref().and_then(|t| t.Position().ok()).map(|d| d.Duration / 10_000_000).unwrap_or(-1);
                let end = tl.as_ref().and_then(|t| t.EndTime().ok()).map(|d| d.Duration / 10_000_000).unwrap_or(-1);
                println!("  - [{id}] {t} — {a} durum={st} {pos}/{end} sn");
            }
        }
        Err(e) => println!("medya err: {e}"),
    }
}
