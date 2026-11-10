//! `cargo run --example parse`: parse a few store titles and print what was found.

use gunsnprices::{normalize_caliber, parse_title, parse_title_with, Options};

fn main() {
    for title in [
        "Glock 19 Gen 5 MOS 9mm 4.02in 15rd Pistol PA195S203MOS",
        "Ruger GP100 .357 Mag / .38 Special 4.2in 6-shot Revolver",
        "Beretta 686 Silver Pigeon I O/U 12/28 BL/WD 3\"",
    ] {
        let info = parse_title(title);
        println!("{title}");
        println!("  kind:     {}", info.kind);
        println!("  model:    {}", info.model.as_ref().map_or("-", |m| m.model.as_str()));
        let calibers: Vec<&str> = info.calibers.iter().map(|c| c.name).collect();
        println!("  calibers: {}", calibers.join(", "));
        println!("  capacity: {:?}, barrel: {:?} in", info.capacity, info.barrel_length);
    }

    let title = "Federal American Eagle 9mm Luger 115gr FMJ, 50/Box";
    let info = parse_title_with(title, Options::new().price(18.99));
    println!("{title}");
    println!(
        "  kind: {}, rounds: {:?}, cost per round: {:?}",
        info.kind, info.rounds, info.cost_per_round
    );

    if let Some(c) = normalize_caliber("308 WIN (7.62X51 NATO)") {
        println!("\"308 WIN (7.62X51 NATO)\" -> {} ({}, {})", c.name, c.id, c.kind);
    }
}
