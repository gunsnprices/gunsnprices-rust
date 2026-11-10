//! Identify the firearm model in a product title: brand -> family -> model.
//!
//! The rules are GunsNPrices' hand-built model dictionaries (50 brands). They are the same rules that assign models to
//! listings on <https://gunsnprices.com>, so [`identify_model`] returns the same model names you see on the site.

use crate::html::unescape;
use crate::json::{self, Value};
use crate::pyre::{back_chars, char_index, collapse_ws, py_escape, py_strip, py_template, AnyRx, Rx, Template};
use crate::Options;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

/// Where the model was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Source {
    /// The model is named in the title.
    Title,
    /// The title only names the brand; the model was decoded from the manufacturer part number (Glock).
    Mpn,
}

impl Source {
    /// `"title"` or `"mpn"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Title => "title",
            Source::Mpn => "mpn",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of [`identify_model`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct ModelMatch {
    /// Brand as named on GunsNPrices, e.g. `"Glock"`.
    pub brand: String,
    /// Model family, e.g. `"Glock 19"`.
    pub family: String,
    /// Most specific model, e.g. `"Glock 19 Gen 5 MOS"`.
    pub model: String,
    /// `"5"`, `"V"`, `"V3"` ... or `""` when unknown.
    pub generation: String,
    /// Configurations found, e.g. `["MOS"]`, `["Compact"]`.
    pub configs: Vec<String>,
    /// Features found, e.g. `["Threaded Barrel", "FDE"]`.
    pub features: Vec<String>,
    /// Read from the title or decoded from the part number.
    pub source: Source,
}

impl fmt::Display for ModelMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.model)
    }
}

// ---------- shared rules ----------

struct Shared {
    airgun: Rx,
    acc_noun: Rx,
    acc_included: Rx,
    gun_words: Rx,
    gen: Rx,
    gen_v: Rx,
    leading_code: Rx,
    mpn_in_title: Rx,
    mpn_gen_us: Rx,
    mpn_aut: Rx,
    mpn_letter: Rx,
    mpn_tb: Rx,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared {
        // airguns and airsoft are not firearms (the site filters them by category; here we go by the title)
        airgun: Rx::new(
            r"\bair\s*-?\s*(?:gun|rifle|pistol)s?\b|\bairgun|\bairsoft|\bbb\s*(?:gun|pistol|rifle)|\bpellets?\b|\bco2\b",
            true,
        ),
        // accessories named after a gun ("Glock 19 holster", "Sig P365 XL 12rd Magazine", "Magpul PMAG for Glock 19").
        // A title that describes a gun (pistol/rifle/..., "15+1", a barrel length) or mentions the item as included
        // ("w/ 3 magazines", "no magazine") is not an accessory.
        acc_noun: Rx::new(
            concat!(
                r"\b(?:holsters?|magazines?(?!\s*(?:pattern|compatible|well|release|disconnect|safety|catch|cut|tube|extension|fed)\b)|pmags?|slings?|",
                r"grip panels?|scope mounts?|recoil pads?|speed ?loaders?|mag(?:azine)? loaders?|cleaning kits?|pouch(?:es)?|snap caps?|replacement barrels?|",
                r"conversion kits?|parts kits?)\b"
            ),
            false,
        ),
        acc_included: Rx::new(
            concat!(
                r"(?:(?:\bw/|\bwith\b|\bwithout\b|\bno\b|\band\b|&|\+|\bincl\w*|\bplus\b|\(\d\)|\b[1-9]\b|\btwo\b|\bthree\b|\bfour\b|,|\bor\b|\bextra\b|",
                r"\bspare\b|\bfactory\b|\bfits\b|\btakes\b|\baccepts\b|\buses\b)[^,]{0,14}|/\s*)$"
            ),
            false,
        ),
        gun_words: Rx::new(
            concat!(
                r"\b(?:pistol|rifle|shotgun|revolver|handgun|carbine|firearm|semi-?auto(?:matic)?|bolt[- ]?action|pump|lever[- ]?action|single[- ]?shot|",
                r#"o/u|over/under|sxs|sbr|derringer)\b|\d+\+1\b|\d+(?:\.\d+)?\s*-?\s*(?:"|in\b|inch)"#
            ),
            false,
        ),
        gen: Rx::new(r"\bgen(?:eration)?\.?\s*-?\s*([1-6])\b|\b([1-6])(?:st|nd|rd|th)\s*gen\b|\bg([1-6])\b", false),
        gen_v: Rx::new(r"^\s*v\b", false),
        leading_code: Rx::new(r"^(\d{2})(x)?\b", false),
        mpn_in_title: Rx::new(r"\b(?:gl)?[pue][a-z0-9]\d{3}[a-z0-9-]*|\bg\d{2}c?\d{3}us\b|\bg\d{4}[a-z]*aut\b", true),
        mpn_gen_us: Rx::new(r"^G(\d{2}C?)([345])(\d{2})US$", true),
        mpn_aut: Rx::new(r"^G(\d{2})\d{2}[A-Z]*AUT$", true),
        mpn_letter: Rx::new(r"^(?:GL)?([PUE])([A-Z0-9])(\d{2})(\d)", true),
        mpn_tb: Rx::new(r"\d(?:mos)?tb\b", true),
    })
}

