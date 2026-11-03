//! Bu modül "kopyaladığım metni yapıştırmadan önce düzelteyim" sorununu çözüyor:
//! Türkçe harflere doğru davranan büyük/küçük harf ve başlık düzeni, fazla
//! boşlukları ve boş satırları temizleme, satırları birleştirme.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Upper,
    Lower,
    Title,
    Tidy,
    JoinLines,
    RemoveEmptyLines,
}

pub const ALL: &[(Op, &str, char)] = &[
    (Op::Upper, "BÜYÜK HARF", '\u{E8D2}'),
    (Op::Lower, "küçük harf", '\u{E8D2}'),
    (Op::Title, "Başlık Düzeni", '\u{E8D2}'),
    (Op::Tidy, "Fazla boşlukları temizle", '\u{E75C}'),
    (Op::JoinLines, "Satırları birleştir", '\u{E8C9}'),
    (Op::RemoveEmptyLines, "Boş satırları sil", '\u{E8C9}'),
];

/// İşlemin seçili dildeki adı.
pub fn name(op: Op) -> &'static str {
    match op {
        Op::Upper => t!("BÜYÜK HARF", "UPPERCASE"),
        Op::Lower => t!("küçük harf", "lowercase"),
        Op::Title => t!("Başlık Düzeni", "Title Case"),
        Op::Tidy => t!("Fazla boşlukları temizle", "Remove extra spaces"),
        Op::JoinLines => t!("Satırları birleştir", "Join lines"),
        Op::RemoveEmptyLines => t!("Boş satırları sil", "Remove empty lines"),
    }
}

fn upper_tr(c: char) -> String {
    match c {
        'i' => "İ".into(),
        'ı' => "I".into(),
        _ => c.to_uppercase().collect(),
    }
}

fn lower_tr(c: char) -> String {
    match c {
        'I' => "ı".into(),
        'İ' => "i".into(),
        _ => c.to_lowercase().collect(),
    }
}

pub fn apply(op: Op, s: &str) -> String {
    match op {
        Op::Upper => s.chars().map(upper_tr).collect(),
        Op::Lower => s.chars().map(lower_tr).collect(),
        Op::Title => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                out.push_str(&if start { upper_tr(c) } else { lower_tr(c) });
                start = c.is_whitespace() || c == '-' || c == '(';
            }
            out
        }
        Op::Tidy => s
            .lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        Op::JoinLines => s.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "),
        Op::RemoveEmptyLines => s.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkish() {
        assert_eq!(apply(Op::Upper, "istanbul ılık"), "İSTANBUL ILIK");
        assert_eq!(apply(Op::Lower, "İSTANBUL ILIK"), "istanbul ılık");
        assert_eq!(apply(Op::Title, "iyi GÜNLER ılgaz"), "İyi Günler Ilgaz");
        assert_eq!(apply(Op::Tidy, "  a   b \n\t c  "), "a b\nc");
        assert_eq!(apply(Op::JoinLines, "a\n\n b\nc"), "a b c");
        assert_eq!(apply(Op::RemoveEmptyLines, "a\n\n  \nb"), "a\nb");
    }
}
