//! Python `re` compatibility.
//!
//! The parsing rules (the patterns in this crate's source and the thousands of patterns in the embedded model
//! dictionaries) are written for Python's `re` module, the engine of the reference implementation. [`Rx`] compiles such
//! a pattern so that it matches the way Python would:
//!
//! * the syntax is translated: `\s` also covers U+001C..U+001F, `\w` is `[\p{L}\p{N}_]` (Python's `str.isalnum()`
//!   plus `_`), `$` also matches before a final newline, `{` that is not a repetition is a literal, `[` inside a class
//!   is a literal, `\<` is a literal, and with `re.I` the letter i also matches "ı" and "İ" like in Python;
//! * a pattern without look-around or backreferences is compiled with the [`regex`] crate (finite automata, linear
//!   time); only patterns that use look-around are compiled with [`fancy_regex`] (backtracking, like Python);
//! * `\b` runs natively unless the text has a character that the crates count as a word character and Python does not
//!   (combining marks, connector punctuation) or the other way round (`½`, `²`); then a look-around version with
//!   Python's word characters is used.
//!
//! Both engines use leftmost-first alternation, so the match found is the one Python finds. Patterns are compiled on
//! first use, and a backtracking pattern is only run when the same pattern without its look-around (a plain regex
//! that matches a superset) finds something.

use std::sync::OnceLock;

/// Upper bound on backtracking steps for a single fancy-regex search. Python has no limit; titles are short, and the
/// limit is far above anything the rules need. A search that hits it counts as "no match".
const BACKTRACK_LIMIT: usize = 50_000_000;

enum Engine {
    Plain(regex::Regex),
    Fancy {
        re: fancy_regex::Regex,
        /// The pattern with its look-around removed: it matches whenever the full pattern does, so a miss here
        /// skips the backtracking search.
        pre: Option<regex::Regex>,
    },
}

/// A Python-syntax regular expression, translated eagerly and compiled on first use.
pub(crate) struct Rx {
    pattern: String,
    relaxed: Option<String>,
    fancy: bool,
    engine: OnceLock<Engine>,
    /// The pattern with `\b`/`\B` spelled out with Python's word characters (`None` without word boundaries), for
    /// texts where the crates' word characters differ from Python's.
    exact: Option<String>,
    /// Prefilter for `exact`: the pattern without look-around and without word boundaries (a superset).
    exact_pre: Option<String>,
    exact_engine: OnceLock<(fancy_regex::Regex, Option<regex::Regex>)>,
}

/// The engine chosen for one text.
enum Use<'a> {
    Plain(&'a regex::Regex),
    Fancy(&'a fancy_regex::Regex, Option<&'a regex::Regex>),
}

/// The text has a character whose "word character" status differs between the crates' `\b` (Unicode `\w`:
/// alphabetic, marks, decimal digits, connector punctuation, join controls) and Python's `\w` (letters, digits,
/// numerics, `_`). ASCII text never has one.
fn needs_exact(s: &str) -> bool {
    static RISKY: OnceLock<regex::Regex> = OnceLock::new();
    !s.is_ascii()
        && RISKY
            .get_or_init(|| {
                regex::Regex::new(r"[\w--[\p{L}\p{N}_]]|[[\p{L}\p{N}_]--\w]").unwrap_or_else(|e| panic!("{e}"))
            })
            .is_match(s)
}

fn fancy_regex(pattern: &str) -> Result<fancy_regex::Regex, String> {
    fancy_regex::RegexBuilder::new(pattern)
        .backtrack_limit(BACKTRACK_LIMIT)
        .delegate_size_limit(64 << 20)
        .delegate_dfa_size_limit(64 << 20)
        .build()
        .map_err(|e| e.to_string())
}

/// Capture group spans of one match, as byte offsets into the searched text.
pub(crate) struct Caps<'t> {
    text: &'t str,
    spans: Vec<Option<(usize, usize)>>,
}

impl<'t> Caps<'t> {
    /// Group `i` (0 = whole match); `None` if the group did not take part in the match.
    pub(crate) fn get(&self, i: usize) -> Option<&'t str> {
        self.spans.get(i).copied().flatten().map(|(a, b)| &self.text[a..b])
    }

    /// Group `i`, or `""` if it did not take part (Python's `m.group(i) or ""`).
    pub(crate) fn str(&self, i: usize) -> &'t str {
        self.get(i).unwrap_or("")
    }

    pub(crate) fn start(&self) -> usize {
        self.spans[0].map_or(0, |s| s.0)
    }

    pub(crate) fn end(&self) -> usize {
        self.spans[0].map_or(0, |s| s.1)
    }
}

