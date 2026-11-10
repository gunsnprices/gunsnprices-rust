//! Recognize firearm calibers in product titles and normalize caliber fields.
//!
//! The caliber list and the retailer spellings come from the GunsNPrices calibers dataset
//! (<https://huggingface.co/datasets/gunsnprices/firearm-calibers>). On top of the dataset spellings, common title
//! forms are generated: "300 Blackout", "12ga", "7.62 x 39", "45 Auto", "6.5 Creed", ".308 Winchester" and so on.

use crate::html::unescape;
use crate::json;
use crate::pyre::{collapse_ws, digit_value, py_escape, py_isspace, py_strip, starts_with_digit, translate, Rx};
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

/// What a caliber is chambered in: the `type` field of the GunsNPrices caliber list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum CaliberKind {
    /// Handgun cartridges (9mm, .45 ACP, .357 Mag, ...).
    Handgun,
    /// Centerfire rifle cartridges (.308 Win, 5.56 NATO, 6.5 Creedmoor, ...).
    Rifle,
    /// Rimfire cartridges (.22 LR, .22 WMR, .17 HMR, ...).
    Rimfire,
    /// Shotgun gauges (12 Gauge, .410 Bore, ...).
    Shotgun,
    /// Airgun pellet calibers (.177, .22 pellet, ...).
    Airgun,
    /// Muzzleloader calibers (.50 cal, ...).
    Muzzleloader,
}

impl CaliberKind {
    /// The name used in the data and in JSON output: `"handgun"`, `"rifle"`, `"rimfire"`, `"shotgun"`, `"airgun"`
    /// or `"muzzleloader"`.
    pub fn as_str(self) -> &'static str {
        match self {
            CaliberKind::Handgun => "handgun",
            CaliberKind::Rifle => "rifle",
            CaliberKind::Rimfire => "rimfire",
            CaliberKind::Shotgun => "shotgun",
            CaliberKind::Airgun => "airgun",
            CaliberKind::Muzzleloader => "muzzleloader",
        }
    }

    fn from_data(s: &str) -> CaliberKind {
        match s {
            "handgun" => CaliberKind::Handgun,
            "rifle" => CaliberKind::Rifle,
            "rimfire" => CaliberKind::Rimfire,
            "shotgun" => CaliberKind::Shotgun,
            "airgun" => CaliberKind::Airgun,
            "muzzleloader" => CaliberKind::Muzzleloader,
            other => panic!("unknown caliber type {other:?} in calibers.json"),
        }
    }
}

impl fmt::Display for CaliberKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A caliber (cartridge) from the GunsNPrices caliber list.
///
/// Calibers are cheap to copy: the strings point into the embedded list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct Caliber {
    /// Stable id, e.g. `"9x19mmparabellum"`.
    pub id: &'static str,
    /// Display name, e.g. `"9×19mm Parabellum"`.
    pub name: &'static str,
    /// Handgun, rifle, rimfire, shotgun, airgun or muzzleloader (`"type"` in JSON).
    #[cfg_attr(feature = "serde", serde(rename = "type"))]
    pub kind: CaliberKind,
    /// GunsNPrices page comparing prices for this caliber (`""` if none).
    pub url: &'static str,
}

impl fmt::Display for Caliber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

// ---------- spellings ----------