fn is_accessory(title: &str) -> bool {
    let s = shared();
    let t = unescape(&unescape(title)).to_lowercase();
    if s.gun_words.is_match(&t) {
        return false;
    }
    s.acc_noun
        .find_iter(&t)
        .into_iter()
        .any(|(start, _)| !s.acc_included.is_match(&t[back_chars(&t, start, 16)..start]))
}

fn translate_dashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '‐' | '‑' | '–' | '—' => out.push('-'),
            '“' | '”' => out.push('"'),
            '’' => out.push('\''),
            '|' => out.push_str(" | "),
            '*' => out.push(' '),
            '®' | '™' => {}
            c => out.push(c),
        }
    }
    out
}

/// Lookup key of a brand name: lower case letters and digits only.
fn key(s: &str) -> String {
    unescape(s)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

// ---------- dictionaries ----------

struct TermModel {
    name: String,
    family: String,
    rx: Rx,
    not: Option<Rx>,
    configs: Vec<String>,
    implied_configs: Vec<String>,
}

struct Numeric {
    codes: HashMap<String, String>,
    find: Rx,
    own: Rx,
    bare: Option<Rx>,
    mpn_model_alias: HashMap<String, String>,
    mpn_letter_model: HashMap<String, String>,
    mpn_letter_gen: HashMap<String, String>,
    no_gen_models: Vec<String>,
}

struct Config {
    name: String,
    rx: Rx,
    /// `(?<![a-z0-9])NAME(?![a-z0-9])`: the config is already spelled out in a model name
    in_name: Rx,
}

struct Dict {
    brand: String,
    rewrite: Vec<(Rx, Template)>,
    exclude: Option<AnyRx>,
    exclude_soft: Option<AnyRx>,
    block: Option<AnyRx>,
    brand_any: Rx,
    brand_start: Option<Rx>,
    configs: Vec<Config>,
    features: Vec<(String, Rx)>,
    gens: Vec<(String, Rx)>,
    brand_names: HashSet<String>,
    deny_names: HashSet<String>,
    family_display: HashMap<String, String>,
    other_brand_models: Vec<String>,
    needed: HashMap<String, Vec<String>>,
    feature_order: Vec<String>,
    implied: HashMap<String, HashMap<String, Vec<String>>>,
    terms: Vec<TermModel>,
    numeric: Option<Numeric>,
}

fn any_rx(v: Option<&Value>) -> Option<AnyRx> {
    AnyRx::new(&v.map(Value::strings).unwrap_or_default(), true)
}

fn string_map(v: Option<&Value>) -> HashMap<String, String> {
    v.map(Value::string_pairs).unwrap_or_default().into_iter().collect()
}

fn pattern_pairs(v: Option<&Value>) -> Vec<(String, Rx)> {
    v.map(Value::string_pairs)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, p)| {
            let rx = Rx::new(&p, true);
            (k, rx)
        })
        .collect()
}