fn plain_regex(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(pattern)
        .size_limit(64 << 20)
        .dfa_size_limit(64 << 20)
        .build()
        .map_err(|e| e.to_string())
}

fn spans_of(c: &regex::Captures<'_>) -> Vec<Option<(usize, usize)>> {
    c.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect()
}

fn fancy_spans_of(c: &fancy_regex::Captures<'_, str>) -> Vec<Option<(usize, usize)>> {
    c.iter().map(|g| g.map(|m| (m.start(), m.end()))).collect()
}

impl Rx {
    /// Translate a Python pattern; `ignore_case` is Python's `re.I`. Panics on a pattern it cannot translate, which
    /// only happens for patterns that are not in this crate (the test suite compiles every rule).
    pub(crate) fn new(py_pattern: &str, ignore_case: bool) -> Rx {
        let t = translate(py_pattern, ignore_case).unwrap_or_else(|e| panic!("bad pattern {py_pattern:?}: {e}"));
        let flag = if ignore_case { "(?i)" } else { "" };
        Rx {
            pattern: format!("{flag}{}", t.pattern),
            relaxed: t.fancy.then(|| format!("{flag}{}", t.relaxed)),
            fancy: t.fancy,
            engine: OnceLock::new(),
            exact: t.has_b.then(|| format!("{flag}{}", t.exact)),
            exact_pre: t.has_b.then(|| format!("{flag}{}", t.relaxed_no_b)),
            exact_engine: OnceLock::new(),
        }
    }

    fn build(&self) -> Result<Engine, String> {
        if !self.fancy {
            return plain_regex(&self.pattern).map(Engine::Plain);
        }
        let re = fancy_regex(&self.pattern)?;
        let pre = self.relaxed.as_deref().and_then(|p| plain_regex(p).ok());
        Ok(Engine::Fancy { re, pre })
    }

