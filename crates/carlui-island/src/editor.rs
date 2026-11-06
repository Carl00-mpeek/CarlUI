//! Bu modül "panodaki metni yapıştırmadan önce düzenleyeyim" isteğini çözüyor.
//! Ada Composition ile çizildiği için içine klasik bir metin kutusu konamaz;
//! bunun yerine panelden taşan kart (island/clipview.rs) açılınca kartın metin
//! alanının tam üstüne çerçevesiz, kartla aynı renkte bir metin kutusu konur.
//! Başlığı, kenarı, gölgesi yoktur; kullanıcı yalnızca kartın içine yazdığını
//! görür. İçinde Windows'un hazır RichEdit denetimi vardır: seçme,
//! kopyala/yapıştır, Ctrl+Z geri alma, Türkçe klavye, IME hepsi kendiliğinden
//! çalışır. Düz metin kipindedir (yapıştırılan biçim taşınmaz).
//!
//! Ctrl+Enter: kaydet ve önceki pencereye yapıştır · Ctrl+S: kaydet (panel açık
//! kalır) · Esc: vazgeç · başka yere tıklamak: kaydet. Sonuç adaya
//! `WM_APP_EDIT_DONE` ile bildirilir.

use std::{cell::RefCell, sync::Mutex};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::{GetModuleHandleW, LoadLibraryW},
        UI::{Controls::SetWindowTheme, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

pub const WM_APP_EDIT_DONE: u32 = WM_APP + 10;

const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
const EM_SETCHARFORMAT: u32 = WM_USER + 68;
const EM_EXLIMITTEXT: u32 = WM_USER + 53;
const EM_SETTEXTMODE: u32 = WM_USER + 89;
const EM_SETMARGINS: u32 = 0x00D3;
const EM_SETSEL: u32 = 0x00B1;
const TM_PLAINTEXT_UNDO: usize = 1 | 8 | 32;
/// Kartın zeminiyle aynı renk (COLORREF, BGR): Rgba(30, 30, 32), açık temada beyaz.
fn bg() -> u32 {
    if carlui_ui::tone::light() { 0x00FFFFFF } else { 0x00201E1E }
}

#[repr(C)]
struct CharFormatW {
    cb_size: u32,
    dw_mask: u32,
    dw_effects: u32,
    y_height: i32,
    y_offset: i32,
    cr_text_color: u32,
    b_char_set: u8,
    b_pitch_and_family: u8,
    sz_face_name: [u16; 32],
}

pub struct Done {
    pub id: String,
    pub text: String,
    pub paste: bool,
}

pub static RESULT: Mutex<Option<Done>> = Mutex::new(None);

struct Editor {
    hwnd: HWND,
    edit: HWND,
    id: String,
    owner: HWND,
    notify: HWND,
}

thread_local! {
    static ED: RefCell<Option<Editor>> = const { RefCell::new(None) };
}

pub fn is_open() -> bool {
    ED.with(|e| e.borrow().is_some())
}

/// Düzenleyicinin kendi penceresi (açık değilse geçersiz).
pub fn hwnd() -> HWND {
    ED.with(|e| e.borrow().as_ref().map(|ed| ed.hwnd)).unwrap_or_default()
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_SIZE => {
                ED.with(|e| {
                    if let Some(ed) = e.borrow().as_ref() {
                        let mut r = RECT::default();
                        let _ = GetClientRect(hwnd, &mut r);
                        let _ = MoveWindow(ed.edit, 0, 0, r.right, r.bottom, true);
                    }
                });
                LRESULT(0)
            }
            WM_SETFOCUS => {
                ED.with(|e| {
                    if let Some(ed) = e.borrow().as_ref() {
                        let _ = SetFocus(ed.edit);
                    }
                });
                LRESULT(0)
            }
            WM_ERASEBKGND => {
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                let bg = CreateSolidBrush(COLORREF(bg()));
                FillRect(HDC(wp.0 as _), &r, bg);
                let _ = DeleteObject(bg);
                LRESULT(1)
            }
            // Başka yere tıklandı: kaydet, kart kapansın.
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
                finish(Some(false), false);
                LRESULT(0)
            }
            WM_CLOSE => {
                finish(None, true);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Düzenleyiciyi `r` (ekran koordinatı) dikdörtgenine açar. `owner`: kartın
/// penceresi (odak oraya döner), `notify`: sonucu alacak ada penceresi.
pub fn open_at(owner: HWND, notify: HWND, r: RECT, scale: f32, id: String, text: &str) -> bool {
    if is_open() {
        return false;
    }
    unsafe {
        if LoadLibraryW(w!("Msftedit.dll")).is_err() {
            carlui_core::warn!("metin düzenleyici (Msftedit.dll) yüklenemedi");
            return false;
        }
        let Ok(instance) = GetModuleHandleW(None) else { return false };
        let class = w!("CarlUI.ClipEditor");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_IBEAM).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class,
            w!("CarlUI Clipboard editor"),
            WS_POPUP | WS_CLIPCHILDREN,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            owner,
            None,
            instance,
            None,
        ) else {
            return false;
        };
        // Windows 11'in köşe yuvarlama/kenar çizgisi olmasın; köşeleri kart çiziyor.
        let corner: u32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(33), &corner as *const _ as *const _, 4);
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(34), &none as *const _ as *const _, 4);
        let style = WS_CHILD | WS_VISIBLE | WS_VSCROLL | WINDOW_STYLE(0x0004 | 0x0040 | 0x1000 | 0x0100);
        let Ok(edit) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("RICHEDIT50W"), PCWSTR::null(), style, 0, 0, 10, 10, hwnd, None, instance, None) else {
            let _ = DestroyWindow(hwnd);
            return false;
        };
        let _ = SetWindowTheme(edit, if carlui_ui::tone::light() { w!("Explorer") } else { w!("DarkMode_Explorer") }, PCWSTR::null());
        SendMessageW(edit, EM_SETTEXTMODE, WPARAM(TM_PLAINTEXT_UNDO), LPARAM(0));
        SendMessageW(edit, EM_EXLIMITTEXT, WPARAM(0), LPARAM(64 * 1024 * 1024));
        SendMessageW(edit, EM_SETBKGNDCOLOR, WPARAM(0), LPARAM(bg() as isize));
        let mut face = [0u16; 32];
        for (i, u) in "Segoe UI".encode_utf16().enumerate() {
            face[i] = u;
        }
        // Önizlemedeki yazıyla aynı boy (13,5 px ≈ 10 pt = 202 twip).
        let cf = CharFormatW {
            cb_size: std::mem::size_of::<CharFormatW>() as u32,
            dw_mask: 0x4000_0000 | 0x2000_0000 | 0x8000_0000,
            dw_effects: 0,
            y_height: 202,
            y_offset: 0,
            cr_text_color: if carlui_ui::tone::light() { 0x00261814 } else { 0x00EBEBEB },
            b_char_set: 1,
            b_pitch_and_family: 0,
            sz_face_name: face,
        };
        SendMessageW(edit, EM_SETCHARFORMAT, WPARAM(4), LPARAM(&cf as *const _ as isize));
        let m = (8.0 * scale) as isize;
        SendMessageW(edit, EM_SETMARGINS, WPARAM(3), LPARAM(m | m << 16));
        let crlf = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let _ = SetWindowTextW(edit, &HSTRING::from(crlf));
        SendMessageW(edit, EM_SETSEL, WPARAM(0), LPARAM(0));
        ED.with(|e| *e.borrow_mut() = Some(Editor { hwnd, edit, id, owner, notify }));
        let _ = MoveWindow(edit, 0, 0, r.right - r.left, r.bottom - r.top, true);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(edit);
        carlui_core::info!("metin düzenleyici açıldı");
        true
    }
}