fn load_dict(d: &Value) -> Dict {
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let brand_word = s("brand_word");
    let brand_rx = s("brand_rx");
    let terms = s("mode") == "terms";
    let rewrite = d
        .get("title_rewrite")
        .map(Value::string_pairs)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (Rx::new(&k, true), py_template(&v).unwrap_or_else(|e| panic!("{e}"))))
        .collect();
    let brand_any = if brand_rx.is_empty() {
        Rx::new(&format!(r"(?<![a-z]){}(?![a-z])", py_escape(&brand_word)), true)
    } else {
        Rx::new(&brand_rx, true)
    };
    let brand_start = (!brand_rx.is_empty()).then(|| Rx::new(&format!(r"^\W*(?:{brand_rx})"), true));
    let configs = d
        .get("configs")
        .map(Value::string_pairs)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, p)| Config {
            rx: Rx::new(&p, true),
            in_name: Rx::new(&format!(r"(?<![a-z0-9]){}(?![a-z0-9])", py_escape(&name)), true),
            name,
        })
        .collect();
    let needed = d
        .get("needed")
        .map(Value::as_object)
        .unwrap_or(&[])
        .iter()
        .map(|(k, v)| (k.clone(), v.strings()))
        .collect();
    let implied = d
        .get("config_implied_by_gen_family")
        .map(Value::as_object)
        .unwrap_or(&[])
        .iter()
        .map(|(cfg, fams)| {
            let fams = fams.as_object().iter().map(|(f, g)| (f.clone(), g.strings())).collect();
            (cfg.clone(), fams)
        })
        .collect();
    let models = d.get("models");
    let term_models = if terms {
        models
            .map(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .map(|m| {
                let ms = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
                let not = ms("not");
                TermModel {
                    name: ms("name"),
                    family: ms("family"),
                    rx: Rx::new(&ms("match"), true),
                    not: (!not.is_empty()).then(|| Rx::new(&not, true)),
                    configs: m.get("configs").map(Value::strings).unwrap_or_default(),
                    implied_configs: m.get("implied_configs").map(Value::strings).unwrap_or_default(),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let numeric = (!terms).then(|| {
        let bw = py_escape(&brand_word);
        let tokens = d.get("bare_model_tokens").map(Value::strings).unwrap_or_default();
        Numeric {
            codes: string_map(models),
            find: Rx::new(
                &format!(
                    concat!(
                        r"(?<![a-z0-9.])(?:{bw}\s*-?\s*g?\s*-?\s*|g)(\d{{2}})(?:(sf|x|c|l|m|s)|\s(sf|x)(?![a-z0-9]))?",
                        r"(?:g([1-6])(?:us)?|(v))?(fs|mos|rtf)?",
                        r#"(?![a-z0-9&"+/])(?!\s*-?\s*(?:round|rounds|rd|rds|rnd|mag|mags|magazine)\b)"#
                    ),
                    bw = bw
                ),
                true,
            ),
            own: Rx::new(&format!(r"^\W*{bw}\b"), true),
            bare: (!tokens.is_empty()).then(|| Rx::new(&format!(r"\b({})\b", tokens.join("|")), false)),
            mpn_model_alias: string_map(d.get("mpn_model_alias")),
            mpn_letter_model: string_map(d.get("mpn_letter_model")),
            mpn_letter_gen: string_map(d.get("mpn_letter_gen")),
            no_gen_models: d.get("no_gen_models").map(Value::strings).unwrap_or_default(),
        }
    });
    Dict {
        brand: s("brand"),
        rewrite,
        exclude: any_rx(d.get("exclude")),
        exclude_soft: any_rx(d.get("exclude_soft")),
        block: any_rx(d.get("other_brand_block")),
        brand_any,
        brand_start,
        configs,
        features: pattern_pairs(d.get("features")),
        gens: pattern_pairs(d.get("gens")),
        brand_names: d
            .get("brand_names")
            .map(Value::strings)
            .unwrap_or_default()
            .iter()
            .map(|x| key(x))
            .collect(),
        deny_names: d
            .get("deny_brand_names")
            .map(Value::strings)
            .unwrap_or_default()
            .iter()
            .map(|x| key(x))
            .collect(),
        family_display: string_map(d.get("family_display")),
        other_brand_models: d.get("other_brand_models").map(Value::strings).unwrap_or_default(),
        needed,
        feature_order: d.get("feature_order").map(Value::strings).unwrap_or_default(),
        implied,
        terms: term_models,
        numeric,
    }
}

fn dicts() -> &'static [Dict] {
    static DICTS: OnceLock<Vec<Dict>> = OnceLock::new();
    DICTS.get_or_init(|| {
        let doc = json::parse(include_str!("../data/models.json"));
        doc.get("dicts").map(Value::as_array).unwrap_or(&[]).iter().map(load_dict).collect()
    })
}

fn norm(s: &str, d: &Dict) -> String {
    let s = translate_dashes(&unescape(s));
    let mut s = py_strip(&collapse_ws(&s.to_lowercase())).to_owned();
    for (rx, to) in &d.rewrite {
        s = rx.replace_all(&s, to);
    }
    s
}

/// Brands covered by the model dictionaries, in dictionary order.
///
/// ```
/// let brands = gunsnprices::list_brands();
/// assert_eq!(brands.len(), 50);
/// assert!(brands.contains(&"Glock"));
/// ```
pub fn list_brands() -> Vec<&'static str> {
    dicts().iter().map(|d| d.brand.as_str()).collect()
}

/// What a dictionary found: code (model name or Glock code), family, generation, configs, features, source.
struct Hit {
    code: String,
    family: String,
    gen: String,
    configs: Vec<String>,
    features: Vec<String>,
    src: Source,
}

// ---------- numeric mode (Glock): codes like 19, 19X, 43X, 20SF; generation from the title or the part number ----------

struct Found {
    code: String,
    num: String,
    end: usize,
    mos: bool,
    gen: String,
}

fn find_models(t: &str, n: &Numeric) -> Vec<Found> {
    let mut found = Vec::new();
    for m in n.find.captures_iter(t) {
        let num = m.str(1).to_lowercase();
        let suf = (if m.get(2).is_some_and(|x| !x.is_empty()) { m.str(2) } else { m.str(3) }).to_uppercase();
        let both = format!("{num}{suf}");
        let code = if n.codes.contains_key(&both) {
            both
        } else if n.codes.contains_key(&num) {
            num.clone()
        } else {
            continue;
        };
        let gen = if !m.str(4).is_empty() {
            m.str(4).to_owned()
        } else if !m.str(5).is_empty() {
            "V".to_owned()
        } else {
            String::new()
        };
        found.push(Found {
            code,
            num,
            end: m.end(),
            mos: m.str(6).to_lowercase() == "mos",
            gen,
        });
    }
    found
}

struct PartNumber {
    code: String,
    mpn: String,
    gen: String,
}

fn decode_mpn(name: &str, mpn: &str, sku: &str, n: &Numeric) -> Option<PartNumber> {
    let s = shared();
    let mut cands: Vec<&str> = vec![mpn, sku];
    cands.extend(s.mpn_in_title.find_iter(name).into_iter().map(|(a, b)| &name[a..b]));
    for c in cands {
        let c = py_strip(c);
        if let Some(m) = s.mpn_gen_us.captures(c) {
            let code = m.str(1).to_uppercase();
            if n.codes.contains_key(&code) {
                return Some(PartNumber { code, mpn: c.to_owned(), gen: m.str(2).to_owned() });
            }
        }
        if let Some(m) = s.mpn_aut.captures(c) {
            if n.codes.contains_key(m.str(1)) {
                return Some(PartNumber { code: m.str(1).to_owned(), mpn: c.to_owned(), gen: String::new() });
            }
        }
        let Some(m) = s.mpn_letter.captures(c) else { continue };
        let letter = m.str(2).to_uppercase();
        let num = m.str(3).to_owned();
        let num = n.mpn_model_alias.get(&num).cloned().unwrap_or(num);
        let code = n.mpn_letter_model.get(&format!("{letter}{num}")).cloned().unwrap_or(num);
        if !n.codes.contains_key(&code) {
            continue;
        }
        let gen = n.mpn_letter_gen.get(&letter).cloned().unwrap_or_default();
        return Some(PartNumber { code, mpn: c.to_owned(), gen });
    }
    None
}

fn parse_numeric(t: &str, name: &str, opts: &Options<'_>, d: &Dict, n: &Numeric) -> Option<Hit> {
    let s = shared();
    let pm = decode_mpn(name, opts.mpn.unwrap_or(""), opts.sku.unwrap_or(""), n);
    if pm.is_none() && d.exclude_soft.as_ref().is_some_and(|rx| rx.is_match(t)) {
        return None;
    }
    let own = match opts.brand {
        Some(b) => d.brand_names.contains(&key(b)) || pm.is_some(),
        None => pm.is_some() || n.own.is_match(t),
    };
    let mut models = find_models(t, n);
    if models.is_empty() && own {
        if let Some(m) = s.leading_code.captures(t) {
            let code = format!("{}{}", m.str(1), m.str(2).to_uppercase());
            if n.codes.contains_key(&code) {
                models.push(Found { code, num: m.str(1).to_owned(), end: m.end(), mos: false, gen: String::new() });
            }
        }
    }
    if models.is_empty() && own {
        if let Some(m) = n.bare.as_ref().and_then(|rx| rx.captures(t)) {
            let tok = m.str(1);
            models.push(Found {
                code: tok.to_uppercase(),
                num: tok.chars().take(2).collect(),
                end: m.end(),
                mos: false,
                gen: String::new(),
            });
        }
    }
    if !own {
        if models.is_empty() || d.block.as_ref().is_some_and(|rx| rx.is_match(t)) {
            return None;
        }
    }
    let mut src = Source::Title;
    let first = models.first();
    let code = if let Some(first) = first {
        let mut code = first.code.clone();
        for x in &models {
            if x.num == first.num && x.code.chars().count() > code.chars().count() {
                code = x.code.clone();
            }
        }
        code
    } else if let (true, Some(p)) = (own, &pm) {
        src = Source::Mpn;
        p.code.clone()
    } else {
        return None;
    };
    let mut gen = String::new();
    if let Some(f) = first.filter(|f| !f.gen.is_empty()) {
        gen = f.gen.clone();
    } else if let Some(m) = s.gen.captures(t) {
        gen = [1, 2, 3].iter().map(|&i| m.str(i)).find(|g| !g.is_empty()).unwrap_or("").to_owned();
    } else if first.is_some_and(|f| s.gen_v.is_match(&t[f.end..])) {
        gen = "V".to_owned();
    } else if let Some(p) = pm.as_ref().filter(|p| !p.gen.is_empty() && p.code == code) {
        gen = p.gen.clone();
    }
    if n.no_gen_models.contains(&code) && gen != "V" {
        gen.clear();
    }
    let mos = first.is_some_and(|f| f.mos) || pm.as_ref().is_some_and(|p| p.mpn.to_lowercase().contains("mos"));
    let configs = d
        .configs
        .iter()
        .filter(|c| c.rx.is_match(t) || (c.name == "MOS" && mos))
        .map(|c| c.name.clone())
        .collect();
    let mut features: Vec<String> = d.features.iter().filter(|(_, rx)| rx.is_match(t)).map(|(f, _)| f.clone()).collect();
    if pm.as_ref().is_some_and(|p| s.mpn_tb.is_match(&p.mpn)) && !features.iter().any(|f| f == "Threaded Barrel") {
        features.push("Threaded Barrel".to_owned());
    }
    let family = n.codes.get(&code).cloned().unwrap_or_default();
    Some(Hit { code, family, gen, configs, features, src })
}

// ---------- terms mode (all other brands): ordered list of model patterns, first match wins ----------

fn parse_terms(t: &str, brand: Option<&str>, d: &Dict) -> Option<Hit> {
    if brand.is_some_and(|b| d.deny_names.contains(&key(b))) {
        return None;
    }
    let own = brand.is_some_and(|b| d.brand_names.contains(&key(b)))
        || d.brand_start.as_ref().is_some_and(|rx| rx.is_match(t));
    if !own && (!d.brand_any.is_match(t) || d.block.as_ref().is_some_and(|rx| rx.is_match(t))) {
        return None;
    }
    let hit = d
        .terms
        .iter()
        .find(|m| m.rx.is_match(t) && !m.not.as_ref().is_some_and(|rx| rx.is_match(t)))?;
    if !own && !d.other_brand_models.is_empty() && !d.other_brand_models.contains(&hit.name) {
        return None;
    }
    let gen = d
        .gens
        .iter()
        .find(|(_, rx)| rx.is_match(t))
        .map(|(g, _)| g.clone())
        .unwrap_or_default();
    let mut configs = Vec::new();
    for c in &d.configs {
        if !hit.configs.contains(&c.name) {
            continue;
        }
        let implied_by_gen = d
            .implied
            .get(&c.name)
            .and_then(|fams| fams.get(&hit.family))
            .is_some_and(|gens| !gen.is_empty() && gens.contains(&gen));
        if c.rx.is_match(t) || implied_by_gen || hit.implied_configs.contains(&c.name) {
            configs.push(c.name.clone());
        }
    }
    let features = d.features.iter().filter(|(_, rx)| rx.is_match(t)).map(|(f, _)| f.clone()).collect();
    Some(Hit {
        code: hit.name.clone(),
        family: hit.family.clone(),
        gen,
        configs,
        features,
        src: Source::Title,
    })
}

/// Which matching dictionary wins: the product's own brand -> brand at the start of the title -> brand mentioned
/// earlier. Positions are counted in characters, like the reference implementation.
fn rank(title: &str, d: &Dict, brand: Option<&str>) -> (u8, usize) {
    if brand.is_some_and(|b| d.brand_names.contains(&key(b))) {
        return (0, 0);
    }
    let t = norm(title, d);
    match d.brand_any.find(&t) {
        Some((a, _)) if char_index(&t, a) <= 2 => (1, 0),
        Some((a, _)) => (2, char_index(&t, a)),
        None => (2, 1_000_000),
    }
}

fn words(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '&' | '+')))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn leaf(r: &Hit, d: &Dict) -> String {
    let mut s = if d.numeric.is_none() {
        if r.gen.is_empty() {
            r.code.clone()
        } else {
            format!("{} {}", r.code, r.gen)
        }
    } else {
        let mut s = format!("{} {}", d.brand, r.code);
        if r.gen == "V" {
            s.push_str(" V");
        } else if !r.gen.is_empty() {
            s.push_str(" Gen ");
            s.push_str(&r.gen);
        }
        s
    };
    for name in &r.configs {
        // skip a config already spelled out in the name ("Plus Shield" on "S&W M&P Shield Plus")
        let Some(c) = d.configs.iter().find(|c| &c.name == name) else { continue };
        if !c.in_name.is_match(&s) && !words(name).is_subset(&words(&s)) {
            s.push(' ');
            s.push_str(name);
        }
    }
    let needed = d.needed.get(&r.family);
    for f in &d.feature_order {
        if r.features.contains(f) && needed.is_some_and(|n| n.contains(f)) {
            s.push(' ');
            s.push_str(f);
        }
    }
    s
}