/// word -> spellings used in titles
fn synonyms(tok: &str) -> Option<&'static [&'static str]> {
    Some(match tok {
        "win" => &["win", "winchester"],
        "winchester" => &["winchester", "win"],
        "rem" => &["rem", "remington"],
        "remington" => &["remington", "rem"],
        "mag" => &["mag", "magnum"],
        "magnum" => &["magnum", "mag"],
        "wby" => &["wby", "weatherby"],
        "weatherby" => &["weatherby", "wby"],
        "spl" => &["spl", "special"],
        "special" => &["special", "spl"],
        "sprg" => &["sprg", "springfield", "spfd"],
        "springfield" => &["springfield", "sprg", "spfd"],
        "government" => &["government", "govt", "gov"],
        "acp" => &["acp", "auto"],
        "auto" => &["auto", "acp"],
        "s&w" => &[
            "s&w",
            "s & w",
            "sw",
            "smith & wesson",
            "smith and wesson",
            "smith&wesson",
        ],
        "blackout" => &["blackout", "blk", "black out"],
        "creedmoor" => &["creedmoor", "creed", "cm", "crd", "creedmore"],
        "parabellum" => &["parabellum", "para", "luger"],
        "gauge" => &["gauge", "ga", "gauges"],
        "bore" => &["bore", "ga", "gauge"],
        "lr" => &["lr", "long rifle"],
        "h&h" => &["h&h", "h & h", "holland & holland", "holland and holland"],
        "h&r" => &["h&r", "h & r"],
        "bm" => &["bm", "bushmaster", "bush"],
        _ => return None,
    })
}

/// A word that may be left out ("300 Blackout" for ".300 AAC Blackout").
const OPTIONAL: [&str; 9] = [
    "aac",
    "nato",
    "win",
    "winchester",
    "rem",
    "remington",
    "sprg",
    "springfield",
    "government",
];
const SEP: &str = r"[\s\-]*";

/// Common title spellings that the synonym table does not produce, by caliber display name.
fn extra(name: &str) -> &'static [&'static str] {
    match name {
        ".300 AAC Blackout" => &["300 aac", "300 bo", "300 black out", "300 blackout", "300 blk"],
        "6.5mm Creedmoor" => &["6.5 creedmoor", "6.5 crd", "6.5 cm", "6.5mm cm", "6.5 cr"],
        "6mm Creedmoor" => &["6 creedmoor"],
        ".22 LR" => &["22lr", "22 lr", "22 long rifle"],
        ".22 WMR" => &["22 mag", "22 magnum", "22 win mag", "22 wmr"],
        "5.56×45mm NATO" => &[
            "5.56 nato",
            "5.56mm nato",
            "556 nato",
            "5.56x45",
            "5.56",
            "5.56mm",
            "5.56 mm",
        ],
        "7.62x51mm NATO" => &["7.62 nato", "7.62mm nato", "762 nato", "762x51"],
        "7.62x39mm" => &["762x39"],
        ".410 Bore" => &["410 ga", "410 gauge", "410 bore"],
        ".45 Colt" => &["45 lc", "45 long colt"],
        ".44 S&W Special" => &["44 spl", "44 special", "44 spcl"],
        ".38 S&W Special" => &["38 spl", "38 special", "38 spcl", "38 spc", "38 sp", "38 spp", "38+p"],
        ".357 Mag" => &[
            "357 rm",
            "357 rem mag",
            "357 rem magnum",
            "357 remington magnum",
            "357 remington mag",
        ],
        "9×19mm Parabellum" => &["9mm luger", "9mm para", "9 mm luger", "9mm"],
        "10mm Auto" => &["10mm", "10 mm"],
        ".45 ACP" => &["45 acp", "45 auto", "45 automatic", "45 automatic colt pistol"],
        ".40 S&W" => &["40 s&w", "40 sw", "40 cal", "40s", ".40"],
        ".380 ACP" => &["380 acp", "380 auto", "380"],
        ".243 Win" => &["243", ".243"],
        ".308 Win" => &["308", ".308"],
        ".350 Legend" => &[".350", "350 leg", "350 lgnd", "350 lgd"],
        ".450 BM" => &["450 bushmaster", "450 bush"],
        ".204 Ruger" => &[".204"],
        ".177" => &["177", ".177"],
        ".300 Win Mag" => &["300 wm"],
        "7mm Remington Magnum" => &["7mm rm", "7 rm", "7mm rem mag"],
        ".338 Lapua Mag" => &["338 lapua", "338 lap"],
        ".30 Carbine" => &["30 m1", "30 m1 carbine"],
        ".30-06 Springfield" => &["30-06 sp"],
        "7.92×57mm Mauser" => &["8x57", "8x57mm", "8x57 js", "8x57 is"],
        ".50 AE" => &["50 action express"],
        "12 Gauge" => &["12 ga", "12ga", "12 gauge", "12-gauge", "12g"],
        "20 Gauge" => &["20 ga", "20ga", "20 gauge", "20-gauge", "20g"],
        "16 Gauge" => &["16 ga", "16ga", "16 gauge", "16g"],
        "28 Gauge" => &["28 ga", "28ga", "28 gauge", "28g"],
        "10 Gauge" => &["10g"],
        _ => &[],
    }
}

