//! Bu modül "araç işini bitirince kullanıcı sonucu nereden görecek?"
//! sorununu çözüyor: kısa bir bildirim olay yolu üzerinden adaya gönderilir
//! (ör. "#1E90FF panoya kopyalandı"). CarlUI çalışmıyorsa sessizce atlanır.

use carlui_core::bus::{BusClient, Event};

pub fn island(app: &str, title: &str, body: &str) {
    let data = serde_json::json!({ "app": app, "title": title, "body": body });
    BusClient::send_once(&Event::Custom { name: "island-note".into(), data });
}
