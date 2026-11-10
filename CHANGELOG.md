# Changelog

## 0.1.0 (2026-11-10)

First release. Rust port of the [gunsnprices](https://pypi.org/project/gunsnprices/) Python package 0.1.2, with
identical results on the 1,150 recorded store titles of its test suite and on all 63,755 titles of the GunsNPrices
catalog.

- `parse_title()` / `parse_title_with()`: firearm or ammo, brand, model, calibers, magazine capacity, barrel length,
  rounds per box, cost per round.
- `identify_model()` / `identify_model_with()`: 50 brands, the same model dictionaries that
  [GunsNPrices](https://gunsnprices.com) uses on its site; decodes Glock part numbers. `list_brands()`.
- `find_calibers()` / `find_caliber()` / `normalize_caliber()` / `list_calibers()` / `caliber_by_id()`: 293 calibers
  with retailer spellings.
- `magazine_capacity()`, `barrel_length()`, `rounds_per_box()`, `cost_per_round()`, `looks_like_ammo()`.
- Optional `serde` feature: `Serialize` with the same JSON field names as the Python and JavaScript packages.
- Optional `cli` feature: the `gunsnprices` command (`cargo install gunsnprices --features cli`).
- Safe Rust, thread-safe, data embedded; the Python-syntax rules run on `regex`, and on `fancy-regex` only where
  they need look-around. Minimum supported Rust version 1.71.
