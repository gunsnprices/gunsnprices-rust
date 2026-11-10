//! Timing: `cargo run --release --example bench` parses the 1,150 golden titles a few times.

use std::time::{Duration, Instant};

fn per_title(titles: &[String], f: impl Fn(&str)) -> Duration {
    let t = Instant::now();
    for s in titles {
        f(s);
    }
    t.elapsed() / titles.len() as u32
}

fn main() {
    let text = std::fs::read_to_string("tests/golden.jsonl").expect("run from the crate directory");
    let titles: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let i = l.find("\"title\": \"")? + 10;
            let j = l[i..].find("\", ")?;
            Some(l[i..i + j].to_owned())
        })
        .collect();

    let t = Instant::now();
    let _ = gunsnprices::find_calibers("x");
    println!("caliber rules compiled in {:?}", t.elapsed());
    let t = Instant::now();
    let _ = gunsnprices::identify_model("x");
    println!("model dictionaries loaded in {:?}", t.elapsed());

    for round in 1..=3 {
        println!("round {round} ({} titles)", titles.len());
        println!(
            "  find_calibers:  {:?} per title",
            per_title(&titles, |s| drop(gunsnprices::find_calibers(s)))
        );
        println!(
            "  identify_model: {:?} per title",
            per_title(&titles, |s| drop(gunsnprices::identify_model(s)))
        );
        println!(
            "  parse_title:    {:?} per title",
            per_title(&titles, |s| drop(gunsnprices::parse_title(s)))
        );
    }
}