/// A bare number or "Nmm" in a title is usually something else, not a caliber; these exceptions were checked
/// against GunsNPrices listings (right in 97%+ of cases).
const ALLOW_BARE: [&str; 17] = [
    "380", "410", "9mm", "10mm", "10 mm", "5.56", "5.56mm", "5.56 mm", "243", ".243", "308", ".308", ".40", ".350",
    ".204", "177", ".177",
];

/// Shotguns "O/U 12/28": gauge / barrel length; on ammo "20/25" is rounds per box / boxes per case;
/// "20/28 gauge" is a two-gauge set. ".357/.38 Spl" names both cartridges.
const CONTEXT: [(&str, &str); 4] = [
    (
        r"(?<![a-z0-9./$#-])12/(?:1[89]|2\d|3[0-4])(?:\.5)?(?![\d/.])(?!\s*(?:ga|gauge|rd|round))",
        "12gauge",
    ),
    (
        r"(?<![a-z0-9./$#-])20/(?:18|18\.5|2[0-46-9]|3[0-2])(?![\d/.])(?!\s*(?:ga|gauge|rd|round))",
        "20gauge",
    ),
    (
        r"(?<![a-z0-9./$#-])28/(?:2[6-9]|3[0-2])(?![\d/.])(?!\s*(?:ga|gauge|rd|round))",
        "28gauge",
    ),
    (r"(?<![a-z0-9.$#-])\.?357(?=\s*/\s*\.?38)", "357mag"),
];

fn is_digit(c: Option<&char>) -> bool {
    c.and_then(|&c| digit_value(c)).is_some()
}

/// Replace `from` with `to` where it sits between two digits (Python `re.sub(r"(?<=\d)F(?=\d)", to, s)`).
fn between_digits(s: &str, from: char, to: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in cs.iter().enumerate() {
        if c == from && i > 0 && is_digit(cs.get(i - 1)) && is_digit(cs.get(i + 1)) {
            out.push_str(to);
        } else {
            out.push(c);
        }
    }
    out
}

/// Lookup key of a spelling: lower case, letters and digits only, "&" as "and", decimal points kept.
fn key(s: &str) -> String {
    let s = unescape(s).to_lowercase().replace(['×', '\u{445}'], "x");
    let s = between_digits(&s, '.', "#");
    let s: String = s
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '#' || *c == '&')
        .collect();
    s.replace('#', ".").replace('&', "and")
}

fn tokens(name: &str) -> Vec<String> {
    let s = unescape(name).to_lowercase().replace('×', "x");
    let s = s.strip_prefix('.').unwrap_or(&s);
    py_strip(s)
        .split(py_isspace)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

struct Rules {
    bare: Rx,
    mm_pair: Rx,
    mm_single: Rx,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| Rules {
        bare: Rx::new(r"^\.?\d+(?:\.\d+)?(?:\s*mm)?$", false),
        mm_pair: Rx::new(r"\A(?:\.?\d+(?:\.\d+)?x\d+mm[a-z]*)\Z", false),
        mm_single: Rx::new(r"\A(?:\d+mm)\Z", false),
    })
}

fn is_bare(v: &str) -> bool {
    rules().bare.is_match(v) && !ALLOW_BARE.contains(&v)
}

