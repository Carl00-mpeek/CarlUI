//! Başlatıcı / arama motoru.
//!
//! Bu kütüphane "eski CarlSpotlight'ın yaptığı her şeyi kabuğun içinde, hafif
//! şekilde yap" sorununu çözüyor: uygulama, ayar, konum ve dosya araması;
//! hesap makinesi, birim ve döviz çevirisi; sistem komutları; yapay zekâ
//! kısayolları; web araması ve kullanım sıklığına göre sıralama. Arayüzü üstteki
//! ada çizer (`carlui-island`); burada yalnızca veri ve eylemler vardır.

#[macro_use]
extern crate carlui_core;

pub mod actions;
pub mod app_sound;
mod arrange;
pub mod calc;
pub mod clipsearch;
pub mod currency;
mod download;
mod emoji;
mod fuzzy;
pub mod files;
pub mod index;
mod openwin;
pub mod quickset;
mod recent;
pub mod search;
mod spotify;
pub mod timer;
mod translate;
pub mod usage;
pub mod voice;
pub mod voice_lines;
mod voice_rate;
mod whatsapp;
