//! JSON dışa aktarımları: Bitwarden (şifresiz), Aegis (şifresiz), 2FAS, andOTP.
//! Hangisi olduğu kök nesnenin anahtarlarından anlaşılır.

use super::{card, identity, login, note, otpauth, two_factor, Batch, Card, ImportError, Login};
use serde_json::Value;

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn n(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0)
}

pub fn parse(t: &str) -> Result<Batch, ImportError> {
    let v: Value = serde_json::from_str(t).map_err(|_| ImportError::Unknown)?;
    if let Some(items) = v.get("items").and_then(Value::as_array) {
        if v.get("encrypted").and_then(Value::as_bool) == Some(true) {
            return Err(ImportError::Encrypted("Bitwarden"));
        }
        return Ok(bitwarden(items));
    }
    if v.get("encrypted").and_then(Value::as_bool) == Some(true) || v.get("encKeyValidation_DO_NOT_EDIT").is_some() {
        return Err(ImportError::Encrypted("Bitwarden"));
    }
    if let Some(db) = v.get("db") {
        let Some(entries) = db.get("entries").and_then(Value::as_array) else { return Err(ImportError::Encrypted("Aegis")) };
        return Ok(aegis(entries));
    }
    if v.get("servicesEncrypted").is_some() {
        return Err(ImportError::Encrypted("2FAS"));
    }
    if let Some(services) = v.get("services").and_then(Value::as_array) {
        return Ok(twofas(services));
    }
    if let Some(list) = v.as_array() {
        if list.iter().any(|e| e.get("secret").is_some()) {
            return Ok(andotp(list));
        }
    }
    Err(ImportError::Unknown)
}

fn bitwarden(items: &[Value]) -> Batch {
    let mut b = Batch { source: "Bitwarden (JSON)".into(), items: Vec::new(), skipped: 0 };
    for it in items {
        let name = s(it, "name");
        let mut notes = s(it, "notes").to_string();
        // Özel alanlar nota eklenir ("ad: değer").
        for f in it.get("fields").and_then(Value::as_array).into_iter().flatten() {
            notes.push_str(&format!("\n{}: {}", s(f, "name"), s(f, "value")));
        }
        let notes = notes.trim();
        match n(it, "type") {
            1 => {
                let l = it.get("login").cloned().unwrap_or_default();
                let url = l.get("uris").and_then(Value::as_array).and_then(|u| u.first()).map(|u| s(u, "uri")).unwrap_or("");
                b.items.push(login(Login { name, url, user: s(&l, "username"), password: s(&l, "password"), totp: s(&l, "totp"), notes }));
            }
            2 => b.items.push(note(name, notes)),
            3 => {
                let c = it.get("card").cloned().unwrap_or_default();
                let (month, year) = (n(&c, "expMonth").to_string(), s(&c, "expYear").to_string());
                b.items.push(card(Card { name, holder: s(&c, "cardholderName"), number: s(&c, "number"), month: &month, year: &year, cvv: s(&c, "code"), notes }));
            }
            4 => {
                let i = it.get("identity").cloned().unwrap_or_default();
                let full = [s(&i, "title"), s(&i, "firstName"), s(&i, "middleName"), s(&i, "lastName")].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" ");
                let id_no = [s(&i, "ssn"), s(&i, "passportNumber"), s(&i, "licenseNumber")].into_iter().find(|x| !x.is_empty()).unwrap_or("");
                let mut extra = Vec::new();
                for (k, label) in [("email", "E-posta"), ("phone", "Telefon"), ("address1", "Adres"), ("city", "Şehir"), ("country", "Ülke"), ("passportNumber", "Pasaport"), ("licenseNumber", "Ehliyet")] {
                    if !s(&i, k).is_empty() {
                        extra.push(format!("{label}: {}", s(&i, k)));
                    }
                }
                if !notes.is_empty() {
                    extra.push(notes.to_string());
                }
                b.items.push(identity(name, &full, id_no, &extra.join("\n")));
            }
            _ => b.skipped += 1,
        }
    }
    b
}

#[allow(clippy::too_many_arguments)]
/// 2FA girdisini ekler; TOTP değilse ya da anahtar geçersizse atlar.
fn push_totp(b: &mut Batch, issuer: &str, account: &str, secret: &str, digits: u64, period: u64, algo: &str, kind: &str) {
    if !(kind.is_empty() || kind.eq_ignore_ascii_case("totp")) {
        b.skipped += 1;
        return;
    }
    let uri = otpauth(issuer, account, secret, digits as u32, period, algo);
    if crate::totp::Totp::parse(&uri).is_err() {
        b.skipped += 1;
        return;
    }
    b.items.push(two_factor(issuer, account, &uri));
}

fn aegis(entries: &[Value]) -> Batch {
    let mut b = Batch { source: "Aegis".into(), items: Vec::new(), skipped: 0 };
    for e in entries {
        let info = e.get("info").cloned().unwrap_or_default();
        push_totp(&mut b, s(e, "issuer"), s(e, "name"), s(&info, "secret"), n(&info, "digits"), n(&info, "period"), s(&info, "algo"), s(e, "type"));
    }
    b
}