    /// The engine for `s`: the look-around version (without prefilter) when `s` needs it.
    fn pick(&self, s: &str) -> Use<'_> {
        if let Some(exact) = self.exact.as_deref().filter(|_| needs_exact(s)) {
            let (re, pre) = self.exact_engine.get_or_init(|| {
                let re = fancy_regex(exact).unwrap_or_else(|e| panic!("bad pattern {exact:?}: {e}"));
                (re, self.exact_pre.as_deref().and_then(|p| plain_regex(p).ok()))
            });
            return Use::Fancy(re, pre.as_ref());
        }
        match self.engine() {
            Engine::Plain(r) => Use::Plain(r),
            Engine::Fancy { re, pre } => Use::Fancy(re, pre.as_ref()),
        }
    }

    fn engine(&self) -> &Engine {
        self.engine.get_or_init(|| {
            self.build()
                .unwrap_or_else(|e| panic!("bad pattern {:?}: {e}", self.pattern))
        })
    }

    /// Compile now, with the look-around version (tests use it to check every rule).
    #[cfg(test)]
    pub(crate) fn compile(&self) -> Result<(), String> {
        self.build()?;
        self.exact.as_deref().map_or(Ok(()), |p| fancy_regex(p).map(|_| ()))
    }

    /// True if the pattern needs the backtracking engine.
    #[cfg(test)]
    pub(crate) fn is_fancy(&self) -> bool {
        self.fancy
    }

    /// The prefilter of a fancy pattern says there is no match at or after `pos`.
    fn ruled_out(pre: Option<&regex::Regex>, s: &str, pos: usize) -> bool {
        pre.is_some_and(|p| p.find_at(s, pos).is_none())
    }

    /// Python `rx.search(s) is not None`.
    pub(crate) fn is_match(&self, s: &str) -> bool {
        match self.pick(s) {
            Use::Plain(r) => r.is_match(s),
            Use::Fancy(re, pre) => !Rx::ruled_out(pre, s, 0) && re.is_match(s).unwrap_or(false),
        }
    }

    /// Python `rx.search(s)`: span of the first match.
    pub(crate) fn find(&self, s: &str) -> Option<(usize, usize)> {
        self.find_at(s, 0)
    }

    /// The first match starting at byte `pos` or later; the text before `pos` is still seen by look-behind and `\b`.
    pub(crate) fn find_at(&self, s: &str, pos: usize) -> Option<(usize, usize)> {
        match self.pick(s) {
            Use::Plain(r) => r.find_at(s, pos).map(|m| (m.start(), m.end())),
            Use::Fancy(re, pre) => {
                if Rx::ruled_out(pre, s, pos) {
                    return None;
                }
                re.find_from_pos(s, pos).ok().flatten().map(|m| (m.start(), m.end()))
            }
        }
    }

    /// Python `rx.match(s, pos) is not None`: a match that starts exactly at `pos`.
    pub(crate) fn match_at(&self, s: &str, pos: usize) -> bool {
        // leftmost-first: if any match starts at `pos`, the first match found from `pos` starts there
        self.find_at(s, pos).is_some_and(|(a, _)| a == pos)
    }

    /// Python `rx.finditer(s)`: spans of all non-overlapping matches.
    pub(crate) fn find_iter(&self, s: &str) -> Vec<(usize, usize)> {
        match self.pick(s) {
            Use::Plain(r) => r.find_iter(s).map(|m| (m.start(), m.end())).collect(),
            Use::Fancy(re, pre) => {
                if Rx::ruled_out(pre, s, 0) {
                    return Vec::new();
                }
                re.find_iter(s)
                    .map_while(Result::ok)
                    .map(|m| (m.start(), m.end()))
                    .collect()
            }
        }
    }

    /// Python `rx.search(s)` with groups.
    pub(crate) fn captures<'t>(&self, s: &'t str) -> Option<Caps<'t>> {
        let spans = match self.pick(s) {
            Use::Plain(r) => r.captures(s).map(|c| spans_of(&c)),
            Use::Fancy(re, pre) => {
                if Rx::ruled_out(pre, s, 0) {
                    return None;
                }
                re.captures(s).ok().flatten().map(|c| fancy_spans_of(&c))
            }
        };
        spans.map(|spans| Caps { text: s, spans })
    }

    /// Python `rx.finditer(s)` with groups.
    pub(crate) fn captures_iter<'t>(&self, s: &'t str) -> Vec<Caps<'t>> {
        let all: Vec<Vec<Option<(usize, usize)>>> = match self.pick(s) {
            Use::Plain(r) => r.captures_iter(s).map(|c| spans_of(&c)).collect(),
            Use::Fancy(re, pre) => {
                if Rx::ruled_out(pre, s, 0) {
                    return Vec::new();
                }
                re.captures_iter(s)
                    .map_while(Result::ok)
                    .map(|c| fancy_spans_of(&c))
                    .collect()
            }
        };
        all.into_iter().map(|spans| Caps { text: s, spans }).collect()
    }

    /// Python `rx.sub(repl, s)` with a template made by [`py_template`].
    pub(crate) fn replace_all(&self, s: &str, template: &Template) -> String {
        let found = self.captures_iter(s);
        if found.is_empty() {
            return s.to_owned();
        }
        let mut out = String::with_capacity(s.len() + 8);
        let mut last = 0;
        for c in found {
            out.push_str(&s[last..c.start()]);
            for part in &template.0 {
                match part {
                    TemplatePart::Lit(t) => out.push_str(t),
                    TemplatePart::Group(i) => out.push_str(c.str(*i)),
                }
            }
            last = c.end();
        }
        out.push_str(&s[last..]);
        out
    }
}

/// Python `re.compile("|".join(patterns), flags).search(s) is not None`, split so that only the alternatives with
/// look-around run on the backtracking engine (for a yes/no search the split gives the same answer).
pub(crate) struct AnyRx {
    parts: Vec<Rx>,
}

impl AnyRx {
    /// `None` for an empty list (Python: no pattern, nothing to check).
    pub(crate) fn new(patterns: &[String], ignore_case: bool) -> Option<AnyRx> {
        if patterns.is_empty() {
            return None;
        }
        let (fancy, plain): (Vec<&String>, Vec<&String>) = patterns
            .iter()
            .partition(|p| translate(p, ignore_case).map(|t| t.fancy).unwrap_or(true));
        let mut parts = Vec::new();
        if !plain.is_empty() {
            let joined: Vec<&str> = plain.iter().map(|p| p.as_str()).collect();
            parts.push(Rx::new(&joined.join("|"), ignore_case));
        }
        parts.extend(fancy.into_iter().map(|p| Rx::new(p, ignore_case)));
        Some(AnyRx { parts })
    }

