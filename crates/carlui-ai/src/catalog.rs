//! Bu modül "hangi modeli nereden, hangi lisansla indiriyoruz?" sorusunun
//! tek kaynağı. Kurucuda hiçbir model yok; kullanıcı Ayarlar → Yapay zekâ'da
//! seçtiği boyutu ister ve dosyalar yalnızca buradaki resmi adreslerden
//! iner. Her dosyanın SHA-256'sı sabittir; indirilen dosya tutmazsa silinir.
//! Lisanslar ticari kullanıma izin verenlerden seçildi (MIT, Apache-2.0,
//! BSD-3, CC-BY-4.0). Ticari kullanımı yasaklayan modeller (ör. RMBG, NLLB)
//! bilerek yok.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    /// Resim büyütme (Real-ESRGAN)
    Upscale,
    /// Arka plan silme (BiRefNet)
    Background,
    /// Konuşmayı yazıya dökme ve dikte (Whisper)
    Speech,
    /// Sesli komut: "klibi kaydet" (Vosk / Whisper)
    Command,
    /// Ekran çevirisi (Opus-MT)
    Translate,
}

impl Feature {
    pub const ALL: [Feature; 5] = [Feature::Speech, Feature::Command, Feature::Background, Feature::Upscale, Feature::Translate];
    pub fn key(self) -> &'static str {
        match self {
            Feature::Upscale => "upscale",
            Feature::Background => "background",
            Feature::Speech => "speech",
            Feature::Command => "command",
            Feature::Translate => "translate",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Small,
    Medium,
    Large,
    Extra,
}

pub struct File {
    pub url: &'static str,
    /// Diskteki adı (model klasöründe)
    pub name: &'static str,
    pub size: u64,
    /// Boşsa doğrulama yapılmaz (yalnızca küçük metin dosyaları).
    pub sha256: &'static str,
    /// Zip ise indirildikten sonra açılır.
    pub unzip: bool,
}

pub struct Model {
    pub id: &'static str,
    pub feature: Feature,
    pub tier: Tier,
    pub name: &'static str,
    pub about_tr: &'static str,
    pub about_en: &'static str,
    pub license: &'static str,
    pub source: &'static str,
    pub files: &'static [File],
}

impl Model {
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
    pub fn about(&self) -> &'static str {
        t!(self.about_tr, self.about_en)
    }
}


macro_rules! whisper {
    ($id:literal, $tier:expr, $repo:literal, $size:literal, $sha:literal, $vocab:literal, $tr:literal, $en:literal, $lic:literal) => {
        Model {
            id: $id,
            feature: Feature::Speech,
            tier: $tier,
            name: concat!("Whisper ", $repo),
            about_tr: $tr,
            about_en: $en,
            license: $lic,
            source: concat!("https://huggingface.co/openai/whisper-", $repo),
            files: &[
                File { url: concat!("https://huggingface.co/openai/whisper-", $repo, "/resolve/main/model.safetensors"), name: "model.safetensors", size: $size, sha256: $sha, unzip: false },
                File { url: concat!("https://huggingface.co/openai/whisper-", $repo, "/resolve/main/vocab.json"), name: "vocab.json", size: $vocab, sha256: "", unzip: false },
                File { url: concat!("https://huggingface.co/openai/whisper-", $repo, "/resolve/main/config.json"), name: "config.json", size: 2000, sha256: "", unzip: false },
            ],
        }
    };
}

