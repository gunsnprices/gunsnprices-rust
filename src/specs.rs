//! Numbers from product titles: magazine capacity, barrel length, rounds per box and cost per round.

use crate::html::unescape;
use crate::pyre::{back_chars, collapse_ws, py_float, py_int, Caps, Rx};
use std::sync::OnceLock;

fn prep(text: &str) -> String {
    let s = unescape(&unescape(text)).to_lowercase();
    let s = s
        .replace(['”', '″', '“'], "\"")
        .replace("''", "\"")
        .replace("’’", "\"")
        .replace(['®', '™'], "");
    collapse_ws(&s)
}

const NUM: &str = r"(\d{1,3}(?:,\d{3})+|\d{1,5})";
const INCH: &str = r#"(?:"|in\b(?!\s*(?:1\b|stock\b|one\b))\.?|inch(?:es)?\b|-inch\b|-in\b)"#;
// 18.5 / 16 1/2
const LEN: &str = r"(\d{1,2}(?:\.\d{1,3})?)(?:[\s-]+(\d{1,2})/(\d{1,2}))?";
const BBL: &str = r"(?:bbl|brl|barrel)\b";
const B: &str = r"(?<![a-z0-9.#$/])";

struct Rules {
    ammo: Rx,
    cap_plus: Rx,
    cap_rd: Rx,
    cap_pair: Rx,
    cap_mag: Rx,
    bar_tagged: [Rx; 2],
    bar_inch: Rx,
    not_barrel_after: Rx,
    not_barrel_before: Rx,
    shotgun: Rx,
    gauge_slash: Rx,
    rpb: [Rx; 4],
    box_case: Rx,
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| Rules {
        ammo: Rx::new(
            concat!(
                r"\b(?:ammo|ammunition|shotshells?|shot ?shells?|cartridges?|bullets?|primers?|slugs?|buckshot|pellets?|bbs?|grains?|\d+\s*gr|",
                r"per box|box of|rounds? box|rds?/box)\b|/\s*(?:box|bx|pk|pack|case|cs|tin)\b"
            ),
            false,
        ),
        // 15+1
        cap_plus: Rx::new(r"(?<![\d.:/x])(\d{1,3})\s*\+\s*1(?!\d)", false),
        cap_rd: Rx::new(
            r"(?<![\d.:/x])(\d{1,3})\s*-?\s*(?:rd|rds|rnd|rnds|round|rounds|shot|shots)\b(?!\s*/?\s*(?:per\b|box|bx|pack|pk|case))",
            false,
        ),
        // "13rd/15rd", "1-30Rd/1-10Rd Mag": ships with magazines of two sizes - take the larger one
        cap_pair: Rx::new(
            r"(?<![\d.:/x])(\d{1,3})\s*-?\s*(?:rd|rds|rnd|rnds|rounds?)?\s*/\s*(?:1?-)?(\d{1,3})\s*-?\s*(?:rd|rds|rnd|rnds|rounds?)\b",
            false,
        ),
        cap_mag: Rx::new(r#"\b(?:cap|capacity)\s*[:=]?\s*(\d{1,3})\b(?!\s*(?:"|in\b|gr\b))"#, false),
        bar_tagged: [
            // 20" barrel, 22 bbl
            Rx::new(&format!(r"(?<![\d.:/x#$-]){LEN}\s*-?\s*{INCH}?\s*{BBL}"), false),
            // barrel: 20"
            Rx::new(&format!(r"\b(?:bbl|brl|barrel)\s*(?:length)?\s*[:=]?\s*{LEN}\s*{INCH}"), false),
        ],
        bar_inch: Rx::new(&format!(r"(?<![\d.:/x#$-]){LEN}\s*-?\s*{INCH}"), false),
        not_barrel_after: Rx::new(
            r"\s*-?\s*(?:chamber|chbr|chmbr|shells?|m-?lok|mlok|handguard|hg\b|rail|keymod|quad|lop\b|length of pull|oal\b|twist|rate)",
            false,
        ),
        not_barrel_before: Rx::new(
            r"(?:lop|oal|overall|length of pull|pull|twist|1:|chamber|m-?lok|mlok|handguard|rail|sight radius)\s*[:=]?\s*$",
            false,
        ),
        shotgun: Rx::new(
            r"\b(?:\d{1,2}|410)\s*(?:ga|gauge|g)\b|\.410|shotgun|o/u|over/under|sxs|side by side|\b(?:12|16|20|28|410)/\d\d\b",
            false,
        ),
        // "O/U 12/28": gauge / barrel length (10 is left out: "10/22" is a Ruger rifle)
        gauge_slash: Rx::new(
            r"(?<![a-z0-9./$#-])(?:12|16|20|28|410)/(1[89](?:\.5)?|2\d|3[0-4])(?![\d/.])(?!\s*(?:ga|gauge|rd|round))",
            false,
        ),
        rpb: [
            Rx::new(
                &format!(
                    r"{B}{NUM}\s*-?\s*(?:rd|rds|round|rounds|rnd|rnds|ct|count|pcs|pc|cartridges|shells|shotshells|bullets)\b(?!\s*(?:mag|magazine|capacity|cap)\b)"
                ),
                false,
            ),
            Rx::new(&format!(r"{B}{NUM}\s*(?:/\s*|per\s+)(?:box|bx|pack|pk|case|cs|tin|can|bag|ct)\b"), false),
            Rx::new(&format!(r"(?:box|pack|case|tin|bx|bag|can) of\s*{NUM}"), false),
            Rx::new(&format!(r"{B}{NUM}\s*-?\s*(?:pack|pk|packs)\b"), false),
        ],
        // distributor notation "50/10", "20/25": rounds per box / boxes per case
        box_case: Rx::new(
            r#"(?<![\d.\-/])(5|10|15|18|20|25|30|40|50|100|200|250|500|1000)\s*/\s*(\d{1,3})(?![\d/."])"#,
            false,
        ),
    })
}