    pub(crate) fn is_match(&self, s: &str) -> bool {
        self.parts.iter().any(|rx| rx.is_match(s))
    }

    #[cfg(test)]
    pub(crate) fn parts(&self) -> &[Rx] {
        &self.parts
    }
}

enum TemplatePart {
    Lit(String),
    Group(usize),
}

/// A parsed Python `re.sub` replacement string (`\g<1>`, `\1`, `\\`, `\n`, ...).
pub(crate) struct Template(Vec<TemplatePart>);

/// Parse a Python replacement string. Unmatched groups are replaced with "" (Python 3.5+).
pub(crate) fn py_template(repl: &str) -> Result<Template, String> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut it = repl.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            lit.push(c);
            continue;
        }
        let group = match it.next() {
            Some('g') => {
                if it.next() != Some('<') {
                    return Err(format!("bad \\g in {repl:?}"));
                }
                let name: String = it.by_ref().take_while(|&c| c != '>').collect();
                Some(name.parse::<usize>().map_err(|_| format!("named group in {repl:?}"))?)
            }
            Some(d @ '0'..='9') => {
                let mut n = d.to_digit(10).unwrap_or(0) as usize;
                if let Some(d2) = it.peek().and_then(|c| c.to_digit(10)) {
                    n = n * 10 + d2 as usize;
                    it.next();
                }
                Some(n)
            }
            Some('n') => {
                lit.push('\n');
                None
            }
            Some('t') => {
                lit.push('\t');
                None
            }
            Some('r') => {
                lit.push('\r');
                None
            }
            Some('\\') => {
                lit.push('\\');
                None
            }
            Some(c) if !c.is_ascii_alphanumeric() => {
                lit.push('\\');
                lit.push(c);
                None
            }
            _ => return Err(format!("unsupported escape in {repl:?}")),
        };
        if let Some(g) = group {
            if !lit.is_empty() {
                parts.push(TemplatePart::Lit(std::mem::take(&mut lit)));
            }
            parts.push(TemplatePart::Group(g));
        }
    }
    if !lit.is_empty() {
        parts.push(TemplatePart::Lit(lit));
    }
    Ok(Template(parts))
}

// ---------- pattern translation ----------

const PY_SPACE: &str = r"\s\x1C-\x1F";
const PY_WORD: &str = r"\p{L}\p{N}_";
/// `\b` and `\B` with Python's word characters.
const PY_B: &str = r"(?:(?<=[\p{L}\p{N}_])(?![\p{L}\p{N}_])|(?<![\p{L}\p{N}_])(?=[\p{L}\p{N}_]))";
const PY_NOT_B: &str = r"(?:(?<=[\p{L}\p{N}_])(?=[\p{L}\p{N}_])|(?<![\p{L}\p{N}_])(?![\p{L}\p{N}_]))";
/// With `re.I`, Python matches "i" and "I" also to "ı" (U+0131) and "İ" (U+0130); the crates' case folding does not.
const PY_I: &str = "iI\u{131}\u{130}";

fn is_i(c: char) -> bool {
    matches!(c, 'i' | 'I' | '\u{130}' | '\u{131}')
}

fn hex_escape(cs: &[char], i: usize, n: usize) -> Result<(String, usize), String> {
    let digits: String = cs.get(i..i + n).ok_or("short escape")?.iter().collect();
    let v = u32::from_str_radix(&digits, 16).map_err(|_| "bad hex escape")?;
    Ok((format!("\\x{{{v:X}}}"), i + n))
}

