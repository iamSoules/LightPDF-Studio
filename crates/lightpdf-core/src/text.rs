//! Text helpers for the standard Helvetica font: WinAnsi encoding, metrics, wrapping.

/// WinAnsiEncoding code points 0x80–0x9F.
const WIN_ANSI_80: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–',
    '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

/// Encodes a char in WinAnsi; `None` if it can't be represented.
pub fn win_ansi(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x20..=0x7E => Some(u as u8),
        0xA0..=0xFF => Some(u as u8),
        _ => WIN_ANSI_80.iter().position(|&x| x == c && !(0x80..=0x9F).contains(&(x as u32))).map(|i| 0x80 + i as u8),
    }
}

/// Replaces characters the standard fonts can't show (like v2's `safeText`).
pub fn safe_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\t' => c,
            c if win_ansi(c).is_some() => c,
            '\u{2009}' | '\u{202f}' | '\u{2002}' | '\u{2003}' => ' ',
            '−' => '-',
            _ => '?',
        })
        .collect()
}

pub fn encode_win_ansi(s: &str) -> Vec<u8> {
    s.chars().filter_map(|c| win_ansi(c).or(Some(b'?'))).collect()
}

/// PDFDocEncoding byte → char (identical to Latin-1 for the printable range).
pub fn pdfdoc_char(b: u8) -> char {
    match b {
        0x80..=0x9F => WIN_ANSI_80[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Helvetica advance widths (1/1000 em) for 0x20..=0x7E.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // ' '..'/'
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0-9
    278, 278, 584, 584, 584, 556, 1015, // :;<=>?@
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667,
    611, // A-Z
    278, 278, 278, 469, 556, 333, // [\]^_`
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500,
    500, // a-z
    334, 260, 334, 584, // {|}~
];

/// Helvetica-Bold advance widths for 0x20..=0x7E.
const HELV_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333,
    333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722,
    667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611,
    611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

fn base_letter(c: char) -> char {
    match c {
        'À'..='Å' => 'A',
        'Æ' => 'W',
        'Ç' => 'C',
        'È'..='Ë' => 'E',
        'Ì'..='Ï' => 'I',
        'Ð' => 'D',
        'Ñ' => 'N',
        'Ò'..='Ö' | 'Ø' | 'Œ' => 'O',
        'Ù'..='Ü' => 'U',
        'Ý' | 'Ÿ' => 'Y',
        'Š' => 'S',
        'Ž' => 'Z',
        'à'..='å' => 'a',
        'æ' | 'œ' => 'm',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'š' => 's',
        'ž' => 'z',
        'ß' => 'p',
        '–' => '_',
        '—' => 'M',
        '‘' | '’' | '‚' => '\'',
        '“' | '”' | '„' => '"',
        '•' | '·' => '-',
        '…' => 'm',
        '€' | '£' | '¥' | '§' | '°' | '±' => '0',
        _ => 'n',
    }
}

pub fn char_width(c: char, bold: bool) -> f64 {
    let t = if bold { &HELV_BOLD } else { &HELV };
    let c = if (' '..='~').contains(&c) { c } else { base_letter(c) };
    t[(c as usize).saturating_sub(0x20).min(94)] as f64 / 1000.0
}

pub fn text_width(s: &str, size: f64, bold: bool) -> f64 {
    s.chars().map(|c| char_width(c, bold)).sum::<f64>() * size
}

/// Word wrap into lines that fit `width` (same algorithm as v2's `wrapText`).
pub fn wrap_text(text: &str, size: f64, bold: bool, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in split_keep_ws(para) {
            let test = format!("{line}{word}");
            if !line.is_empty() && text_width(test.trim_end(), size, bold) > width {
                lines.push(line.trim_end().to_string());
                line = word.trim_start().to_string();
            } else {
                line = test;
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

fn split_keep_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_ws = None;
    for (i, c) in s.char_indices() {
        let ws = c.is_whitespace();
        if in_ws.is_some_and(|w| w != ws) {
            out.push(&s[start..i]);
            start = i;
        }
        in_ws = Some(ws);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Escapes bytes for a PDF literal string.
pub fn pdf_literal(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.push(b'(');
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\n' => out.extend_from_slice(b"\\n"),
            _ => out.push(b),
        }
    }
    out.push(b')');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding() {
        assert_eq!(win_ansi('A'), Some(65));
        assert_eq!(win_ansi('ä'), Some(0xE4));
        assert_eq!(win_ansi('€'), Some(0x80));
        assert_eq!(win_ansi('—'), Some(0x97));
        assert_eq!(win_ansi('日'), None);
        assert_eq!(safe_text("Grüße 日本"), "Grüße ??");
    }

    #[test]
    fn wrapping() {
        let l = wrap_text("Hello world this is a test", 12.0, false, 60.0);
        assert!(l.len() > 1);
        for line in &l {
            assert!(text_width(line, 12.0, false) <= 60.0 || !line.contains(' '));
        }
        assert_eq!(wrap_text("a\nb", 12.0, false, 100.0), vec!["a", "b"]);
        assert!((text_width("Hello", 10.0, false) - 22.78).abs() < 0.01);
    }
}
