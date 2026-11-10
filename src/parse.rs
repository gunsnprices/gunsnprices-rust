//! One call for everything: parse a firearm or ammunition product title.

use crate::calibers::{find_calibers, Caliber};
use crate::models::{identify_model_with, ModelMatch};
use crate::specs::{barrel_length, cost_per_round, looks_like_ammo, magazine_capacity, rounds_per_box};
use std::fmt;
use std::str::FromStr;

/// Firearm or ammunition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Kind {
    /// A gun (pistol, rifle, shotgun, ...).
    Firearm,
    /// Ammunition.
    Ammo,
}

impl Kind {
    /// `"firearm"` or `"ammo"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Firearm => "firearm",
            Kind::Ammo => "ammo",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing a [`Kind`] from a string other than `"firearm"` or `"ammo"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseKindError(String);

impl fmt::Display for ParseKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown kind {:?} (expected \"firearm\" or \"ammo\")", self.0)
    }
}

impl std::error::Error for ParseKindError {}

impl FromStr for Kind {
    type Err = ParseKindError;

    fn from_str(s: &str) -> Result<Kind, ParseKindError> {
        match s {
            "firearm" => Ok(Kind::Firearm),
            "ammo" => Ok(Kind::Ammo),
            _ => Err(ParseKindError(s.to_owned())),
        }
    }
}

/// What you know about a listing besides its title (all optional), for [`parse_title_with`] and
/// [`identify_model_with`](crate::identify_model_with).
///
/// ```
/// use gunsnprices::{Kind, Options};
///
/// let opts = Options::new().brand("Glock").mpn("PA195S203").price(549.0).kind(Kind::Firearm);
/// assert_eq!(opts.brand, Some("Glock"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct Options<'a> {
    /// Manufacturer, if known (helps with titles that don't name the brand).
    pub brand: Option<&'a str>,
    /// Manufacturer part number, if known (Glock part numbers encode the model and generation).
    pub mpn: Option<&'a str>,
    /// Store SKU, if known.
    pub sku: Option<&'a str>,
    /// Price of the listing; with ammo it gives the cost per round.
    pub price: Option<f64>,
    /// Firearm or ammo; guessed from the title when not given.
    pub kind: Option<Kind>,
}

impl<'a> Options<'a> {
    /// No hints: the same as [`Options::default`].
    pub fn new() -> Options<'a> {
        Options::default()
    }

    /// Set the manufacturer.
    pub fn brand(mut self, brand: &'a str) -> Options<'a> {
        self.brand = Some(brand);
        self
    }

    /// Set the manufacturer part number.
    pub fn mpn(mut self, mpn: &'a str) -> Options<'a> {
        self.mpn = Some(mpn);
        self
    }

    /// Set the store SKU.
    pub fn sku(mut self, sku: &'a str) -> Options<'a> {
        self.sku = Some(sku);
        self
    }

    /// Set the listing price.
    pub fn price(mut self, price: f64) -> Options<'a> {
        self.price = Some(price);
        self
    }

    /// Skip the firearm/ammo guess.
    pub fn kind(mut self, kind: Kind) -> Options<'a> {
        self.kind = Some(kind);
        self
    }

    /// Empty strings and a zero price count as "not given", like falsy values in the reference implementation.
    pub(crate) fn normalized(self) -> Options<'a> {
        Options {
            brand: self.brand.filter(|s| !s.is_empty()),
            mpn: self.mpn.filter(|s| !s.is_empty()),
            sku: self.sku.filter(|s| !s.is_empty()),
            price: self.price.filter(|&p| p != 0.0),
            kind: self.kind,
        }
    }
}

/// Result of [`parse_title`]. Fields that don't apply or weren't found are `None` (or empty).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[non_exhaustive]
pub struct ProductInfo {
    /// The title as given.
    pub title: String,
    /// Firearm or ammo.
    pub kind: Kind,
    /// Brand of the identified model, else the brand passed in [`Options`], else `""`.
    pub brand: String,
    /// Firearms only.
    pub model: Option<ModelMatch>,
    /// First caliber in the title.
    pub caliber: Option<Caliber>,
    /// Every caliber in the title (".357 Mag / .38 Special"), in order.
    pub calibers: Vec<Caliber>,
    /// Magazine capacity, firearms only (for "15+1" it is 15).
    pub capacity: Option<u32>,
    /// Barrel length in inches, firearms only.
    pub barrel_length: Option<f64>,
    /// Rounds per box, ammo only.
    pub rounds: Option<u32>,
    /// Price per round, ammo only; needs a price.
    pub cost_per_round: Option<f64>,
}

/// Parse a product title into brand, model, caliber, capacity, barrel length and rounds per box.
///
/// The kind (firearm or ammo) is guessed from the title. Use [`parse_title_with`] to pass the brand, part number,
/// price or kind.
///
/// ```
/// let info = gunsnprices::parse_title("Glock 19 Gen5 MOS 9mm 4.02in 15+1 Black");
/// assert_eq!(info.model.unwrap().model, "Glock 19 Gen 5 MOS");
/// assert_eq!(info.caliber.unwrap().name, "9×19mm Parabellum");
/// assert_eq!(info.capacity, Some(15));
/// assert_eq!(info.barrel_length, Some(4.02));
/// ```
pub fn parse_title(title: &str) -> ProductInfo {
    parse_title_with(title, Options::new())
}

/// [`parse_title`] with what you know about the listing: brand, part number, SKU, price, kind.
///
/// ```
/// use gunsnprices::{parse_title_with, Kind, Options};
///
/// let info = parse_title_with("Federal American Eagle 9mm Luger 115gr FMJ, 50/Box", Options::new().price(18.99));
/// assert_eq!(info.kind, Kind::Ammo);
/// assert_eq!(info.rounds, Some(50));
/// assert_eq!(info.cost_per_round, Some(0.3798));
/// ```
pub fn parse_title_with(title: &str, opts: Options<'_>) -> ProductInfo {
    let opts = opts.normalized();
    let kind = opts
        .kind
        .unwrap_or_else(|| if looks_like_ammo(title) { Kind::Ammo } else { Kind::Firearm });
    let calibers = find_calibers(title);
    let mut info = ProductInfo {
        title: title.to_owned(),
        kind,
        brand: opts.brand.unwrap_or("").to_owned(),
        model: None,
        caliber: calibers.first().copied(),
        calibers,
        capacity: None,
        barrel_length: None,
        rounds: None,
        cost_per_round: None,
    };
    match kind {
        Kind::Ammo => {
            info.rounds = rounds_per_box(title);
            info.cost_per_round = match (opts.price, info.rounds) {
                (Some(price), Some(rounds)) => cost_per_round(price, rounds),
                _ => None,
            };
        }
        Kind::Firearm => {
            info.model = identify_model_with(title, opts);
            if let Some(m) = &info.model {
                info.brand = m.brand.clone();
            }
            info.capacity = magazine_capacity(title);
            info.barrel_length = barrel_length(title);
        }
    }
    info
}