/// Translate the escape at `cs[i] == '\\'`; returns the Rust text, the next index and whether it is a backreference.
fn escape(cs: &[char], i: usize, in_class: bool) -> Result<(String, usize, bool), String> {
    let e = *cs.get(i + 1).ok_or("trailing backslash")?;
    let next = i + 2;
    let simple = |s: &str| Ok((s.to_string(), next, false));
    match e {
        'd' | 'D' => simple(&format!("\\{e}")),
        'b' if in_class => simple(r"\x08"),
        'b' | 'B' => simple(&format!("\\{e}")),
        's' if in_class => simple(PY_SPACE),
        's' => simple(&format!("[{PY_SPACE}]")),
        'S' => simple(&format!("[^{PY_SPACE}]")),
        'w' if in_class => simple(PY_WORD),
        'w' => simple(&format!("[{PY_WORD}]")),
        'W' => simple(&format!("[^{PY_WORD}]")),
        'A' if !in_class => simple(r"\A"),
        'Z' if !in_class => simple(r"\z"),
        'n' | 't' | 'r' => simple(&format!("\\{e}")),
        'f' => simple(r"\x0C"),
        'v' => simple(r"\x0B"),
        'a' => simple(r"\x07"),
        'x' => hex_escape(cs, next, 2).map(|(s, j)| (s, j, false)),
        'u' => hex_escape(cs, next, 4).map(|(s, j)| (s, j, false)),
        'U' => hex_escape(cs, next, 8).map(|(s, j)| (s, j, false)),
        '1'..='9' if !in_class => Ok((format!("\\{e}"), next, true)),
        '<' | '>' => simple(&e.to_string()),
        c if c.is_ascii_punctuation() || c == ' ' => simple(&format!("\\{c}")),
        c if !c.is_ascii() => simple(&c.to_string()),
        c => Err(format!("unsupported escape \\{c}")),
    }
}

/// `{m,n}` repetition at `cs[i] == '{'` as Python reads it; `None` if the brace is a literal.
fn repetition(cs: &[char], i: usize) -> Option<(String, usize)> {
    let mut j = i + 1;
    let lo: String = cs[j..].iter().take_while(|c| c.is_ascii_digit()).collect();
    j += lo.len();
    let (hi, comma) = if cs.get(j) == Some(&',') {
        j += 1;
        let hi: String = cs[j..].iter().take_while(|c| c.is_ascii_digit()).collect();
        j += hi.len();
        (hi, true)
    } else {
        (lo.clone(), false)
    };
    if cs.get(j) != Some(&'}') || (!comma && lo.is_empty()) {
        return None;
    }
    let lo = if lo.is_empty() { "0".to_string() } else { lo };
    let q = if !comma {
        format!("{{{lo}}}")
    } else if hi.is_empty() {
        format!("{{{lo},}}")
    } else {
        format!("{{{lo},{hi}}}")
    };
    Some((q, j + 1))
}

pub(crate) struct Translated {
    /// The pattern in the `regex`/`fancy-regex` dialect.
    pub(crate) pattern: String,
    /// The same without look-around groups (only meaningful when `fancy`).
    pub(crate) relaxed: String,
    /// Needs the backtracking engine (look-around, backreferences, atomic groups).
    pub(crate) fancy: bool,
    /// The pattern with `\b`/`\B` spelled out with Python's word characters (only meaningful when `has_b`).
    pub(crate) exact: String,
    /// The pattern has `\b` or `\B`.
    pub(crate) has_b: bool,
    /// `relaxed` without the word boundaries: matches whenever `exact` does.
    pub(crate) relaxed_no_b: String,
}