fn token_alts(tok: &str) -> Vec<String> {
    let r = rules();
    if r.mm_pair.is_match(tok) {
        // 9x19mm, 5.56x45mm, 7.62x54mmr
        return vec![tok.to_owned(), tok.replace("mm", "")];
    }
    if r.mm_single.is_match(tok) {
        return vec![tok.to_owned(), format!("{} mm", &tok[..tok.len() - 2])];
    }
    let mut alts: Vec<String> = match synonyms(tok) {
        Some(s) => s.iter().map(|x| (*x).to_owned()).collect(),
        None => vec![tok.to_owned()],
    };
    if OPTIONAL.contains(&tok) {
        alts.push(String::new());
    }
    alts
}

/// All title spellings generated from a name (at most 400 combinations, like the reference implementation).
fn variants(name: &str) -> Vec<String> {
    let toks = tokens(name);
    if toks.is_empty() || name.contains('/') {
        return Vec::new();
    }
    let alts: Vec<Vec<String>> = toks.iter().map(|t| token_alts(t)).collect();
    let mut out: Vec<String> = Vec::new();
    let mut idx = vec![0usize; alts.len()];
    for _ in 0..400 {
        let v = idx
            .iter()
            .zip(&alts)
            .map(|(&i, a)| a[i].as_str())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !v.is_empty() && !is_bare(&v) {
            out.push(v);
        }
        // itertools.product order: the last position changes fastest
        let mut pos = alts.len();
        loop {
            if pos == 0 {
                out.sort();
                out.dedup();
                return out;
            }
            pos -= 1;
            idx[pos] += 1;
            if idx[pos] < alts[pos].len() {
                break;
            }
            idx[pos] = 0;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Python-syntax pattern for one spelling: optional leading dot, flexible "x" and separators.
fn spelling_rx(variant: &str) -> String {
    let parts: Vec<String> = variant
        .split(' ')
        .map(|tok| between_digits(&py_escape(tok), 'x', r"\s*x\s*").replace(r"\-", SEP))
        .collect();
    let body = parts.join(SEP);
    if starts_with_digit(variant) {
        format!(r"\.?{body}")
    } else {
        body
    }
}

struct Row {
    id: String,
    name: String,
    kind: CaliberKind,
    url: String,
    site_name: String,
    aliases: Vec<String>,
    n: f64,
}

/// Python's `(?<![a-z0-9.$])(?:SPELLINGS)(?![a-z0-9])` with `re.I`, run on the `regex` crate.
///
/// The look-around is done by hand so the ~2,000 spellings stay in one finite-automaton search: the look-ahead
/// becomes one consumed character that is not a letter or digit, with a NUL appended to the text so that it also
/// holds at the end (no spelling can match a NUL), and the look-behind is checked on each match, retrying one
/// character later when it fails, exactly as a backtracking engine would.
struct Spellings {
    re: regex::Regex,
}

/// Characters that Python's case-insensitive `[a-z0-9]` matches.
fn ci_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '\u{130}' | '\u{131}' | '\u{17F}' | '\u{212A}')
}

impl Spellings {
    fn new(alternatives: &[&str]) -> Spellings {
        let t = translate(&alternatives.join("|"), true).unwrap_or_else(|e| panic!("caliber spellings: {e}"));
        assert!(!t.fancy, "caliber spellings must not need look-around");
        let pattern = format!(
            r"(?i)(?:{})(?-i:[^a-zA-Z0-9\x{{130}}\x{{131}}\x{{17F}}\x{{212A}}])",
            t.pattern
        );
        let re = regex::RegexBuilder::new(&pattern)
            .size_limit(256 << 20)
            .dfa_size_limit(256 << 20)
            .build()
            .unwrap_or_else(|e| panic!("caliber spellings: {e}"));
        Spellings { re }
    }

    /// Python `finditer` spans.
    fn find_iter(&self, s: &str) -> Vec<(usize, usize)> {
        let hay = format!("{s}\0");
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= s.len() {
            let Some(m) = self.re.find_at(&hay, pos) else { break };
            let start = m.start();
            if s[..start]
                .chars()
                .next_back()
                .is_some_and(|c| ci_alnum(c) || c == '.' || c == '$')
            {
                pos = start + s[start..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            let end = m.end() - hay[..m.end()].chars().next_back().map_or(0, char::len_utf8);
            out.push((start, end));
            pos = end.max(start + 1);
        }
        out
    }
}

struct Index {
    calibers: Vec<Caliber>,
    by_id: HashMap<&'static str, usize>,
    by_key: HashMap<String, usize>,
    big: Spellings,
    context: Vec<(Rx, usize)>,
    gram: Rx,
}

fn rows() -> &'static [Row] {
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(|| {
        let doc = json::parse(include_str!("../data/calibers.json"));
        let list = doc.get("calibers").map(json::Value::as_array).unwrap_or(&[]);
        list.iter()
            .map(|c| {
                let s = |k: &str| c.get(k).and_then(json::Value::as_str).unwrap_or("").to_owned();
                Row {
                    id: s("id"),
                    name: s("name"),
                    kind: CaliberKind::from_data(&s("type")),
                    url: s("url"),
                    site_name: s("site_name"),
                    aliases: c.get("aliases").map(json::Value::strings).unwrap_or_default(),
                    n: c.get("n").and_then(json::Value::as_f64).unwrap_or(0.0),
                }
            })
            .collect()
    })
}

/// The full spelling table: `(key, Python pattern, caliber index)` in the order the big pattern tries them.
fn spelling_table(rows: &[Row]) -> Vec<(String, String, usize)> {
    let mut conflict: Vec<(String, Vec<(f64, usize)>)> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    let mut pats: HashMap<String, String> = HashMap::new();
    for (ci, c) in rows.iter().enumerate() {
        let names = [c.name.as_str(), c.site_name.as_str()]
            .into_iter()
            .chain(c.aliases.iter().map(String::as_str))
            .chain(extra(&c.name).iter().copied());
        for n in names {
            let own = unescape(n).to_lowercase();
            let mut vs = variants(n);
            if !is_bare(&own) {
                vs.push(own.clone());
            }
            for v in vs {
                let k = key(&v);
                if k.is_empty() || !k.bytes().any(|b| b.is_ascii_digit()) {
                    continue;
                }
                let at = *slot.entry(k.clone()).or_insert_with(|| {
                    conflict.push((k.clone(), Vec::new()));
                    conflict.len() - 1
                });
                let cands = &mut conflict[at].1;
                if cands.iter().all(|&(_, i)| i != ci) {
                    cands.push((c.n, ci));
                }
                pats.entry(k).or_insert_with(|| {
                    if v != own || !v.contains('/') {
                        spelling_rx(&v)
                    } else {
                        py_escape(&v)
                    }
                });
            }
        }
    }
    let mut table = Vec::new();
    for (k, mut cands) in conflict {
        cands.sort_by(|a, b| b.0.total_cmp(&a.0));
        // one spelling, several calibers: keep the one with 10x+ more listings, otherwise the spelling is ambiguous
        if cands.len() == 1 || cands[0].0 >= 10.0 * cands[1].0.max(1.0) {
            let p = pats.remove(&k).unwrap_or_default();
            table.push((k, p, cands[0].1));
        }
    }
    // longer patterns first (stable, like Python's sorted)
    table.sort_by_key(|(_, p, _)| std::cmp::Reverse(p.chars().count()));
    table
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let rows = rows();
        let calibers: Vec<Caliber> = rows
            .iter()
            .map(|r| Caliber {
                id: &r.id,
                name: &r.name,
                kind: r.kind,
                url: &r.url,
            })
            .collect();
        let by_id: HashMap<&'static str, usize> = calibers.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
        let table = spelling_table(rows);
        let alternatives: Vec<&str> = table.iter().map(|(_, p, _)| p.as_str()).collect();
        let big = Spellings::new(&alternatives);
        let by_key = table.into_iter().map(|(k, _, i)| (k, i)).collect();
        let context = CONTEXT.iter().map(|(p, id)| (Rx::new(p, false), by_id[id])).collect();
        Index {
            calibers,
            by_id,
            by_key,
            big,
            context,
            gram: Rx::new(r"^\d+\s?g$", false),
        }
    })
}

