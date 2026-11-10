//! A small JSON reader for the embedded data files.
//!
//! The data is fixed at compile time and checked by the test suite, so this only needs to read valid JSON. It keeps
//! the order of object keys, which matters: title rewrites run in file order and configurations and features are
//! reported in file order.

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> &[Value] {
        match self {
            Value::Arr(items) => items,
            _ => &[],
        }
    }

    pub(crate) fn as_object(&self) -> &[(String, Value)] {
        match self {
            Value::Obj(items) => items,
            _ => &[],
        }
    }

    /// An array of strings (other items are skipped).
    pub(crate) fn strings(&self) -> Vec<String> {
        self.as_array()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    }

    /// An object of string values, in file order (other values are skipped).
    pub(crate) fn string_pairs(&self) -> Vec<(String, String)> {
        self.as_object()
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
            .collect()
    }
}

/// Parse a JSON document. Panics with a position on malformed input (only used on embedded data).
pub(crate) fn parse(src: &str) -> Value {
    let mut p = Parser {
        b: src.as_bytes(),
        src,
        i: 0,
    };
    let v = p.value();
    p.ws();
    assert!(p.i == p.b.len(), "trailing data in embedded JSON at byte {}", p.i);
    v
}

struct Parser<'a> {
    b: &'a [u8],
    src: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn fail(&self, what: &str) -> ! {
        panic!("malformed embedded JSON at byte {}: {}", self.i, what)
    }

    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        if self.b.get(self.i) != Some(&c) {
            self.fail("unexpected character");
        }
        self.i += 1;
    }

    fn lit(&mut self, word: &str, v: Value) -> Value {
        if self.src[self.i..].starts_with(word) {
            self.i += word.len();
            v
        } else {
            self.fail("bad literal")
        }
    }

    fn value(&mut self) -> Value {
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Value::Obj(items);
                }
                loop {
                    self.ws();
                    let k = self.string();
                    self.eat(b':');
                    let v = self.value();
                    items.push((k, v));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Value::Obj(items);
                        }
                        _ => self.fail("expected , or }"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Value::Arr(items);
                }
                loop {
                    items.push(self.value());
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Value::Arr(items);
                        }
                        _ => self.fail("expected , or ]"),
                    }
                }
            }
            Some(b'"') => Value::Str(self.string()),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'n') => self.lit("null", Value::Null),
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                match self.src[start..self.i].parse::<f64>() {
                    Ok(n) => Value::Num(n),
                    Err(_) => self.fail("bad number"),
                }
            }
            _ => self.fail("unexpected character"),
        }
    }

    fn hex4(&mut self) -> u32 {
        let h = self.src.get(self.i..self.i + 4).unwrap_or_else(|| self.fail("short \\u escape"));
        let v = u32::from_str_radix(h, 16).unwrap_or_else(|_| self.fail("bad \\u escape"));
        self.i += 4;
        v
    }

    fn string(&mut self) -> String {
        if self.b.get(self.i) != Some(&b'"') {
            self.fail("expected string");
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\' {
                self.i += 1;
            }
            out.push_str(&self.src[start..self.i]);
            match self.b.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return out;
                }
                Some(b'\\') => {
                    let e = *self.b.get(self.i + 1).unwrap_or_else(|| self.fail("short escape"));
                    self.i += 2;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut c = self.hex4();
                            if (0xD800..0xDC00).contains(&c) && self.src[self.i..].starts_with("\\u") {
                                self.i += 2;
                                let lo = self.hex4();
                                c = 0x10000 + ((c - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                        }
                        _ => self.fail("bad escape"),
                    }
                }
                _ => self.fail("unterminated string"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_json() {
        let v = parse(r#" {"b": [1, -2.5e1, true, false, null], "a": "xé😀\n\"", "e": {}} "#);
        let items = v.as_object();
        assert_eq!(items[0].0, "b");
        assert_eq!(items[1].0, "a");
        assert_eq!(v.get("a").and_then(Value::as_str), Some("xé😀\n\""));
        assert_eq!(v.get("b").unwrap().as_array()[1].as_f64(), Some(-25.0));
        assert!(v.get("e").unwrap().as_object().is_empty());
    }
}