/// Translate a Python pattern to the `regex`/`fancy-regex` dialect; `ignore_case` is Python's `re.I`.
pub(crate) fn translate(p: &str, ignore_case: bool) -> Result<Translated, String> {
    let cs: Vec<char> = p.chars().collect();
    let mut out = String::with_capacity(p.len() + 16);
    let mut relaxed = String::with_capacity(p.len() + 16);
    let mut exact = String::with_capacity(p.len() + 16);
    let mut relaxed_no_b = String::with_capacity(p.len() + 16);
    let mut has_b = false;
    let mut fancy = false;
    // open groups: true for a look-around group; `skip` = depth at which the outermost open look-around started
    let mut groups: Vec<bool> = Vec::new();
    let mut skip: Option<usize> = None;
    let mut i = 0;
    while i < cs.len() {
        let mark = out.len();
        // what `exact` gets instead of the text added to `out` (word boundaries)
        let mut exact_piece: Option<&str> = None;
        match cs[i] {
            '\\' if matches!(cs.get(i + 1), Some('b' | 'B')) => {
                let not = cs[i + 1] == 'B';
                out.push_str(if not { r"\B" } else { r"\b" });
                exact_piece = Some(if not { PY_NOT_B } else { PY_B });
                has_b = true;
                i += 2;
            }
            '\\' => {
                let (s, j, backref) = escape(&cs, i, false)?;
                fancy |= backref;
                out.push_str(&s);
                i = j;
            }
            '[' => {
                out.push('[');
                i += 1;
                if cs.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                // the class holds i, I, ı or İ (alone or in a range): with re.I it gets all four
                let mut has_i = false;
                if cs.get(i) == Some(&']') {
                    out.push_str(r"\]");
                    i += 1;
                }
                loop {
                    let c = *cs.get(i).ok_or("unterminated class")?;
                    if c != '\\' && c != ']' {
                        let hi = match (cs.get(i + 1), cs.get(i + 2)) {
                            (Some('-'), Some(&h)) if h != ']' && h != '\\' => h,
                            _ => c,
                        };
                        has_i |= ['I', 'i', '\u{130}', '\u{131}'].iter().any(|x| (c..=hi).contains(x));
                    }
                    match c {
                        ']' => {
                            if ignore_case && has_i {
                                out.push_str(PY_I);
                            }
                            out.push(']');
                            i += 1;
                            break;
                        }
                        '\\' => {
                            let (s, j, _) = escape(&cs, i, true)?;
                            out.push_str(&s);
                            i = j;
                            continue;
                        }
                        '[' | '&' | '~' => {
                            out.push('\\');
                            out.push(c);
                        }
                        '-' if cs.get(i + 1) == Some(&'-') => return Err("'--' in a class".into()),
                        _ => out.push(c),
                    }
                    i += 1;
                }
            }
            '(' => {
                let a = cs.get(i + 1).copied();
                let b = cs.get(i + 2).copied();
                let c = cs.get(i + 3).copied();
                let look = matches!(
                    (a, b, c),
                    (Some('?'), Some('=' | '!'), _) | (Some('?'), Some('<'), Some('=' | '!'))
                );
                if a == Some('?') && b == Some('#') {
                    // comment group
                    while i < cs.len() && cs[i] != ')' {
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
                if look || (a == Some('?') && (b == Some('>') || (b == Some('P') && c == Some('=')))) {
                    fancy = true;
                }
                if look && skip.is_none() {
                    skip = Some(groups.len());
                }
                groups.push(look);
                out.push('(');
                i += 1;
            }
            ')' => {
                groups.pop();
                out.push(')');
                i += 1;
                if skip == Some(groups.len()) {
                    // the closing parenthesis of a look-around: not in `relaxed`
                    skip = None;
                    exact.push(')');
                    continue;
                }
            }
            '$' => {
                out.push_str(r"(?:\n?\z)");
                i += 1;
            }
            '{' => {
                if let Some((q, j)) = repetition(&cs, i) {
                    out.push_str(&q);
                    i = j;
                } else {
                    out.push_str(r"\{");
                    i += 1;
                }
            }
            '}' => {
                out.push_str(r"\}");
                i += 1;
            }
            c if ignore_case && is_i(c) => {
                out.push('[');
                out.push_str(PY_I);
                out.push(']');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
        if skip.is_none() {
            relaxed.push_str(&out[mark..]);
            if exact_piece.is_none() {
                relaxed_no_b.push_str(&out[mark..]);
            }
        }
        exact.push_str(exact_piece.unwrap_or(&out[mark..]));
    }
    Ok(Translated {
        pattern: out,
        relaxed,
        fancy,
        exact,
        has_b,
        relaxed_no_b,
    })
}

// ---------- Python string helpers ----------

/// Python `str.isspace()` for one character (Unicode White_Space plus U+001C..U+001F).
pub(crate) fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python `re.sub(r"\s+", " ", s)`.
pub(crate) fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if py_isspace(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// Python `str.strip()`.
pub(crate) fn py_strip(s: &str) -> &str {
    s.trim_matches(py_isspace)
}

/// Value of a Unicode decimal digit (what Python's `\d` matches and `int()` reads).
pub(crate) fn digit_value(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        return Some(c as u32 - '0' as u32);
    }
    let cp = c as u32;
    let zeros = &crate::tables::DIGIT_ZEROS;
    let i = zeros.partition_point(|&z| z <= cp);
    let zero = *zeros.get(i.checked_sub(1)?)?;
    (cp - zero < 10).then_some(cp - zero)
}

/// Python `int(s)` for a string of decimal digits (any script).
pub(crate) fn py_int(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    s.chars().try_fold(0u64, |acc, c| {
        acc.checked_mul(10)?.checked_add(u64::from(digit_value(c)?))
    })
}

/// Python `float(s)` for digits with an optional decimal point (any script).
pub(crate) fn py_float(s: &str) -> Option<f64> {
    let ascii: Option<String> = s
        .chars()
        .map(|c| {
            if c == '.' {
                Some('.')
            } else {
                digit_value(c).and_then(|d| char::from_digit(d, 10))
            }
        })
        .collect();
    ascii?.parse().ok()
}

/// Python `s[:1].isdigit()` for the caliber names in the data (decimal digits; superscripts never start a name).
pub(crate) fn starts_with_digit(s: &str) -> bool {
    s.chars().next().and_then(digit_value).is_some()
}

/// Python `re.escape` (3.7+): the same characters get a backslash.
pub(crate) fn py_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if "()[]{}?*+-|^$\\.&~# \t\n\r\x0B\x0C".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Number of characters before byte offset `byte` (Python string index).
pub(crate) fn char_index(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// Byte offset `n` characters before byte offset `byte`, stopping at 0 (Python `s[max(0, i - n):i]`).
pub(crate) fn back_chars(s: &str, byte: usize, n: usize) -> usize {
    s[..byte].char_indices().rev().take(n).last().map_or(byte, |(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation() {
        let t = |p: &str| translate(p, false).unwrap();
        assert_eq!(t(r"\bfoo\s*x{2,}$").pattern, r"\bfoo[\s\x1C-\x1F]*x{2,}(?:\n?\z)");
        assert_eq!(t(r"a{,3}b{}c{").pattern, r"a{0,3}b\{\}c\{");
        assert_eq!(t(r"[\s\-&[]").pattern, r"[\s\x1C-\x1F\-\&\[]");
        assert!(t(r"(?<![a-z])x").fancy);
        assert!(!t(r"(?:x)|(y)").fancy);
        assert_eq!(t(r"\<\#\ ").pattern, r"<\#\ ");
        let x = t(r"(?<![a-z(])x(?=(a|b)\))y(?!z)");
        assert!(x.fancy);
        assert_eq!(x.relaxed, "xy");
    }

    #[test]
    fn engines_agree_with_python() {
        let r = Rx::new(r"(?<![\d.])(\d{1,3})\s*\+\s*1(?!\d)", false);
        assert!(r.is_fancy());
        assert_eq!(r.find("x 15+1 y"), Some((2, 6)));
        assert_eq!(r.find("115+12"), None);
        let r = Rx::new(r"^\d+\s?g$", false);
        assert!(!r.is_fancy());
        assert!(r.is_match("12g\n"));
        assert!(!r.is_match("12g\n\n"));
        let t = py_template(r"glock \g<2> gen \g<1>").unwrap();
        assert_eq!(
            Rx::new(r"\bglock gen\.? ?([1-6]) (\d{2})\b", true).replace_all("glock gen 5 19 mos", &t),
            "glock 19 gen 5 mos"
        );
    }

    #[test]
    fn word_boundaries_and_case_like_python() {
        // Python: a combining mark is not a word character, "½" is
        let b = Rx::new(r"\b15gr\b", false);
        assert!(b.is_match("1\u{20dd}15gr"));
        assert!(b.is_match("x \u{301}15gr"));
        assert!(!b.is_match("\u{bd}15gr"));
        assert!(Rx::new(r"a\b", false).is_match("a\u{301}"));
        let fancy = Rx::new(r"(?<!x)\b18\b", false);
        assert_eq!(fancy.find("16\u{bd} 18\u{301}"), Some((5, 7)));
        // re.I: "i" also matches dotless and dotted I
        assert!(Rx::new("sarsilmaz", true).is_match("sars\u{131}lmaz"));
        assert!(Rx::new("[a-z]", true).is_match("\u{130}"));
        assert!(!Rx::new("[^a-z]", true).is_match("\u{131}"));
        assert!(!Rx::new("sarsilmaz", false).is_match("sars\u{131}lmaz"));
    }

    #[test]
    fn helpers() {
        assert_eq!(collapse_ws(" a \t\u{1c}b  "), " a b ");
        assert_eq!(py_int("١٥"), Some(15));
        assert_eq!(py_float("4.02"), Some(4.02));
        assert_eq!(py_escape("a b-c&d~e#f.g"), r"a\ b\-c\&d\~e\#f\.g");
        assert_eq!(back_chars("ab×cd", 5, 2), 2);
        assert_eq!(back_chars("abc", 2, 10), 0);
        assert_eq!(char_index("a×b", 3), 2);
    }
}