fn prep(text: &str) -> String {
    let s = unescape(text)
        .to_lowercase()
        .replace('×', "x")
        .replace(['”', '″'], "\"")
        .replace(['®', '™'], "");
    collapse_ws(&s)
}

/// All calibers mentioned in `text`, in order of appearance, without duplicates.
///
/// ```
/// let names: Vec<&str> = gunsnprices::find_calibers("Ruger GP100 .357 Mag / .38 Special 4.2in Revolver")
///     .iter()
///     .map(|c| c.name)
///     .collect();
/// assert_eq!(names, [".357 Mag", ".38 S&W Special"]);
/// ```
pub fn find_calibers(text: &str) -> Vec<Caliber> {
    let ix = index();
    let s = prep(text);
    let co2 = s.contains("co2");
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for (a, b) in ix.big.find_iter(&s) {
        let m = &s[a..b];
        // "12g" next to CO2 is a 12-gram CO2 cartridge, not 12 gauge
        if co2 && ix.gram.is_match(m) {
            continue;
        }
        if let Some(&i) = ix.by_key.get(&key(m)) {
            hits.push((a, i));
        }
    }
    for (rx, i) in &ix.context {
        hits.extend(rx.find_iter(&s).into_iter().map(|(a, _)| (a, *i)));
    }
    hits.sort_by_key(|h| h.0);
    let mut out: Vec<Caliber> = Vec::new();
    for (_, i) in hits {
        let c = ix.calibers[i];
        if !out.iter().any(|x| x.id == c.id) {
            out.push(c);
        }
    }
    out
}