pub static MODELS: &[Model] = &[
    // ---- Konuşma → yazı (dikte ve dosya yazıya dökme)
    whisper!("whisper-base", Tier::Small, "base", 290403936, "07cadb9f25677c8d50df603e66a98fbd842cce45047139baeb16e6219a1e807b", 835550,
        "Hızlı, her bilgisayarda çalışır. Kısa dikte için yeterli; Türkçede ara sıra kelime hatası yapar.",
        "Fast, runs on any PC. Fine for short dictation; makes the odd word mistake.", "Apache-2.0 (OpenAI)"),
    whisper!("whisper-small", Tier::Medium, "small", 966995080, "1d7734884874f1a1513ed9aa760a4f8e97aaa02fd6d93a3a85d27b2ae9ca596b", 835550,
        "Önerilen. Türkçede belirgin şekilde daha doğru; ekran kartıyla hızlı, yalnız işlemciyle biraz bekletir.",
        "Recommended. Noticeably more accurate; fast with a graphics card, a bit slow on CPU only.", "Apache-2.0 (OpenAI)"),
    whisper!("whisper-turbo", Tier::Large, "large-v3-turbo", 1617824864, "542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1", 1036558,
        "En doğru sonuç; uzun kayıtlar ve altyazı için. Ekran kartı ve 4 GB+ bellek ister.",
        "Most accurate; for long recordings and subtitles. Wants a graphics card and 4 GB+ of memory.", "MIT (OpenAI)"),
    // ---- Sesli komut
    Model {
        id: "vosk-tr",
        feature: Feature::Command,
        tier: Tier::Small,
        name: "Vosk small TR",
        about_tr: "Önerilen. 35 MB; sürekli dinlerken işlemciyi neredeyse hiç yormaz. Yalnızca komut sözcüklerini tanır.",
        about_en: "Recommended. 35 MB; barely touches the CPU while listening. Only recognizes the command words.",
        license: "Apache-2.0 (Alpha Cephei)",
        source: "https://alphacephei.com/vosk/models",
        files: &[
            File { url: "https://alphacephei.com/vosk/models/vosk-model-small-tr-0.3.zip", name: "model.zip", size: 36_000_000, sha256: "", unzip: true },
            File { url: "https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip", name: "vosk.zip", size: 8_000_000, sha256: "", unzip: true },
        ],
    },
    Model {
        id: "cmd-whisper-tiny",
        feature: Feature::Command,
        tier: Tier::Medium,
        name: "Whisper tiny",
        about_tr: "Daha serbest konuşmayı anlar (\"son iki dakikayı kaydet\"). Yalnızca konuşma algılanınca çalışır.",
        about_en: "Understands freer phrasing (\"save the last two minutes\"). Only runs when speech is detected.",
        license: "Apache-2.0 (OpenAI)",
        source: "https://huggingface.co/openai/whisper-tiny",
        files: &[
            File { url: "https://huggingface.co/openai/whisper-tiny/resolve/main/model.safetensors", name: "model.safetensors", size: 151061672, sha256: "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395", unzip: false },
            File { url: "https://huggingface.co/openai/whisper-tiny/resolve/main/vocab.json", name: "vocab.json", size: 835550, sha256: "", unzip: false },
            File { url: "https://huggingface.co/openai/whisper-tiny/resolve/main/config.json", name: "config.json", size: 2000, sha256: "", unzip: false },
        ],
    },
    Model {
        id: "cmd-whisper-base",
        feature: Feature::Command,
        tier: Tier::Large,
        name: "Whisper base",
        about_tr: "Gürültülü ortamda en iyi tanıma. Dikte için Whisper base indirildiyse ayrıca yer kaplamaz.",
        about_en: "Best recognition in noisy rooms. Takes no extra space if Whisper base is already downloaded for dictation.",
        license: "Apache-2.0 (OpenAI)",
        source: "https://huggingface.co/openai/whisper-base",
        files: &[],
    },
    // ---- Arka plan silme
    Model {
        id: "birefnet-tiny",
        feature: Feature::Background,
        tier: Tier::Small,
        name: "BiRefNet Swin-T",
        about_tr: "Hızlı. Ürün, portre ve basit sahnelerde temiz kenar verir.",
        about_en: "Fast. Clean edges on products, portraits and simple scenes.",
        license: "MIT (ZhengPeng7)",
        source: "https://github.com/ZhengPeng7/BiRefNet",
        files: &[File { url: "https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-bb_swin_v1_tiny-epoch_232.onnx", name: "model.onnx", size: 224005088, sha256: "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333", unzip: false }],
    },
    Model {
        id: "birefnet-general",
        feature: Feature::Background,
        tier: Tier::Medium,
        name: "BiRefNet general",
        about_tr: "Önerilen. Karmaşık sahnelerde en doğru ayırma; ekran kartıyla birkaç saniye.",
        about_en: "Recommended. Most accurate cut-outs on busy scenes; a few seconds on a graphics card.",
        license: "MIT (ZhengPeng7)",
        source: "https://github.com/ZhengPeng7/BiRefNet",
        files: &[File { url: "https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-general-epoch_244.onnx", name: "model.onnx", size: 972666916, sha256: "58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c", unzip: false }],
    },
    Model {
        id: "birefnet-matting",
        feature: Feature::Background,
        tier: Tier::Large,
        name: "BiRefNet matting",
        about_tr: "Saç, tül, duman ve yarı saydam kenarlarda en iyisi. Ekran kartı önerilir.",
        about_en: "Best for hair, tulle, smoke and semi-transparent edges. A graphics card is recommended.",
        license: "MIT (ZhengPeng7)",
        source: "https://github.com/ZhengPeng7/BiRefNet",
        files: &[File { url: "https://github.com/ZhengPeng7/BiRefNet/releases/download/v1/BiRefNet-matting-epoch_100.onnx", name: "model.onnx", size: 972667742, sha256: "6065d27c615ea27308f5b88598dd8db116eb07436c7a323ca40d13b2866c309e", unzip: false }],
    },
    // ---- Resim büyütme
    Model {
        id: "esrgan-general",
        feature: Feature::Upscale,
        tier: Tier::Small,
        name: "Real-ESRGAN general v3",
        about_tr: "5 MB, çok hızlı. Fotoğrafları 4 kat büyütür; ekran görüntüsü ve günlük işler için.",
        about_en: "5 MB, very fast. Upscales photos 4×; for screenshots and everyday use.",
        license: "BSD-3-Clause (xinntao)",
        source: "https://github.com/xinntao/Real-ESRGAN",
        files: &[File { url: "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-x4v3.pth", name: "model.pth", size: 4885111, sha256: "8dc7edb9ac80ccdc30c3a5dca6616509367f05fbc184ad95b731f05bece96292", unzip: false }],
    },
    Model {
        id: "esrgan-x2",
        feature: Feature::Upscale,
        tier: Tier::Medium,
        name: "Real-ESRGAN x2plus",
        about_tr: "2 kat büyütür; büyük fotoğrafları aşırı büyütmeden netleştirmek için.",
        about_en: "Upscales 2×; sharpens large photos without blowing them up too far.",
        license: "BSD-3-Clause (xinntao)",
        source: "https://github.com/xinntao/Real-ESRGAN",
        files: &[File { url: "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.1/RealESRGAN_x2plus.pth", name: "model.pth", size: 67061725, sha256: "49fafd45f8fd7aa8d31ab2a22d14d91b536c34494a5cfe31eb5d89c2fa266abb", unzip: false }],
    },
    Model {
        id: "esrgan-x4",
        feature: Feature::Upscale,
        tier: Tier::Large,
        name: "Real-ESRGAN x4plus",
        about_tr: "Önerilen. En iyi ayrıntı; eski ve düşük çözünürlüklü fotoğraflar için. Daha yavaş.",
        about_en: "Recommended. Best detail, for old and low-resolution photos. Slower.",
        license: "BSD-3-Clause (xinntao)",
        source: "https://github.com/xinntao/Real-ESRGAN",
        files: &[File { url: "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.1.0/RealESRGAN_x4plus.pth", name: "model.pth", size: 67040989, sha256: "4fa0d38905f75ac06eb49a7951b426670021be3018265fd191d2125df9d682f1", unzip: false }],
    },
    Model {
        id: "esrgan-anime",
        feature: Feature::Upscale,
        tier: Tier::Extra,
        name: "Real-ESRGAN anime 6B",
        about_tr: "Çizim, anime ve logolar için; düz renkleri ve çizgileri temiz tutar.",
        about_en: "For drawings, anime and logos; keeps flat colors and lines clean.",
        license: "BSD-3-Clause (xinntao)",
        source: "https://github.com/xinntao/Real-ESRGAN",
        files: &[File { url: "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.2.4/RealESRGAN_x4plus_anime_6B.pth", name: "model.pth", size: 17938799, sha256: "f872d837d3c90ed2e05227bed711af5671a6fd1c9f7d7e91c911a61f155e99da", unzip: false }],
    },
    // ---- Çeviri
    Model {
        id: "opus-en-tr",
        feature: Feature::Translate,
        tier: Tier::Medium,
        name: "Opus-MT tc-big en→tr",
        about_tr: "İngilizceden Türkçeye. Ekrandaki İngilizce metni çevirir.",
        about_en: "English to Turkish. Translates English text on screen.",
        license: "CC-BY-4.0 (Helsinki-NLP, Tiedemann & Thottingal)",
        source: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr",
        files: &[
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/main/pytorch_model.bin", name: "model.pth", size: 469780741, sha256: "7bc7e1973431752bcbb083c090c3c8f1e6eb3389d2e2e0faf9bf1a85d969a789", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/main/source.spm", name: "source.spm", size: 796953, sha256: "180d3f94ddfee5585d78b702290ded0f4f9bd40f299349c8383b32a579fefad4", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/main/vocab.json", name: "vocab.json", size: 1501287, sha256: "", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/main/config.json", name: "config.json", size: 1137, sha256: "", unzip: false },
        ],
    },
    Model {
        id: "opus-tr-en",
        feature: Feature::Translate,
        tier: Tier::Large,
        name: "Opus-MT tc-big tr→en",
        about_tr: "Türkçeden İngilizceye. Kendi yazdığın metni İngilizceye çevirmek için.",
        about_en: "Turkish to English. For translating your own Turkish text into English.",
        license: "CC-BY-4.0 (Helsinki-NLP, Tiedemann & Thottingal)",
        source: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en",
        files: &[
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/main/model.safetensors", name: "model.safetensors", size: 469717280, sha256: "fbb0eb4c56cf9a6248adbda79c2bbce72cd8969b4b794fbbc434d4b7d995549d", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/main/source.spm", name: "source.spm", size: 833378, sha256: "a6946a00a5cbd04044bb1c25b48ea798561a2fd12d872e5d1452bdc3dc076e77", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/main/vocab.json", name: "vocab.json", size: 1501287, sha256: "", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/main/config.json", name: "config.json", size: 1076, sha256: "", unzip: false },
        ],
    },
];

