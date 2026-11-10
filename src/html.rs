//! `unescape`: decode HTML character references exactly like Python's `html.unescape`.
//!
//! Store titles often carry entities (`26&quot;`, `10&#x2B;1`, `&amp;`), sometimes escaped twice. The rules follow
//! the HTML5 standard as implemented by Python: named references with and without `;`, the longest known prefix of an
//! unknown name (`&ampx` -> `&x`), numeric references remapped or dropped the way browsers do.

use crate::tables::{HTML5, INVALID_CHARREFS, INVALID_CODEPOINTS};
use std::borrow::Cow;

fn named(name: &str) -> Option<&'static str> {
    HTML5
        .binary_search_by(|(k, _)| k.as_bytes().cmp(name.as_bytes()))
        .ok()
        .map(|i| HTML5[i].1)
}

fn numeric(digits: &str, radix: u32) -> String {
    // Python's int() has no upper bound; anything past u64 is far above 0x10FFFF anyway
    let num = digits
        .chars()
        .try_fold(0u64, |acc, c| {
            acc.checked_mul(u64::from(radix))?.checked_add(u64::from(c.to_digit(radix)?))
        })
        .unwrap_or(u64::MAX);
    if let Ok(i) = INVALID_CHARREFS.binary_search_by_key(&num, |&(k, _)| u64::from(k)) {
        return INVALID_CHARREFS[i].1.to_string();
    }
    if (0xD800..=0xDFFF).contains(&num) || num > 0x10FFFF {
        return "\u{FFFD}".to_string();
    }
    let num = num as u32; // <= 0x10FFFF here
    if INVALID_CODEPOINTS.binary_search(&num).is_ok() {
        return String::new();
    }
    char::from_u32(num).map(String::from).unwrap_or_default()
}

/// Length in bytes of the character reference body after `&` at the start of `rest`, or 0 if none
/// (Python's `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)`).
fn charref_len(rest: &str) -> usize {
    let b = rest.as_bytes();
    let with_semicolon = |n: usize| if b.get(n) == Some(&b';') { n + 1 } else { n };
    if b.first() == Some(&b'#') {
        let digits = b[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            return with_semicolon(1 + digits);
        }
        if matches!(b.get(1), Some(b'x') | Some(b'X')) {
            let hex = b[2..].iter().take_while(|c| c.is_ascii_hexdigit()).count();
            if hex > 0 {
                return with_semicolon(2 + hex);
            }
        }
        return 0;
    }
    let mut n = 0;
    for (count, c) in rest.chars().enumerate() {
        if count == 32 || matches!(c, '\t' | '\n' | '\u{c}' | ' ' | '<' | '&' | '#' | ';') {
            break;
        }
        n += c.len_utf8();
    }
    if n == 0 {
        0
    } else {
        with_semicolon(n)
    }
}

fn replace(body: &str, out: &mut String) {
    if let Some(num) = body.strip_prefix('#') {
        let (digits, radix) = match num.strip_prefix(['x', 'X']) {
            Some(hex) => (hex, 16),
            None => (num, 10),
        };
        out.push_str(&numeric(digits.trim_end_matches(';'), radix));
        return;
    }
    if let Some(v) = named(body) {
        out.push_str(v);
        return;
    }
    // the longest known name that the reference starts with ("&amp;" written as "&ampx")
    let ends: Vec<usize> = body.char_indices().map(|(i, _)| i).skip(2).collect();
    for &end in ends.iter().rev() {
        if let Some(v) = named(&body[..end]) {
            out.push_str(v);
            out.push_str(&body[end..]);
            return;
        }
    }
    out.push('&');
    out.push_str(body);
}

/// Decode HTML character references like Python's `html.unescape`.
pub(crate) fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let n = charref_len(after);
        if n == 0 {
            out.push('&');
        } else {
            replace(&after[..n], &mut out);
        }
        rest = &after[n..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn matches_python() {
        // expected values from Python 3.13 html.unescape
        let cases = [
            ("26&quot; Barrel", "26\" Barrel"),
            ("10&#x2B;1", "10+1"),
            ("&amp;quot;", "&quot;"),
            ("&ampx &amp", "&x &"),
            ("&#128;&#x80;", "\u{20ac}\u{20ac}"),
            ("&#0;&#xD800;&#1114112;", "\u{FFFD}\u{FFFD}\u{FFFD}"),
            ("a&#1;b&#xFFFF;c", "abc"),
            ("&notit; &notin; &not", "\u{ac}it; \u{2209} \u{ac}"),
            ("&#; &#x; & &zz; &;", "&#; &#x; & &zz; &;"),
            ("&#99999999999999999999999;", "\u{FFFD}"),
            ("&Eacute;t&eacute", "\u{c9}t\u{e9}"),
        ];
        for (input, want) in cases {
            assert_eq!(unescape(input), want, "{input}");
        }
    }
}