const CALIBER_SLASH: [(&str, &str); 11] = [
    ("30", "30"),
    ("45", "70"),
    ("30", "06"),
    ("22", "250"),
    ("25", "06"),
    ("38", "55"),
    ("44", "40"),
    ("32", "20"),
    ("38", "40"),
    ("25", "20"),
    ("32", "40"),
];

/// True if the title reads like ammunition (bullet weight, "ammo", "20/box", ...) rather than a firearm.
///
/// ```
/// assert!(gunsnprices::looks_like_ammo("Federal 9mm 115gr FMJ 50/Box"));
/// assert!(!gunsnprices::looks_like_ammo("Glock 19 Gen 5 9mm Pistol"));
/// ```
pub fn looks_like_ammo(title: &str) -> bool {
    rules().ammo.is_match(&prep(title))
}

fn int(s: &str) -> u64 {
    py_int(s).unwrap_or(0)
}

/// Magazine capacity of a firearm or magazine from its title ("17RD", "15+1", "10-shot"), or `None`.
///
/// For "15+1" the magazine capacity (15) is returned. Use it on firearm and magazine titles: in ammunition titles
/// "20 rounds" means rounds per box (see [`rounds_per_box`]).
///
/// ```
/// assert_eq!(gunsnprices::magazine_capacity("Glock 19 Gen5 9mm 4.02in 15+1 Black"), Some(15));
/// assert_eq!(gunsnprices::magazine_capacity("Springfield Hellcat Pro OSP 9mm 3.7in 15rd/17rd"), Some(17));
/// ```
pub fn magazine_capacity(title: &str) -> Option<u32> {
    let r = rules();
    let s = prep(title);
    if let Some(m) = r.cap_pair.captures(&s) {
        let n = int(m.str(1)).max(int(m.str(2)));
        if (1..=110).contains(&n) {
            return Some(n as u32);
        }
    }
    for rx in [&r.cap_plus, &r.cap_rd, &r.cap_mag] {
        for m in rx.captures_iter(&s) {
            let n = int(m.str(1));
            if (1..=110).contains(&n) {
                return Some(n as u32);
            }
        }
    }
    None
}

fn length(m: &Caps<'_>) -> f64 {
    let mut v = py_float(m.str(1)).unwrap_or(0.0);
    let (whole, part) = (m.str(2), m.str(3));
    if !whole.is_empty() && !part.is_empty() {
        let (a, b) = (int(whole), int(part));
        if [2, 4, 8, 16].contains(&b) && a < b {
            v += a as f64 / b as f64;
        }
    }
    v
}

