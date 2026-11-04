//! CSV dışa aktarımları. Her şifre yöneticisi sütunlara başka ad verir
//! ("login_username", "Username", "email" …); sütunlar adlarından tanınır,
//! sıraları önemsizdir. Ayırıcı (virgül / noktalı virgül / sekme) ilk satırdan
//! bulunur; tırnak içindeki virgül ve satır sonu korunur.

use super::{card, login, note, two_factor, Batch, Card, Imported, ImportError, Login};
use zeroize::Zeroizing;

type Row = Vec<Zeroizing<String>>;

/// RFC 4180 CSV okuyucu.
fn records(t: &str, sep: char) -> Vec<Row> {
    let (mut rows, mut row, mut cell) = (Vec::new(), Row::new(), Zeroizing::new(String::new()));
    let (mut quoted, mut chars) = (false, t.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            (true, '"') => quoted = false,
            (true, _) => cell.push(c),
            (false, '"') if cell.is_empty() => quoted = true,
            (false, c) if c == sep => row.push(std::mem::replace(&mut cell, Zeroizing::new(String::new()))),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::replace(&mut cell, Zeroizing::new(String::new())));
                rows.push(std::mem::take(&mut row));
            }
            (false, _) => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    rows
}

/// İlk satırda tırnak dışında en çok geçen ayırıcı.
fn separator(t: &str) -> char {
    let first = t.lines().next().unwrap_or("");
    let mut quoted = false;
    let mut n = [0usize; 3];
    for c in first.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => n[0] += 1,
            ';' if !quoted => n[1] += 1,
            '\t' if !quoted => n[2] += 1,
            _ => {}
        }
    }
    let i = (0..3).max_by_key(|&i| n[i]).unwrap_or(0);
    [',', ';', '\t'][i]
}

fn norm(h: &str) -> String {
    h.trim().trim_matches('"').to_lowercase().replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sütunlar: her anlam için olası adlar (normalleştirilmiş).
#[derive(Default)]
struct Cols {
    name: Option<usize>,
    url: Option<usize>,
    user: Option<usize>,
    email: Option<usize>,
    password: Option<usize>,
    totp: Option<usize>,
    notes: Option<usize>,
    kind: Option<usize>,
    fields: Option<usize>,
    holder: Option<usize>,
    number: Option<usize>,
    cvv: Option<usize>,
    expiry: Option<usize>,
    month: Option<usize>,
    year: Option<usize>,
}

fn find(h: &[String], names: &[&str]) -> Option<usize> {
    names.iter().find_map(|n| h.iter().position(|x| x == n))
}

impl Cols {
    fn of(h: &[String]) -> Self {
        Cols {
            name: find(h, &["name", "title", "item name", "account name", "account"]),
            url: find(h, &["url", "login uri", "website", "web site", "uri", "urls", "login url", "site", "hostname"]),
            user: find(h, &["username", "login username", "user name", "login", "user", "login name"]),
            email: find(h, &["email", "e mail", "email address"]),
            password: find(h, &["password", "login password", "pass"]),
            totp: find(h, &["totp", "login totp", "otpauth", "otp auth", "otp", "otpsecret", "otp secret", "2fa", "one time password", "authenticator key"]),
            notes: find(h, &["notes", "note", "extra", "comments", "comment"]),
            kind: find(h, &["type", "item type"]),
            fields: find(h, &["fields", "custom fields"]),
            holder: find(h, &["cardholdername", "cardholder name", "card holder", "card holder name", "name on card"]),
            number: find(h, &["cardnumber", "card number", "number"]),
            cvv: find(h, &["cvc", "cvv", "card cvv", "security code", "verification number"]),
            expiry: find(h, &["expirydate", "expiry date", "expiration", "expiration date", "expiry"]),
            month: find(h, &["exp month", "expmonth", "expiry month", "expiration month"]),
            year: find(h, &["exp year", "expyear", "expiry year", "expiration year"]),
        }
    }

    fn known(&self) -> bool {
        self.password.is_some() || self.number.is_some() || (self.totp.is_some() && (self.name.is_some() || self.user.is_some()))
    }
}

/// Başlıklardan kaynağın adı (önizlemede gösterilir).
fn source(h: &[String]) -> &'static str {
    let has = |n: &str| h.iter().any(|x| x == n);
    if has("login uri") || has("login username") {
        "Bitwarden"
    } else if has("httprealm") || has("formactionorigin") {
        "Firefox"
    } else if has("grouping") {
        "LastPass"
    } else if has("otpauth") {
        "1Password"
    } else if has("group") && has("title") {
        "KeePass"
    } else if has("vault") && has("createtime") {
        "Proton Pass"
    } else if has("username2") || has("otpsecret") {
        "Dashlane"
    } else if has("cardholdername") || has("additional urls") {
        "NordPass"
    } else if h.len() == 5 && has("name") && has("url") && has("username") && has("password") && has("note") {
        "Chrome / Edge / Brave"
    } else {
        "CSV"
    }
}

pub fn parse(t: &str) -> Result<Batch, ImportError> {
    let rows = records(t, separator(t));
    let Some((head, body)) = rows.split_first() else { return Err(ImportError::Empty) };
    let h: Vec<String> = head.iter().map(|c| norm(c)).collect();
    let c = Cols::of(&h);
    if !c.known() {
        return Err(ImportError::Unknown);
    }
    let mut b = Batch { source: format!("{} (CSV)", source(&h)), items: Vec::new(), skipped: 0 };
    for r in body {
        match row(&c, r) {
            Some(Some(i)) => b.items.push(i),
            Some(None) => {}
            None => b.skipped += 1,
        }
    }
    Ok(b)
}