pub fn get(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn of(f: Feature) -> impl Iterator<Item = &'static Model> {
    MODELS.iter().filter(move |m| m.feature == f)
}


/// Ayar boşken kullanılan (önerilen) model.
pub fn recommended(f: Feature) -> &'static Model {
    let id = match f {
        Feature::Upscale => "esrgan-x4",
        Feature::Background => "birefnet-general",
        Feature::Speech => "whisper-small",
        Feature::Command => "vosk-tr",
        Feature::Translate => "opus-en-tr",
    };
    get(id).expect("katalogda önerilen model yok")
}

/// Ayarlarda seçili model (geçersiz ya da boşsa önerilen).
pub fn chosen(cfg: &carlui_core::config::AiConfig, f: Feature) -> &'static Model {
    get(cfg.chosen(f.key())).filter(|m| m.feature == f).unwrap_or_else(|| recommended(f))
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Small => t!("Küçük", "Small"),
            Tier::Medium => t!("Orta", "Medium"),
            Tier::Large => t!("Büyük", "Large"),
            Tier::Extra => t!("Özel", "Special"),
        }
    }
}

impl Feature {
    pub fn title(self) -> &'static str {
        match self {
            Feature::Upscale => t!("Resim büyütme", "Image upscaling"),
            Feature::Background => t!("Arka plan silme", "Background removal"),
            Feature::Speech => t!("Konuşmayı yazıya dökme", "Speech to text"),
            Feature::Command => t!("Sesli komut", "Voice command"),
            Feature::Translate => t!("Ekran çevirisi", "Screen translation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sane() {
        for f in Feature::ALL {
            assert_eq!(recommended(f).feature, f);
        }
        for (i, m) in MODELS.iter().enumerate() {
            assert!(MODELS[..i].iter().all(|o| o.id != m.id), "aynı kimlik: {}", m.id);
            for f in m.files {
                assert!(f.url.starts_with("https://"));
                assert!(f.sha256.is_empty() || f.sha256.len() == 64);
            }
            assert!(!m.license.contains("NC"), "ticari olmayan lisans: {}", m.id);
        }
    }
}
