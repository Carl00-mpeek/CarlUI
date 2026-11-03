//! İmleç üreticisini dener: birkaç görünüm için tüm rollerin .cur/.ani
//! dosyalarını üretir ve hepsini koyu/açık zeminde gösteren bir önizleme
//! (BMP) yazar. Kullanım: cargo run -p carlui-core --example cursors -- <klasör>

use carlui_core::config::{CursorConfig, CursorStyle};
use carlui_core::cursor::{self, draw, shapes::Role};

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out).unwrap();
    let looks = [
        CursorConfig::default(),
        CursorConfig { style: CursorStyle::Ink, ..Default::default() },
        CursorConfig { style: CursorStyle::Soft, ..Default::default() },
        CursorConfig { style: CursorStyle::Zen, ..Default::default() },
        CursorConfig { style: CursorStyle::Zen, glow: false, ..Default::default() },
    ];
    let size = 64u32;
    let cell = size + 8;
    let cols = Role::ALL.len() as u32;
    let (w, h) = (cell * cols, cell * looks.len() as u32 * 2);
    let mut sheet = vec![0u8; (w * h * 3) as usize];
    let r = draw::Renderer::new(size).unwrap();
    for (li, cfg) in looks.iter().enumerate() {
        let l = cursor::look(cfg);
        let dir = out.join(format!("set{li}"));
        std::fs::create_dir_all(&dir).unwrap();
        for (ci, role) in Role::ALL.into_iter().enumerate() {
            std::fs::write(dir.join(role.file()), cursor::encode(&r, role, &l, 32).unwrap_or_default()).unwrap();
            let px = r.render(role, 3, &l).unwrap();
            for light in [false, true] {
                let bg: [f32; 3] = if light { [0.93, 0.93, 0.95] } else { [0.13, 0.13, 0.16] };
                let row = li as u32 * 2 + light as u32;
                for y in 0..cell {
                    for x in 0..cell {
                        let (sx, sy) = (x as i32 - 4, y as i32 - 4);
                        let mut c = bg;
                        if sx >= 0 && sy >= 0 && (sx as u32) < size && (sy as u32) < size {
                            let i = ((sy as u32 * size + sx as u32) * 4) as usize;
                            let al = px[i + 3] as f32 / 255.0;
                            for k in 0..3 {
                                c[k] = c[k] * (1.0 - al) + px[i + 2 - k] as f32 / 255.0 * al;
                            }
                        }
                        let (gx, gy) = (ci as u32 * cell + x, row * cell + y);
                        let j = (((h - 1 - gy) * w + gx) * 3) as usize;
                        sheet[j] = (c[2] * 255.0) as u8;
                        sheet[j + 1] = (c[1] * 255.0) as u8;
                        sheet[j + 2] = (c[0] * 255.0) as u8;
                    }
                }
            }
        }
    }
    let mut bmp = Vec::new();
    let data = sheet.len() as u32;
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(54 + data).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(w as i32).to_le_bytes());
    bmp.extend_from_slice(&(h as i32).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 24]);
    bmp.extend_from_slice(&sheet);
    std::fs::write(out.join("onizleme.bmp"), bmp).unwrap();
    println!("tamam: {}", out.display());
}
