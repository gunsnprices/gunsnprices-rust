//! Parse firearm and ammunition product titles into structured data: brand, model, caliber, magazine capacity,
//! barrel length, rounds per box and cost per round.
//!
//! ```
//! let info = gunsnprices::parse_title("Glock 19 Gen 5 MOS 9mm 4.02in 15rd Pistol PA195S203MOS");
//! let model = info.model.as_ref().unwrap();
//! assert_eq!(model.model, "Glock 19 Gen 5 MOS");
//! assert_eq!(info.caliber.unwrap().name, "9×19mm Parabellum");
//! assert_eq!(info.capacity, Some(15));
//! assert_eq!(info.barrel_length, Some(4.02));
//! ```
//!
//! This is the title parser behind [GunsNPrices](https://gunsnprices.com), a price comparison site that tracks 40+ US
//! gun and ammo stores. Store titles are messy (`G19 GEN5 9MM 15RD FS`, `O/U 12/28 BL/WD 3"`,
//! `Federal 308 150gr ... 20rds`), and matching the same product across stores starts with reading them reliably.
//! The model dictionaries (50 brands) and the caliber list (293 calibers) embedded in this crate are the ones the
//! site uses to group listings, so you get the same model names you see on gunsnprices.com.
//!
//! It is a port of the [Python package](https://pypi.org/project/gunsnprices/) and gives the same results: the test
//! suite checks 1,150 recorded store titles against the Python output.
//!
//! # What it recognizes
//!
//! | Field | Example in a title | Result |
//! |---|---|---|
//! | Firearm or ammo | `... 150gr ... 20rds` | [`Kind::Ammo`] |
//! | Brand and model | `G19 Gen5 MOS`, part number `PA195S203` | `Glock 19 Gen 5 MOS` |
//! | Calibers | `.357 Mag / .38 Spl`, `6.5 CM`, `12ga`, `300 BLK`, `7MM RM` | `.357 Mag`, `.38 S&W Special`, ... |
//! | Magazine capacity | `15+1`, `17RD`, `13rd/15rd`, `10-shot` | `15`, `17`, `15`, `10` |
//! | Barrel length, inches | `4.02in`, `16 1/2"`, `22 BBL`, `O/U 12/28` | `4.02`, `16.5`, `22`, `28` |
//! | Rounds per box | `50/Box`, `Box of 20`, `1,000 Rounds`, `20/25` | `50`, `20`, `1000`, `20` |
//! | Cost per round | price `449.99` + `1000 Rounds` | `0.45` |
//!
//! # Functions
//!
//! * [`parse_title`] / [`parse_title_with`]: everything at once, as a [`ProductInfo`].
//! * [`identify_model`] / [`identify_model_with`]: brand, family and model ([`ModelMatch`]); [`list_brands`].
//! * [`find_calibers`], [`find_caliber`], [`normalize_caliber`], [`list_calibers`], [`caliber_by_id`].
//! * [`magazine_capacity`], [`barrel_length`], [`rounds_per_box`], [`cost_per_round`], [`looks_like_ammo`].
//!
//! The rules are compiled on first use (about 0.1 s); after that a title takes well under a millisecond. All
//! functions are thread-safe.
//!
//! # Cargo features
//!
//! * `serde`: `Serialize` for [`ProductInfo`], [`ModelMatch`], [`Caliber`] and the enums, with the same JSON field
//!   names as the Python and JavaScript packages.
//! * `cli`: the `gunsnprices` command line tool (`cargo install gunsnprices --features cli`).
//!
//! # Data license
//!
//! The code is MIT. The embedded model dictionaries and caliber list (`data/*.json`) are © GunsNPrices, licensed
//! under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/): credit "GunsNPrices (gunsnprices.com)" with a
//! link to <https://gunsnprices.com> if you redistribute them.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod calibers;
mod html;
mod json;
mod models;
mod parse;
mod pyre;
mod specs;
#[rustfmt::skip]
mod tables;

pub use calibers::{caliber_by_id, find_caliber, find_calibers, list_calibers, normalize_caliber, Caliber, CaliberKind};
pub use models::{identify_model, identify_model_with, list_brands, ModelMatch, Source};
pub use parse::{parse_title, parse_title_with, Kind, Options, ParseKindError, ProductInfo};
pub use specs::{barrel_length, cost_per_round, looks_like_ammo, magazine_capacity, rounds_per_box};

/// Version of this crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn engines() {
        let (plain, fancy) = crate::models::pattern_engines();
        eprintln!("dictionary patterns: {plain} on regex, {fancy} on fancy-regex");
        assert!(plain > fancy);
    }
}
