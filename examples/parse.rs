use std::time::Instant;
fn main() {
    let text = std::fs::read_to_string("tests/golden.jsonl").unwrap();
    let titles: Vec<String> = text.lines().filter_map(|l| {
        let i = l.find("\"title\": \"")? + 10; let j = l[i..].find("\", ")? ; Some(l[i..i+j].to_string())
    }).collect();
    let t = Instant::now(); let _ = gunsnprices::find_calibers("x"); println!("calibers init {:?}", t.elapsed());
    let t = Instant::now(); let _ = gunsnprices::identify_model("x"); println!("models init {:?}", t.elapsed());
    let t = Instant::now(); let _ = gunsnprices::barrel_length("x"); let _ = gunsnprices::looks_like_ammo("x"); println!("specs init {:?}", t.elapsed());
    for round in 0..3 { println!("round {round}"); for (name, f) in [
        ("find_calibers", (|s: &str| { let _ = gunsnprices::find_calibers(s); }) as fn(&str)),
        ("identify_model", |s: &str| { let _ = gunsnprices::identify_model(s); }),
        ("specs", |s: &str| { let _ = (gunsnprices::barrel_length(s), gunsnprices::magazine_capacity(s), gunsnprices::rounds_per_box(s), gunsnprices::looks_like_ammo(s)); }),
        ("parse_title", |s: &str| { let _ = gunsnprices::parse_title(s); }),
    ] {
        let t = Instant::now();
        for s in &titles { f(s); }
        println!("{name}: {:?} per title ({} titles)", t.elapsed() / titles.len() as u32, titles.len());
    } }
}