fn text_of(edit: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, &mut buf).max(0) as usize;
        let s = String::from_utf16_lossy(&buf[..got]);
        // RichEdit paragrafları \r ile döndürebilir; Windows uygulamaları için \r\n.
        s.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
    }
}

/// `save`: Some(paste) kaydet, None vazgeç. `refocus`: odak kartın penceresine dönsün.
fn finish(save: Option<bool>, refocus: bool) {
    let Some(ed) = ED.with(|e| e.borrow_mut().take()) else { return };
    unsafe {
        if let Some(paste) = save {
            let text = text_of(ed.edit);
            if let Ok(mut r) = RESULT.lock() {
                *r = Some(Done { id: ed.id.clone(), text, paste });
            }
        }
        if refocus {
            let _ = SetForegroundWindow(ed.owner);
        }
        let _ = ShowWindow(ed.hwnd, SW_HIDE);
        let _ = DestroyWindow(ed.hwnd);
        // Kaydedilmese de ada haber alsın (kart kapansın).
        let _ = PostMessageW(ed.notify, WM_APP_EDIT_DONE, WPARAM(save.is_some() as usize), LPARAM(0));
    }
}

/// Kartın düğmeleri: Kaydet (`save`) yeni metni kaydının üstüne yazar,
/// X eski hâline döner (değişiklik atılır). Odak panele döner.
pub fn commit(save: bool) {
    finish(save.then_some(false), true);
}

/// Kartın boş yerine tıklandı: yazmaya devam.
pub fn focus() {
    ED.with(|e| {
        if let Some(ed) = e.borrow().as_ref() {
            unsafe {
                let _ = SetForegroundWindow(ed.hwnd);
                let _ = SetFocus(ed.edit);
            }
        }
    });
}

/// Panel kapanırken: sonucu bildirmeden kapat.
pub fn close() {
    finish(None, false);
}

/// Ana döngüde, mesaj dağıtılmadan önce: düzenleyicinin kısayolları.
pub fn pre_translate(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN {
        return false;
    }
    let mine = ED.with(|e| e.borrow().as_ref().is_some_and(|ed| msg.hwnd == ed.edit || msg.hwnd == ed.hwnd));
    if !mine {
        return false;
    }
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    match VIRTUAL_KEY(msg.wParam.0 as u16) {
        VK_ESCAPE => finish(None, true),
        VK_S if ctrl => finish(Some(false), true),
        VK_RETURN if ctrl => finish(Some(true), false),
        _ => return false,
    }
    true
}

/// Test için: düzenleyicideki metni değiştirip kaydet.
pub fn test_save(text: Option<&str>, paste: bool) {
    let edit = ED.with(|e| e.borrow().as_ref().map(|ed| ed.edit));
    if let (Some(edit), Some(t)) = (edit, text) {
        unsafe {
            let _ = SetWindowTextW(edit, &HSTRING::from(t));
        }
    }
    if edit.is_some() {
        finish(Some(paste), !paste);
    }
}
