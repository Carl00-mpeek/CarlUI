//! Bu modül "hangi modeli nereden, hangi lisansla indiriyoruz?" sorusunun
//! tek kaynağı. Kurucuda hiçbir model yok; kullanıcı Ayarlar → Yapay zekâ'da
//! seçtiği boyutu ister ve dosyalar yalnızca buradaki resmi adreslerden
//! iner. Her dosyanın SHA-256'sı sabittir; indirilen dosya tutmazsa silinir.
//! Öncelik ticari kullanıma izin veren lisanslarda (MIT, Apache-2.0, BSD-3,
//! CC-BY-4.0, kamu malı). Ticari kullanımı yasaklayan bir model ancak başka
//! seçenek yoksa (ör. Türkçe ses) `restricted` işaretiyle eklenir: lisansı
//! ayarlarda turuncu yazılır, ilk indirmeden önce kullanıcı açıkça onaylar.
//! Model kurucuda yoktur; onu resmi kaynaktan indiren kullanıcıdır.

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
    /// Sesli okuma (Piper, Kokoro)
    Speak,
}

impl Feature {
    pub const ALL: [Feature; 6] = [Feature::Speech, Feature::Speak, Feature::Command, Feature::Background, Feature::Upscale, Feature::Translate];
    pub fn key(self) -> &'static str {
        match self {
            Feature::Upscale => "upscale",
            Feature::Background => "background",
            Feature::Speech => "speech",
            Feature::Command => "command",
            Feature::Translate => "translate",
            Feature::Speak => "speak",
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
    /// Zorunlu: tutmazsa dosya kurulmaz (`net::fetch`). Adresler sabit sürüme
    /// bağlıdır; model güncellenirse adres, boyut ve SHA birlikte değiştirilir.
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
    /// Lisans ticari kullanımı yasaklıyor (ör. CC BY-NC). İndirmeden önce kullanıcıya
    /// açıkça sorulur, ayarlarda turuncu yazılır.
    pub restricted: bool,
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


/// Türkçe metni okuyan ses (Sesli okuma'da seçilmez, Türkçede kendiliğinden kullanılır).
pub const TURKISH_VOICE: &str = "piper-dfki-tr";

// Sabit sürümler: dosyalar değişirse SHA tutmaz, bu yüzden commit'e bağlı adresler.
macro_rules! PIPER {
    () => {
        "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4"
    };
}
macro_rules! KOKORO {
    () => {
        "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231"
    };
}
macro_rules! MISAKI {
    () => {
        "https://raw.githubusercontent.com/hexgrad/misaki/fba1236595f2d2bf21d414ba6e57d25256afada3/misaki/data"
    };
}
const MISAKI_GOLD: File = File { url: concat!(MISAKI!(), "/us_gold.json"), name: "us_gold.json", size: 3000469, sha256: "dc414872a49a28ae6c141463d502fd945f3b2fde040484fdc47d00cc4612686f", unzip: false };
const MISAKI_SILVER: File = File { url: concat!(MISAKI!(), "/us_silver.json"), name: "us_silver.json", size: 3099517, sha256: "de8f67be911bb6c659187b4a65fd966b6a30e56350e0f790d763210b053ac475", unzip: false };
const KOKORO_TOKENIZER: File = File { url: concat!(KOKORO!(), "/tokenizer.json"), name: "tokenizer.json", size: 3497, sha256: "77a02c8e164413299b4b4c403b14f8e0e1c1b727db4d46a09d6327b861060a34", unzip: false };
const KOKORO_VOICE: File = File { url: concat!(KOKORO!(), "/voices/af_heart.bin"), name: "voice.bin", size: 522240, sha256: "d583ccff3cdca2f7fae535cb998ac07e9fcb90f09737b9a41fa2734ec44a8f0b", unzip: false };

macro_rules! whisper {
    ($id:literal, $tier:expr, $repo:literal, $rev:literal, $size:literal, $sha:literal, $vocab:literal, $vocab_sha:literal, $cfg:literal, $cfg_sha:literal, $tr:literal, $en:literal, $lic:literal) => {
        Model {
            id: $id,
            feature: Feature::Speech,
            tier: $tier,
            name: concat!("Whisper ", $repo),
            about_tr: $tr,
            about_en: $en,
            license: $lic,
            restricted: false,
            source: concat!("https://huggingface.co/openai/whisper-", $repo),
            files: &[
                File { url: concat!("https://huggingface.co/openai/whisper-", $repo, "/resolve/", $rev, "/model.safetensors"), name: "model.safetensors", size: $size, sha256: $sha, unzip: false },
                File { url: concat!("https://huggingface.co/openai/whisper-", $repo, "/resolve/", $rev, "/vocab.json"), name: "vocab.json", size: $vocab, sha256: $vocab_sha, unzip: false },
                File { url: concat!("https://huggingface.co/openai/whisper-", $repo, "/resolve/", $rev, "/config.json"), name: "config.json", size: $cfg, sha256: $cfg_sha, unzip: false },
            ],
        }
    };
}

pub static MODELS: &[Model] = &[
    // ---- Konuşma → yazı (dikte ve dosya yazıya dökme)
    whisper!("whisper-base", Tier::Small, "base", "e37978b90ca9030d5170a5c07aadb050351a65bb", 290403936, "07cadb9f25677c8d50df603e66a98fbd842cce45047139baeb16e6219a1e807b",
        835550, "8f680bba319e01a653d2e8a5dbc17a9157179e0576e6ce74ce0c06356c6e24f9", 1983, "a153c53883a6799b6f056b4a8d1a515c9926d03994682ba88a7616618d7da0c1",
        "Hızlı, her bilgisayarda çalışır. Kısa dikte için yeterli; Türkçede ara sıra kelime hatası yapar.",
        "Fast, runs on any PC. Fine for short dictation; makes the odd word mistake.", "Apache-2.0 (OpenAI)"),
    whisper!("whisper-small", Tier::Medium, "small", "973afd24965f72e36ca33b3055d56a652f456b4d", 966995080, "1d7734884874f1a1513ed9aa760a4f8e97aaa02fd6d93a3a85d27b2ae9ca596b",
        835550, "8f680bba319e01a653d2e8a5dbc17a9157179e0576e6ce74ce0c06356c6e24f9", 1967, "e6a2b489da1b5aed65a8eb8d1e7466fa867ad5643a8bc138ba708bd56b2875c4",
        "Önerilen. Türkçede belirgin şekilde daha doğru; ekran kartıyla hızlı, yalnız işlemciyle biraz bekletir.",
        "Recommended. Noticeably more accurate; fast with a graphics card, a bit slow on CPU only.", "Apache-2.0 (OpenAI)"),
    whisper!("whisper-turbo", Tier::Large, "large-v3-turbo", "41f01f3fe87f28c78e2fbf8b568835947dd65ed9", 1617824864, "542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1",
        1036558, "e2aa043ef015641d363d8288e7c241c85e36a5c761fb303598e0710233344387", 1256, "c5b526b3e3cd64cd8940dabb45e8ba726629e22d8ed389c29b552f9140daf04a",
        "En doğru sonuç; uzun kayıtlar ve altyazı için. Ekran kartı ve 4 GB+ bellek ister.",
        "Most accurate; for long recordings and subtitles. Wants a graphics card and 4 GB+ of memory.", "MIT (OpenAI)"),
    // ---- Sesli komut
    Model {
        id: "vosk-tr",
        feature: Feature::Command,
        tier: Tier::Small,
        name: "Vosk small TR",
        about_tr: "Anlık tekrar komutları için önerilen. 50 MB, sürekli dinlerken işlemciyi yormaz. Sesli asistan bunun yerine Whisper tiny kullanır.",
        about_en: "Recommended for instant replay commands. 50 MB, barely touches the CPU. The voice assistant uses Whisper tiny instead.",
        license: "Apache-2.0 (Alpha Cephei)",
        restricted: false,
        source: "https://alphacephei.com/vosk/models",
        files: &[
            File { url: "https://alphacephei.com/vosk/models/vosk-model-small-tr-0.3.zip", name: "model.zip", size: 36855784, sha256: "8c8d07cec1bce31add14967c16891c84152cdf76d391e33c08278119b9ea96e5", unzip: true },
            File { url: "https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip", name: "vosk.zip", size: 14882445, sha256: "f1dcc9cca460630f81ea8f71794f69c80bed6556d2a4e6237b5785e1d2dff34b", unzip: true },
        ],
    },
    Model {
        id: "cmd-whisper-tiny",
        feature: Feature::Command,
        tier: Tier::Medium,
        name: "Whisper tiny",
        about_tr: "Sesli asistanın varsayılanı; her dilde, en hafif Whisper. Yalnızca konuşma bitince çalışır.",
        about_en: "The voice assistant default; any language, the lightest Whisper. Only runs once you stop talking.",
        license: "Apache-2.0 (OpenAI)",
        restricted: false,
        source: "https://huggingface.co/openai/whisper-tiny",
        files: &[
            File { url: "https://huggingface.co/openai/whisper-tiny/resolve/169d4a4341b33bc18d8881c4b69c2e104e1cc0af/model.safetensors", name: "model.safetensors", size: 151061672, sha256: "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395", unzip: false },
            File { url: "https://huggingface.co/openai/whisper-tiny/resolve/169d4a4341b33bc18d8881c4b69c2e104e1cc0af/vocab.json", name: "vocab.json", size: 835550, sha256: "8f680bba319e01a653d2e8a5dbc17a9157179e0576e6ce74ce0c06356c6e24f9", unzip: false },
            File { url: "https://huggingface.co/openai/whisper-tiny/resolve/169d4a4341b33bc18d8881c4b69c2e104e1cc0af/config.json", name: "config.json", size: 1983, sha256: "ffdccec4f3211f4c63310f2b7098f309fe70f3952cedc5e4d11e43f5b2379b98", unzip: false },
        ],
    },
    Model {
        id: "cmd-whisper-base",
        feature: Feature::Command,
        tier: Tier::Large,
        name: "Whisper base",
        about_tr: "Daha doğru; asistan komutlarını daha az karıştırır. Dikte için Whisper base indirildiyse ek yer kaplamaz.",
        about_en: "More accurate; the assistant mixes up fewer commands. Takes no extra space if Whisper base is downloaded for dictation.",
        license: "Apache-2.0 (OpenAI)",
        restricted: false,
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
        restricted: false,
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
        restricted: false,
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
        restricted: false,
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
        restricted: false,
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
        restricted: false,
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
        restricted: false,
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
        restricted: false,
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
        restricted: false,
        source: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr",
        files: &[
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/e539fc16a8a1a0ea5950eb339b595bfcce990e90/pytorch_model.bin", name: "model.pth", size: 469780741, sha256: "7bc7e1973431752bcbb083c090c3c8f1e6eb3389d2e2e0faf9bf1a85d969a789", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/e539fc16a8a1a0ea5950eb339b595bfcce990e90/source.spm", name: "source.spm", size: 796953, sha256: "180d3f94ddfee5585d78b702290ded0f4f9bd40f299349c8383b32a579fefad4", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/e539fc16a8a1a0ea5950eb339b595bfcce990e90/vocab.json", name: "vocab.json", size: 1501287, sha256: "01ef522455d2bf22b0716416ee34688cc2b3f5fa257f742665d21b6acc450ac1", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-en-tr/resolve/e539fc16a8a1a0ea5950eb339b595bfcce990e90/config.json", name: "config.json", size: 1137, sha256: "d5ff17d7f5373c403a3ed253d963e5f487a2b95607abdb873429ad4de64db598", unzip: false },
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
        restricted: false,
        source: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en",
        files: &[
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/2261c8fc7b1af59caee87f8ff0ecf3fbccfe8391/model.safetensors", name: "model.safetensors", size: 469717280, sha256: "fbb0eb4c56cf9a6248adbda79c2bbce72cd8969b4b794fbbc434d4b7d995549d", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/2261c8fc7b1af59caee87f8ff0ecf3fbccfe8391/source.spm", name: "source.spm", size: 833378, sha256: "a6946a00a5cbd04044bb1c25b48ea798561a2fd12d872e5d1452bdc3dc076e77", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/2261c8fc7b1af59caee87f8ff0ecf3fbccfe8391/vocab.json", name: "vocab.json", size: 1501287, sha256: "01ef522455d2bf22b0716416ee34688cc2b3f5fa257f742665d21b6acc450ac1", unzip: false },
            File { url: "https://huggingface.co/Helsinki-NLP/opus-mt-tc-big-tr-en/resolve/2261c8fc7b1af59caee87f8ff0ecf3fbccfe8391/config.json", name: "config.json", size: 1076, sha256: "9e03c439d75b5166feb31a01eeedb2befdc01eb41b3541ce37dc8e61eebd9d93", unzip: false },
        ],
    },
    // ---- Sesli okuma (metin → ses)
    Model {
        id: "piper-kristin",
        feature: Feature::Speak,
        tier: Tier::Small,
        name: "Piper Kristin (English)",
        about_tr: "İngilizce kadın sesi; çok hızlı, her bilgisayarda gecikmesiz okur. Biraz robotik.",
        about_en: "English female voice; very fast, reads with no delay on any PC. A little robotic.",
        license: "Public domain (Piper) + Apache-2.0 (misaki)",
        restricted: false,
        source: "https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_US/kristin/medium",
        files: &[
            File { url: concat!(PIPER!(), "/en/en_US/kristin/medium/en_US-kristin-medium.onnx"), name: "model.onnx", size: 63531379, sha256: "5849957f929cbf720c258f8458692d6103fff2f0e3d3b19c8259474bb06a18d4", unzip: false },
            File { url: concat!(PIPER!(), "/en/en_US/kristin/medium/en_US-kristin-medium.onnx.json"), name: "model.onnx.json", size: 4968, sha256: "5681426d4aead22195de70531eeeeddb46493cfaffc5764b2ea3db73428b651c", unzip: false },
            MISAKI_GOLD,
            MISAKI_SILVER,
        ],
    },
    Model {
        id: "kokoro-q8",
        feature: Feature::Speak,
        tier: Tier::Medium,
        name: "Kokoro 82M (English)",
        about_tr: "Önerilen. İnsana çok yakın, doğal İngilizce ses; işlemcide de akıcı okur.",
        about_en: "Recommended. Natural, near-human English voice; reads smoothly even on CPU.",
        license: "Apache-2.0 (hexgrad)",
        restricted: false,
        source: "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX",
        files: &[
            File { url: concat!(KOKORO!(), "/onnx/model_quantized.onnx"), name: "model.onnx", size: 92361116, sha256: "fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478", unzip: false },
            KOKORO_TOKENIZER,
            KOKORO_VOICE,
            MISAKI_GOLD,
            MISAKI_SILVER,
        ],
    },
    Model {
        id: "kokoro-fp32",
        feature: Feature::Speak,
        tier: Tier::Large,
        name: "Kokoro 82M full (English)",
        about_tr: "Aynı ses, sıkıştırılmamış: en temiz ton. Daha çok yer kaplar, biraz daha yavaş.",
        about_en: "The same voice, uncompressed: the cleanest tone. Bigger and a bit slower.",
        license: "Apache-2.0 (hexgrad)",
        restricted: false,
        source: "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX",
        files: &[
            File { url: concat!(KOKORO!(), "/onnx/model.onnx"), name: "model.onnx", size: 325532232, sha256: "8fbea51ea711f2af382e88c833d9e288c6dc82ce5e98421ea61c058ce21a34cb", unzip: false },
            KOKORO_TOKENIZER,
            KOKORO_VOICE,
            MISAKI_GOLD,
            MISAKI_SILVER,
        ],
    },
    Model {
        id: TURKISH_VOICE,
        feature: Feature::Speak,
        tier: Tier::Extra,
        name: "Piper dfki (Türkçe)",
        about_tr: "Türkçe metinler kendiliğinden bu sesle okunur. Açık lisanslı başka Türkçe ses yok.",
        about_en: "Turkish text is read with this voice automatically. No openly licensed Turkish voice exists.",
        license: "CC BY-NC-SA 4.0 (DFKI)",
        restricted: true,
        source: "https://huggingface.co/rhasspy/piper-voices/tree/main/tr/tr_TR/dfki/medium",
        files: &[
            File { url: concat!(PIPER!(), "/tr/tr_TR/dfki/medium/tr_TR-dfki-medium.onnx"), name: "model.onnx", size: 63201294, sha256: "2844717f524ab965d3fe86e60562cbb601d3e456836efcc2196cc3a14112a8fb", unzip: false },
            File { url: concat!(PIPER!(), "/tr/tr_TR/dfki/medium/tr_TR-dfki-medium.onnx.json"), name: "model.onnx.json", size: 4960, sha256: "13ebd7810f1b61b5027583cf3131a0a233b6ea81c38f2200ebc4ff41c3cca039", unzip: false },
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
        Feature::Speak => "kokoro-q8",
    };
    get(id).expect("katalogda önerilen model yok")
}

/// Ayarlarda "Seç" düğmesi var mı: çeviride iki yön de kullanılır, Türkçe ses
/// Türkçe metinde kendiliğinden devreye girer.
pub fn selectable(m: &Model) -> bool {
    m.feature != Feature::Translate && m.id != TURKISH_VOICE
}

/// Ayarlarda seçili model (geçersiz ya da boşsa önerilen).
pub fn chosen(cfg: &carlui_core::config::AiConfig, f: Feature) -> &'static Model {
    get(cfg.chosen(f.key())).filter(|m| m.feature == f && selectable(m)).unwrap_or_else(|| recommended(f))
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
            Feature::Upscale => t!("Resim kalite artırma", "Image quality enhancement"),
            Feature::Background => t!("Arka plan silme", "Background removal"),
            Feature::Speech => t!("Konuşmayı yazıya dökme", "Speech to text"),
            Feature::Command => t!("Sesli komut", "Voice command"),
            Feature::Translate => t!("Ekran çevirisi", "Screen translation"),
            Feature::Speak => t!("Sesli okuma", "Read aloud"),
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
            assert!(!m.files.is_empty() || get("whisper-base").is_some() && m.id == "cmd-whisper-base", "dosyasız model: {}", m.id);
            for f in m.files {
                assert!(crate::net::allowed(f.url), "izinsiz adres: {}", f.url);
                assert!(!f.url.contains("/resolve/main/") && !f.url.contains("/latest/"), "sürüme bağlı değil: {}", f.url);
                if f.url.contains("huggingface.co") {
                    let rev = f.url.split("/resolve/").nth(1).and_then(|r| r.split('/').next()).unwrap_or("");
                    assert!(rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()), "commit'e bağlı değil: {}", f.url);
                }
                assert!(crate::net::valid_sha(f.sha256), "SHA eksik: {}", f.url);
                assert!(crate::net::plain_name(f.name), "dosya adı: {}", f.name);
                assert!(f.size > 0);
            }
            assert_eq!(m.license.contains("NC"), m.restricted, "lisans işareti yanlış: {}", m.id);
        }
    }
}
