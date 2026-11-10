//! Regression test: `parse_title_with` on 1,150 real store titles must give the recorded answers.
//!
//! `golden.jsonl` is the same file the Python package (tests/golden.jsonl) and the JavaScript package
//! (test/golden.jsonl) use, so all implementations stay identical.

use gunsnprices::{parse_title_with, Kind, Options, ProductInfo};
use serde_json::{json, Value};

fn compact(info: &ProductInfo) -> Value {
    let model = info.model.as_ref().map(|m| {
        json!([
            m.family,
            m.model,
            m.generation,
            m.configs,
            m.features,
            m.source.as_str()
        ])
    });
    json!({
        "kind": info.kind.as_str(),
        "brand": info.brand,
        "model": model,
        "calibers": info.calibers.iter().map(|c| c.id).collect::<Vec<_>>(),
        "capacity": info.capacity,
        "barrel_length": info.barrel_length,
        "rounds": info.rounds,
        "cost_per_round": info.cost_per_round,
    })
}

/// JSON equality where numbers compare by value (Python: `26 == 26.0`).
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

#[test]
fn golden() {
    let text = include_str!("golden.jsonl");
    let mut total = 0;
    let mut failures = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let case: Value = serde_json::from_str(line).expect("valid JSON line");
        let args = &case["args"];
        let title = args["title"].as_str().expect("title");
        let mut opts = Options::new();
        if let Some(b) = args.get("brand").and_then(Value::as_str) {
            opts = opts.brand(b);
        }
        if let Some(m) = args.get("mpn").and_then(Value::as_str) {
            opts = opts.mpn(m);
        }
        if let Some(s) = args.get("sku").and_then(Value::as_str) {
            opts = opts.sku(s);
        }
        if let Some(p) = args.get("price").and_then(Value::as_f64) {
            opts = opts.price(p);
        }
        if let Some(k) = args.get("kind").and_then(Value::as_str) {
            opts = opts.kind(k.parse::<Kind>().expect("kind"));
        }
        let got = compact(&parse_title_with(title, opts));
        total += 1;
        if !same(&got, &case["expected"]) {
            failures.push(format!(
                "{title}\n    expected {}\n    got      {got}",
                case["expected"]
            ));
        }
    }
    assert_eq!(total, 1150, "golden.jsonl should have 1,150 cases");
    eprintln!(
        "golden: {}/{} titles parse exactly like the Python package",
        total - failures.len(),
        total
    );
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