fn parse_one(title: &str, d: &Dict, opts: &Options<'_>) -> Option<Hit> {
    let t = norm(title, d);
    if d.exclude.as_ref().is_some_and(|rx| rx.is_match(&t)) {
        return None;
    }
    match &d.numeric {
        None => parse_terms(&t, opts.brand, d),
        Some(n) => {
            // the numeric dictionary (Glock) only runs when the brand is given or named in the title;
            // otherwise part-number decoding picks up other brands' codes (CVA "PR3110S" -> "Glock 31")
            let brand_given = opts.brand.is_some_and(|b| d.brand_names.contains(&key(b)));
            if !brand_given && !d.brand_any.is_match(&t) {
                return None;
            }
            parse_numeric(&t, title, opts, d, n)
        }
    }
}

/// Find the brand, model family and model in a firearm product title.
///
/// Returns `None` when no covered brand/model is recognized. Only firearms are covered: titles of accessories, parts
/// and airguns return `None`. Use [`identify_model_with`] to pass the brand, part number or SKU from a product feed.
///
/// ```
/// let m = gunsnprices::identify_model("Glock 19 Gen 5 MOS 9mm 4.02in 15rd Pistol PA195S203MOS").unwrap();
/// assert_eq!(m.family, "Glock 19");
/// assert_eq!(m.model, "Glock 19 Gen 5 MOS");
/// assert_eq!(gunsnprices::identify_model("Glock 19 holster"), None);
/// ```
pub fn identify_model(title: &str) -> Option<ModelMatch> {
    identify_model_with(title, Options::new())
}

