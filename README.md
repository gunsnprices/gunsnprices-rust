# gunsnprices

[![crates.io](https://img.shields.io/crates/v/gunsnprices.svg)](https://crates.io/crates/gunsnprices)
[![docs.rs](https://img.shields.io/docsrs/gunsnprices)](https://docs.rs/gunsnprices)
[![CI](https://github.com/gunsnprices/gunsnprices-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/gunsnprices/gunsnprices-rust/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20%2B%20CC%20BY%204.0%20data-blue)](#license)

Parse firearm and ammunition product titles into structured data: brand, model, caliber, magazine capacity, barrel
length, rounds per box and cost per round.

```rust
let info = gunsnprices::parse_title("Glock 19 Gen 5 MOS 9mm 4.02in 15rd Pistol PA195S203MOS");
assert_eq!(info.model.as_ref().unwrap().model, "Glock 19 Gen 5 MOS");
assert_eq!(info.caliber.unwrap().name, "9×19mm Parabellum");
assert_eq!(info.capacity, Some(15));
assert_eq!(info.barrel_length, Some(4.02));
```

This is the title parser behind [GunsNPrices](https://gunsnprices.com), a price comparison site that tracks 40+ US gun
and ammo stores. Store titles are messy (`G19 GEN5 9MM 15RD FS`, `O/U 12/28 BL/WD 3"`, `Federal 308 150gr ... 20rds`),
and matching the same product across stores starts with reading them reliably. The model dictionaries and the caliber
list in this crate are the ones the site uses to group listings, so you get the same model names you see on
[gunsnprices.com](https://gunsnprices.com).

Safe Rust (`#![forbid(unsafe_code)]`), thread-safe, data compiled into the binary. It is a port of the
[Python package](https://pypi.org/project/gunsnprices/) and gives identical results: the test suite checks the same
1,150 recorded store titles as the Python and [JavaScript](https://www.npmjs.com/package/gunsnprices) versions, and it
agrees with Python on all 63,755 titles of the GunsNPrices catalog.

## Install

```bash
cargo add gunsnprices
# with serde::Serialize on the result types
cargo add gunsnprices --features serde
```

Minimum supported Rust version: 1.71.

## What it recognizes

| Field | Example in a title | Result |
|---|---|---|
| Firearm or ammo | `... 150gr ... 20rds` | `Kind::Ammo` |
| Brand and model (50 brands) | `G19 Gen5 MOS`, part number `PA195S203` | `Glock 19 Gen 5 MOS` |
| Calibers (293) | `.357 Mag / .38 Spl`, `6.5 CM`, `12ga`, `300 BLK`, `7MM RM` | `.357 Mag`, `.38 S&W Special`, ... |
| Magazine capacity | `15+1`, `17RD`, `13rd/15rd`, `10-shot` | `15`, `17`, `15`, `10` |
| Barrel length, inches | `4.02in`, `16 1/2"`, `22 BBL`, `O/U 12/28` | `4.02`, `16.5`, `22.0`, `28.0` |
| Rounds per box | `50/Box`, `Box of 20`, `1,000 Rounds`, `20/25` | `50`, `20`, `1000`, `20` |
| Cost per round | price `449.99` + `1000 Rounds` | `0.45` |

Brands with model dictionaries: Barrett, Benelli, Beretta, Bergara, Browning, Cadex, Canik, Charles Daly, Chiappa,
Christensen Arms, Cimarron, CMMG, Colt, CVA, CZ, Daniel Defense, Desert Eagle, Diamondback, Fierce, FN, Glock,
Great Lakes, Henry, Heritage, HK, Howa, Kel-Tec, Kimber, Kriss, Marlin, Mossberg, Remington, Retay, Rossi, Ruger, Savage,
SCCY, Seekins, Shadow Systems, Sig Sauer, Smith & Wesson, Springfield, Stevens, Stoeger, Taurus, Tikka, Tristar, Walther,
Weatherby, Winchester.

## Usage

### Everything at once

```rust
use gunsnprices::{parse_title_with, Kind, Options};

let ammo = parse_title_with(
    "Federal American Eagle 5.56x45mm 55gr FMJ 1000 Rounds",
    Options::new().price(449.99),
);
assert_eq!(ammo.kind, Kind::Ammo);
assert_eq!(ammo.caliber.unwrap().name, "5.56×45mm NATO");
assert_eq!(ammo.rounds, Some(1000));
assert_eq!(ammo.cost_per_round, Some(0.45));
```

`parse_title(title)` and `parse_title_with(title, Options)` return a `ProductInfo`:

| Field | Type | Notes |
|---|---|---|
| `kind` | `Kind` | `Firearm` or `Ammo`; set `Options::kind` to skip the guess |
| `brand` | `String` | brand of the identified model, else the brand you passed, else `""` |
| `model` | `Option<ModelMatch>` | firearms only |
| `caliber` | `Option<Caliber>` | first caliber in the title |
| `calibers` | `Vec<Caliber>` | all calibers, in order |
| `capacity` | `Option<u32>` | magazine capacity (for `15+1` it is 15), firearms only |
| `barrel_length` | `Option<f64>` | inches, firearms only |
| `rounds` | `Option<u32>` | rounds per box, ammo only |
| `cost_per_round` | `Option<f64>` | ammo with a price only |

`Options` takes the `brand` (the manufacturer field from a feed, which helps with titles that don't name the brand),
`mpn`, `sku`, `price` and `kind`. With the part number it decodes Glock models when the title says nothing useful:

```rust
use gunsnprices::{identify_model_with, Options, Source};

let m = identify_model_with("GLOCK 9MM PISTOL 15RD", Options::new().brand("Glock").mpn("UA4350201")).unwrap();
assert_eq!(m.model, "Glock 43");
assert_eq!(m.source, Source::Mpn);
```

With the `serde` feature, `ProductInfo`, `ModelMatch` and `Caliber` serialize with the same JSON field names as the
Python and JavaScript packages (`barrel_length`, `cost_per_round`, caliber `type`).

### Models

```rust
let m = gunsnprices::identify_model("Sig Sauer P365 XL 9mm 3.7\" 12+1 Optic Ready").unwrap();
assert_eq!(m.family, "Sig Sauer P365");
assert_eq!(m.model, "Sig Sauer P365 XL");

// accessories, parts, airguns and airsoft are not firearms
assert_eq!(gunsnprices::identify_model("Glock 19 holster"), None);
assert_eq!(gunsnprices::identify_model("Umarex Glock 19 CO2 BB pistol"), None);
```

A `ModelMatch` has `brand`, `family`, `model` (the most specific name), `generation`, `configs` (such as `MOS`,
`Compact`), `features` (such as `Threaded Barrel`, `FDE`) and `source` (`Source::Title` or `Source::Mpn`).
`list_brands()` returns the 50 brands.

### Calibers

```rust
use gunsnprices::{find_calibers, normalize_caliber, CaliberKind};

let names: Vec<&str> = find_calibers("Henry Big Boy .44 Mag/.44 Spl 20in").iter().map(|c| c.name).collect();
assert_eq!(names, [".44 Magnum", ".44 S&W Special"]);

let c = normalize_caliber("308 WIN (7.62X51 NATO)").unwrap();
assert_eq!((c.id, c.name, c.kind), ("308win", ".308 Win", CaliberKind::Rifle));
assert_eq!(c.url, "https://gunsnprices.com/ammo/rifle-ammo/308-win");
```

Each `Caliber` is a small `Copy` value with a stable `id`, a display `name`, a `kind` (`Handgun`, `Rifle`, `Rimfire`,
`Shotgun`, `Airgun`, `Muzzleloader`) and the `url` of its GunsNPrices price comparison page. `normalize_caliber` is
meant for caliber fields in product feeds; `find_calibers` searches free text. `list_calibers()` returns all 293 and
`caliber_by_id("9x19mmparabellum")` looks one up.

### Numbers

```rust
use gunsnprices::{barrel_length, cost_per_round, magazine_capacity, rounds_per_box};

assert_eq!(magazine_capacity("Springfield Hellcat Pro OSP 9mm 3.7in 15rd/17rd"), Some(17));
assert_eq!(barrel_length("Mossberg 590 Persuader 12/20 BL/SY 3\""), Some(20.0)); // 3" is the chamber
assert_eq!(rounds_per_box("Winchester 308 150gr PowerMax Bonded 20rds"), Some(20));
assert_eq!(cost_per_round(31.99, 20), Some(1.5995));
```

### Command line

```bash
cargo install gunsnprices --features cli
gunsnprices "Ruger GP100 .357 Mag / .38 Special 4.2in 6-shot Revolver"
gunsnprices --price 18.99 "Federal 9mm 115gr FMJ 50/Box"
gunsnprices < titles.txt > parsed.jsonl      # one title per line in, JSON Lines out
```

```console
$ echo "Hornady 6.5 CM 140gr ELD Match 20rds" | gunsnprices
{"title":"Hornady 6.5 CM 140gr ELD Match 20rds","kind":"ammo","brand":"","model":null,"caliber":{"id":"6-5mmcreedmoor","name":"6.5mm Creedmoor","type":"rifle","url":"https://gunsnprices.com/ammo/rifle-ammo/6-5mm-creedmoor"},"calibers":[{"id":"6-5mmcreedmoor","name":"6.5mm Creedmoor","type":"rifle","url":"https://gunsnprices.com/ammo/rifle-ammo/6-5mm-creedmoor"}],"capacity":null,"barrel_length":null,"rounds":20,"cost_per_round":null}
```

Options: `--brand`, `--mpn`, `--sku`, `--price`, `--kind firearm|ammo`.

## Performance

The rules are compiled on first use (about 0.1 s in a release build); after that a title takes about 60 µs
(`cargo run --release --example bench`). Every function is thread-safe.

The rules are written in Python's regular expression syntax. Patterns without look-around run on the
[`regex`](https://crates.io/crates/regex) crate (linear time); the ones that need look-around run on
[`fancy-regex`](https://crates.io/crates/fancy-regex), behind a look-around-free prefilter that skips the backtracking
search when it cannot match.

## Accuracy

Measured with the reference implementation on 50,202 firearm and 13,555 ammunition listings from 40+ stores on
gunsnprices.com (October 2026), against the attributes stored on the site:

| What | Found in the title | Right when found |
|---|---|---|
| Model (32,317 listings with a known model) | 99.97% | 99.8% same name as the site |
| Caliber, firearms | 81% | 97.8% |
| Caliber, ammo | 91% | 94.9% |
| Magazine capacity | 46% | 95.6% |
| Barrel length | 58% | 94.4% |
| Rounds per box | 80% | 96.6% |

"Found" is limited by the titles themselves: many stores don't put the caliber or barrel length in the title at all.
Most of the "wrong" cases we checked by hand are errors in the store data, not in the parser.

## Data

The model dictionaries and caliber list ship inside the crate (`data/*.json`, the same files as the Python package,
embedded with `include_str!`). The same data, with listing counts and price levels, is published as open datasets:

- Hugging Face: [gunsnprices/firearm-calibers](https://huggingface.co/datasets/gunsnprices/firearm-calibers),
  [gunsnprices/firearm-models](https://huggingface.co/datasets/gunsnprices/firearm-models)
- GitHub: [gunsnprices/firearm-datasets](https://github.com/gunsnprices/firearm-datasets)
- MCP server for AI assistants, built on the same parser: [gunsnprices-mcp](https://github.com/gunsnprices/gunsnprices-mcp)

## Limitations

- Titles from US retailers, in English.
- Models are covered for the 50 brands above. Other brands still get calibers and numbers.
- Built for firearms and ammunition. Magazines, parts and accessories get calibers and numbers but no model.

## License

The code is under the [MIT License](LICENSE). The model and caliber data in `data/` is under
[CC BY 4.0](LICENSE-DATA): you can use it for anything, including commercial use, if you credit
**GunsNPrices (gunsnprices.com)** with a link. The crate's SPDX license expression is `MIT AND CC-BY-4.0`.

Questions, corrections and missing models: open an issue or write to app@gunsnprices.com.
Current gun and ammo prices: [gunsnprices.com](https://gunsnprices.com).