/// Satırdan kayıt. Some(None): boş satır (sayılmaz); None: desteklenmeyen tür.
fn row(c: &Cols, r: &Row) -> Option<Option<Imported>> {
    let g = |i: Option<usize>| i.and_then(|i| r.get(i)).map(|s| s.as_str()).unwrap_or("");
    let kind = g(c.kind).trim().to_lowercase().replace([' ', '_', '-'], "");
    let (name, url, password, totp) = (g(c.name), g(c.url), g(c.password), g(c.totp));
    let user = if g(c.user).trim().is_empty() { g(c.email) } else { g(c.user) };
    let mut notes = Zeroizing::new(g(c.notes).trim().to_string());
    if !g(c.fields).trim().is_empty() {
        if !notes.is_empty() {
            notes.push_str("\n\n");
        }
        notes.push_str(g(c.fields).trim());
    }
    let number = g(c.number);
    match kind.as_str() {
        "" | "login" | "password" | "1" => {}
        "note" | "securenote" | "2" => return Some(Some(note(name, &notes))),
        "card" | "creditcard" | "3" => {}
        _ => return None,
    }
    // Firefox'un kendi hesabı gibi tarayıcı içi adresler kayıt değildir.
    if url.trim().starts_with("chrome://") || url.trim().starts_with("about:") {
        return Some(None);
    }
    if !number.trim().is_empty() || kind == "card" || kind == "creditcard" || kind == "3" {
        let (mut month, mut year) = (g(c.month).to_string(), g(c.year).to_string());
        if let Some((m, y)) = crate::card::expiry(g(c.expiry)) {
            (month, year) = (m.to_string(), y.to_string());
        }
        return Some(Some(card(Card { name, holder: g(c.holder), number, month: &month, year: &year, cvv: g(c.cvv), notes: &notes })));
    }
    let empty = |s: &str| s.trim().is_empty();
    if empty(user) && empty(password) && empty(url) && empty(totp) {
        return Some(if empty(&notes) { None } else { Some(note(name, &notes)) });
    }
    if empty(password) && empty(url) && !empty(totp) {
        let (issuer, account) = crate::totp::names(totp);
        let issuer = if issuer.is_empty() { name.trim().to_string() } else { issuer };
        let account = if account.is_empty() { user.trim().to_string() } else { account };
        return Some(Some(two_factor(&issuer, &account, totp)));
    }
    Some(Some(login(Login { name, url, user, password, totp, notes: &notes })))
}

#[cfg(test)]
mod tests {
    use super::super::parse as top;
    use crate::entry::{Category, FieldKind};

    fn val(i: &super::Imported, kind: FieldKind) -> String {
        i.body.fields.iter().find(|f| f.kind == kind).map(|f| f.value.clone()).unwrap_or_default()
    }

    #[test]
    fn chrome() {
        let b = top(b"name,url,username,password,note\ngithub.com,https://github.com/login,carl,\"p,a\"\"ss\",\"iki\nsatir\"\n,https://www.gitlab.com/,carl2,x,\n").unwrap();
        assert_eq!(b.source, "Chrome / Edge / Brave (CSV)");
        assert_eq!(b.items.len(), 2);
        assert_eq!(val(&b.items[0], FieldKind::Secret), "p,a\"ss", "tırnak içi virgül ve kaçış");
        assert_eq!(val(&b.items[0], FieldKind::Multiline), "iki\nsatir");
        assert_eq!(b.items[1].meta.name, "gitlab.com", "ad yoksa adresin makinesi");
        assert_eq!(b.items[0].meta.category, Category::Password);
    }

    #[test]
    fn bitwarden_with_totp_and_note() {
        let csv = "\u{FEFF}folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
                   ,,login,GitHub,,,0,https://github.com,carl,s3cret,JBSWY3DPEHPK3PXP\n\
                   ,,note,Wi-Fi,ev parolası 1234,,0,,,,\n\
                   ,,identity,Kimlik,,,0,,,,\n";
        let b = top(csv.as_bytes()).unwrap();
        assert_eq!(b.source, "Bitwarden (CSV)");
        assert_eq!(b.items.len(), 2);
        assert_eq!(b.skipped, 1, "kimlik CSV'de desteklenmez");
        assert_eq!(val(&b.items[0], FieldKind::Totp), "JBSWY3DPEHPK3PXP");
        assert_eq!(b.items[1].meta.category, Category::Note);
    }

    #[test]
    fn firefox_semicolon_and_cards() {
        let ff = "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\"\n\"https://a.com\",\"u\",\"p\",,\"\",\"{1}\"\n\"chrome://FirefoxAccounts\",\"x\",\"y\",,,\n";
        let b = top(ff.as_bytes()).unwrap();
        assert_eq!(b.source, "Firefox (CSV)");
        assert_eq!(b.items.len(), 1, "tarayıcı içi hesap atlanır");
        let semi = top(b"Title;Username;Password;URL\nX;u;p;https://x.com\n").unwrap();
        assert_eq!(semi.items[0].meta.name, "X");
        let cards = top(b"name,type,cardholdername,cardnumber,cvc,expirydate\nKart,credit_card,CARL,4111 1111 1111 1111,123,04/27\n").unwrap();
        let c = &cards.items[0];
        assert_eq!(c.meta.category, Category::Card);
        assert_eq!(crate::card::Part::Number.value(&c.body).unwrap().as_str(), "4111111111111111");
        assert_eq!(crate::card::Part::MmYy.value(&c.body).unwrap().as_str(), "04/27");
    }

    #[test]
    fn totp_only_rows() {
        let b = top(b"name,username,totp\nDiscord,carl,otpauth://totp/Discord:carl?secret=JBSWY3DPEHPK3PXP\n").unwrap();
        assert_eq!(b.items[0].meta.category, Category::TwoFactor);
        assert_eq!(b.items[0].meta.name, "Discord");
    }
}