fn twofas(services: &[Value]) -> Batch {
    let mut b = Batch { source: "2FAS".into(), items: Vec::new(), skipped: 0 };
    for e in services {
        let otp = e.get("otp").cloned().unwrap_or_default();
        let issuer = if s(&otp, "issuer").is_empty() { s(e, "name") } else { s(&otp, "issuer") };
        let account = if s(&otp, "account").is_empty() { s(&otp, "label") } else { s(&otp, "account") };
        push_totp(&mut b, issuer, account, s(e, "secret"), n(&otp, "digits"), n(&otp, "period"), s(&otp, "algorithm"), s(&otp, "tokenType"));
    }
    b
}

fn andotp(list: &[Value]) -> Batch {
    let mut b = Batch { source: "andOTP".into(), items: Vec::new(), skipped: 0 };
    for e in list {
        let label = s(e, "label");
        let (issuer, account) = match (s(e, "issuer"), label.split_once(':')) {
            ("", Some((i, a))) => (i.trim(), a.trim()),
            (i, _) => (i, label),
        };
        push_totp(&mut b, issuer, account, s(e, "secret"), n(e, "digits"), n(e, "period"), s(e, "algorithm"), s(e, "type"));
    }
    b
}

#[cfg(test)]
mod tests {
    use super::super::{parse as top, ImportError};
    use crate::entry::{Category, FieldKind};

    #[test]
    fn bitwarden_all_types() {
        let j = r#"{"encrypted":false,"items":[
            {"type":1,"name":"GitHub","notes":null,"login":{"username":"carl","password":"s3cret","totp":"otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP","uris":[{"uri":"https://github.com"}]},"fields":[{"name":"PIN","value":"42"}]},
            {"type":2,"name":"Not","notes":"gizli not"},
            {"type":3,"name":"Ziraat","card":{"cardholderName":"CARL","number":"4111111111111111","expMonth":"4","expYear":"2027","code":"123"}},
            {"type":4,"name":"Kimlik","identity":{"firstName":"Carl","lastName":"Test","ssn":"12345678901","email":"c@o.com"}},
            {"type":5,"name":"SSH"}
        ]}"#;
        let b = top(j.as_bytes()).unwrap();
        assert_eq!(b.items.len(), 4);
        assert_eq!(b.skipped, 1);
        let cats: Vec<_> = b.items.iter().map(|i| i.meta.category).collect();
        assert_eq!(cats, [Category::Password, Category::Note, Category::Card, Category::Identity]);
        let gh = &b.items[0].body;
        assert!(gh.fields.iter().any(|f| f.kind == FieldKind::Totp && f.value.contains("JBSWY3DP")));
        assert!(gh.fields.iter().any(|f| f.kind == FieldKind::Multiline && f.value.contains("PIN: 42")));
        assert_eq!(crate::card::Part::MmYy.value(&b.items[2].body).unwrap().as_str(), "04/27");
        assert_eq!(b.items[3].body.fields[0].value, "Carl Test");
        assert_eq!(b.items[3].body.fields[1].value, "12345678901");
    }

    #[test]
    fn encrypted_exports() {
        assert_eq!(top(br#"{"encrypted":true,"items":[]}"#).err(), Some(ImportError::Encrypted("Bitwarden")));
        assert_eq!(top(br#"{"version":1,"db":"base64..."}"#).err(), Some(ImportError::Encrypted("Aegis")));
        assert_eq!(top(br#"{"services":[],"servicesEncrypted":"x"}"#).err(), Some(ImportError::Encrypted("2FAS")));
        assert_eq!(top(br#"{"hello":1}"#).err(), Some(ImportError::Unknown));
    }

    #[test]
    fn authenticators() {
        let aegis = r#"{"version":1,"db":{"version":2,"entries":[
            {"type":"totp","name":"carl","issuer":"GitHub","info":{"secret":"JBSWY3DPEHPK3PXP","algo":"SHA1","digits":6,"period":30}},
            {"type":"steam","name":"x","issuer":"Steam","info":{"secret":"JBSWY3DPEHPK3PXP"}},
            {"type":"totp","name":"y","issuer":"Bozuk","info":{"secret":"1"}}]}}"#;
        let b = top(aegis.as_bytes()).unwrap();
        assert_eq!((b.source.as_str(), b.items.len(), b.skipped), ("Aegis", 1, 2));
        assert_eq!(b.items[0].meta.name, "GitHub");

        let twofas = r#"{"services":[{"name":"Discord","secret":"JBSWY3DPEHPK3PXP","otp":{"account":"carl","digits":8,"period":30,"algorithm":"SHA256","tokenType":"TOTP"}}],"schemaVersion":4}"#;
        let b = top(twofas.as_bytes()).unwrap();
        let t = crate::totp::Totp::parse(&b.items[0].body.fields[1].value).unwrap();
        assert_eq!((t.digits, t.algo), (8, crate::totp::Algo::Sha256));

        let andotp = r#"[{"secret":"JBSWY3DPEHPK3PXP","label":"Google:carl@gmail.com","digits":6,"type":"TOTP","algorithm":"SHA1","period":30}]"#;
        let b = top(andotp.as_bytes()).unwrap();
        assert_eq!(b.items[0].meta.name, "Google");
        assert_eq!(b.items[0].body.fields[0].value, "carl@gmail.com");
    }
}