/// Barrel length in inches from a firearm title (`4.25"`, "16 in", "20-inch", `16 1/2"`, "22 BBL"), or `None`.
///
/// Chamber length (3" on shotguns), twist rate, handguard and length-of-pull values are skipped.
///
/// ```
/// assert_eq!(gunsnprices::barrel_length("Ruger GP100 .357 Mag 4.2in Revolver"), Some(4.2));
/// assert_eq!(gunsnprices::barrel_length("Mossberg 590 Persuader 12/20 BL/SY 3\""), Some(20.0));
/// ```
pub fn barrel_length(title: &str) -> Option<f64> {
    let r = rules();
    let s = prep(title);
    for rx in &r.bar_tagged {
        for m in rx.captures_iter(&s) {
            let v = length(&m);
            if (1.5..=36.0).contains(&v) {
                return Some(v);
            }
        }
    }
    if let Some(m) = r.gauge_slash.captures(&s) {
        return py_float(m.str(1));
    }
    let shotgun = r.shotgun.is_match(&s);
    for m in r.bar_inch.captures_iter(&s) {
        let v = length(&m);
        if !(1.5..=36.0).contains(&v) {
            continue;
        }
        if r.not_barrel_after.match_at(&s, m.end())
            || r.not_barrel_before
                .is_match(&s[back_chars(&s, m.start(), 20)..m.start()])
        {
            continue;
        }
        // 3" chamber
        if shotgun && v <= 3.5 {
            continue;
        }
        return Some(v);
    }
    None
}

/// Number of rounds in one package of ammunition ("20rds", "50/box", "Box of 25", "1,000 Rounds"), or `None`.
///
/// ```
/// assert_eq!(gunsnprices::rounds_per_box("Federal American Eagle 9mm 115gr FMJ, 50/Box"), Some(50));
/// assert_eq!(gunsnprices::rounds_per_box("PMC Bronze 5.56 55gr FMJ 1,000 Rounds"), Some(1000));
/// ```
pub fn rounds_per_box(title: &str) -> Option<u32> {
    let r = rules();
    let s = prep(title);
    for rx in &r.rpb {
        if let Some(m) = rx.captures(&s) {
            let n = int(&m.str(1).replace(',', ""));
            if (1..=20000).contains(&n) {
                return Some(n as u32);
            }
        }
    }
    for m in r.box_case.captures_iter(&s) {
        let pair = (m.str(1), m.str(2));
        if !CALIBER_SLASH.contains(&pair) && !["2", "4", "8", "16", "32"].contains(&pair.1) {
            return Some(int(pair.0) as u32);
        }
    }
    None
}

/// Price of one round, rounded to 4 decimals like Python's `round(price / rounds, 4)`; `None` when the price is not
/// positive or `rounds` is 0.
///
/// ```
/// assert_eq!(gunsnprices::cost_per_round(18.99, 50), Some(0.3798));
/// assert_eq!(gunsnprices::cost_per_round(449.99, 1000), Some(0.45));
/// assert_eq!(gunsnprices::cost_per_round(0.0, 50), None);
/// ```
pub fn cost_per_round(price: f64, rounds: u32) -> Option<f64> {
    if price == 0.0 || rounds == 0 || price <= 0.0 {
        return None;
    }
    Some(round4(price / f64::from(rounds)))
}

/// Python's `round(x, 4)`: the decimal value nearest to `x` with 4 decimals (ties to even on the exact binary value).
fn round4(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    // Rust's fixed-precision formatting is exact (correctly rounded, ties to even), as is parsing back
    format!("{x:.4}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_matches_python() {
        // expected values from Python: round(x, 4)
        assert_eq!(round4(0.53125), 0.5312);
        assert_eq!(round4(0.00005), 0.0001);
        assert_eq!(round4(2.675e-1), 0.2675);
        assert_eq!(round4(1.00005), 1.0001);
        assert_eq!(cost_per_round(31.99, 20), Some(1.5995));
    }
}