/// [`identify_model`] with the brand, manufacturer part number and SKU from [`Options`] (price and kind are ignored).
///
/// The brand helps with titles that don't name it; Glock part numbers encode the model and generation.
///
/// ```
/// use gunsnprices::{identify_model_with, Options, Source};
///
/// let m = identify_model_with("GLOCK 9MM PISTOL 15RD", Options::new().brand("Glock").mpn("UA4350201")).unwrap();
/// assert_eq!(m.model, "Glock 43");
/// assert_eq!(m.source, Source::Mpn);
/// ```
pub fn identify_model_with(title: &str, opts: Options<'_>) -> Option<ModelMatch> {
    let opts = opts.normalized();
    if title.is_empty() || shared().airgun.is_match(title) || is_accessory(title) {
        return None;
    }
    let mut hits: Vec<((u8, usize), &Dict, Hit)> = Vec::new();
    for d in dicts() {
        if let Some(r) = parse_one(title, d, &opts) {
            hits.push((rank(title, d, opts.brand), d, r));
        }
    }
    hits.sort_by_key(|h| h.0);
    let (_, d, r) = hits.into_iter().next()?;
    let family = match d.family_display.get(&r.family).filter(|f| !f.is_empty()) {
        Some(f) => f.clone(),
        None if d.numeric.is_some() => format!("{} {}", d.brand, r.family),
        None => r.family.clone(),
    };
    let model = leaf(&r, d);
    Some(ModelMatch {
        brand: d.brand.clone(),
        family,
        model,
        generation: r.gen,
        configs: r.configs,
        features: r.features,
        source: r.src,
    })
}

#[cfg(test)]
pub(crate) fn pattern_engines() -> (usize, usize) {
    // (patterns on the regex crate, patterns on fancy-regex) across the dictionaries
    let mut plain = 0;
    let mut fancy = 0;
    let mut count = |rx: &Rx| if rx.is_fancy() { fancy += 1 } else { plain += 1 };
    for d in dicts() {
        d.rewrite.iter().for_each(|(rx, _)| count(rx));
        [&d.exclude, &d.exclude_soft, &d.block].into_iter().flatten().flat_map(AnyRx::parts).for_each(&mut count);
        d.brand_start.iter().for_each(&mut count);
        count(&d.brand_any);
        d.configs.iter().for_each(|c| count(&c.rx));
        d.features.iter().chain(&d.gens).for_each(|(_, rx)| count(rx));
        for m in &d.terms {
            count(&m.rx);
            if let Some(rx) = &m.not {
                count(rx);
            }
        }
    }
    (plain, fancy)
}
