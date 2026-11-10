//! Command line: `gunsnprices "Glock 19 Gen 5 9mm 15rd"` prints the parsed title as JSON.
//!
//! Without a title, titles are read from standard input, one per line (JSON Lines out).

use gunsnprices::{parse_title_with, Kind, Options};
use std::io::{self, BufRead, BufWriter, Write};
use std::process::ExitCode;

const USAGE: &str = "\
Parse firearm and ammunition product titles into JSON: brand, model, caliber, magazine capacity,
barrel length, rounds per box and cost per round.

Usage: gunsnprices [OPTIONS] [TITLE]...

With TITLE (the words are joined with spaces), prints one JSON object.
Without TITLE, reads titles from standard input, one per line, and prints JSON Lines.

Options:
      --brand <BRAND>  manufacturer, if known
      --mpn <MPN>      manufacturer part number, if known (Glock part numbers encode the model)
      --sku <SKU>      store SKU, if known
      --price <PRICE>  listing price (gives cost per round for ammo)
      --kind <KIND>    firearm or ammo (skips the guess)
  -h, --help           print this help
  -V, --version        print the version

Examples:
  gunsnprices \"Ruger GP100 .357 Mag / .38 Special 4.2in 6-shot Revolver\"
  gunsnprices --price 18.99 \"Federal 9mm 115gr FMJ 50/Box\"
  gunsnprices < titles.txt > parsed.jsonl

Data: GunsNPrices (https://gunsnprices.com), CC BY 4.0.";

struct Args {
    brand: Option<String>,
    mpn: Option<String>,
    sku: Option<String>,
    price: Option<f64>,
    kind: Option<Kind>,
    words: Vec<String>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args {
        brand: None,
        mpn: None,
        sku: None,
        price: None,
        kind: None,
        words: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    let mut only_words = false;
    while let Some(arg) = it.next() {
        if only_words || !arg.starts_with("--") && arg != "-h" && arg != "-V" {
            a.words.push(arg);
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n.to_owned(), Some(v.to_owned())),
            None => (arg.clone(), None),
        };
        let value = |it: &mut dyn Iterator<Item = String>| {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match name.as_str() {
            "--" => only_words = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("gunsnprices {}", gunsnprices::VERSION);
                return Ok(None);
            }
            "--brand" => a.brand = Some(value(&mut it)?),
            "--mpn" => a.mpn = Some(value(&mut it)?),
            "--sku" => a.sku = Some(value(&mut it)?),
            "--price" => {
                let v = value(&mut it)?;
                a.price = Some(v.parse().map_err(|_| format!("--price: not a number: {v}"))?);
            }
            "--kind" => a.kind = Some(value(&mut it)?.parse().map_err(|e| format!("--kind: {e}"))?),
            _ => return Err(format!("unknown option {name} (see --help)")),
        }
    }
    Ok(Some(a))
}

fn run() -> Result<(), String> {
    let Some(a) = parse_args()? else { return Ok(()) };
    let mut opts = Options::new();
    if let Some(b) = &a.brand {
        opts = opts.brand(b);
    }
    if let Some(m) = &a.mpn {
        opts = opts.mpn(m);
    }
    if let Some(s) = &a.sku {
        opts = opts.sku(s);
    }
    if let Some(p) = a.price {
        opts = opts.price(p);
    }
    if let Some(k) = a.kind {
        opts = opts.kind(k);
    }
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let io_err = |e: io::Error| e.to_string();
    if !a.words.is_empty() {
        let info = parse_title_with(&a.words.join(" "), opts);
        let json = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
        writeln!(out, "{json}").map_err(io_err)?;
        return out.flush().map_err(io_err);
    }
    for line in io::stdin().lock().lines() {
        let line = line.map_err(io_err)?;
        let title = line.trim();
        if title.is_empty() {
            continue;
        }
        let json = serde_json::to_string(&parse_title_with(title, opts)).map_err(|e| e.to_string())?;
        if let Err(e) = writeln!(out, "{json}") {
            // stop quietly when the reader goes away (`| head`)
            return if e.kind() == io::ErrorKind::BrokenPipe { Ok(()) } else { Err(e.to_string()) };
        }
    }
    out.flush().map_err(io_err)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gunsnprices: {e}");
            ExitCode::from(2)
        }
    }
}