/// The first caliber mentioned in `text`, or `None`.
///
/// ```
/// assert_eq!(gunsnprices::find_caliber("Hornady 6.5 CM 140gr ELD Match").unwrap().id, "6-5mmcreedmoor");
/// ```
pub fn find_caliber(text: &str) -> Option<Caliber> {
    find_calibers(text).into_iter().next()
}

/// Normalize a caliber field ("9mm Luger", "5.56 NATO", "12 GA") to a [`Caliber`].
///
/// The whole value is looked up first; if it isn't a known spelling, the first caliber found inside it is returned.
///
/// ```
/// let c = gunsnprices::normalize_caliber("308 WIN (7.62X51 NATO)").unwrap();
/// assert_eq!(c.name, ".308 Win");
/// ```
pub fn normalize_caliber(value: &str) -> Option<Caliber> {
    let ix = index();
    match ix.by_key.get(&key(value)) {
        Some(&i) => Some(ix.calibers[i]),
        None => find_caliber(value),
    }
}

/// The full caliber list, sorted by name.
pub fn list_calibers() -> Vec<Caliber> {
    let mut all = index().calibers.clone();
    all.sort_by(|a, b| a.name.cmp(b.name));
    all
}

/// The caliber with this id (`"9x19mmparabellum"`), or `None`.
pub fn caliber_by_id(id: &str) -> Option<Caliber> {
    let ix = index();
    ix.by_id.get(id).map(|&i| ix.calibers[i])
}

#[cfg(test)]
pub(crate) fn debug_spellings() -> Vec<(String, String, &'static str)> {
    let rows = rows();
    spelling_table(rows)
        .into_iter()
        .map(|(k, p, i)| (k, p, rows[i].id.as_str()))
        .collect()
}
