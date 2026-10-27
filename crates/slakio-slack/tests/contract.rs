//! The contract test against a real workspace: run by its owner only, never in CI. It reports
//! shape differences (a field present, missing or of another type than the fake's), never a
//! value. Skipped (listed as ignored) unless asked for with `SLAKIO_TEST_REAL=1` and
//! `cargo test -p slakio-slack --test contract -- --ignored`.
//!
//! This version cannot reach Slack (the transport serves the loopback address only), so the
//! real run stops with a clear message; the shape comparison it will use is tested here.

use serde_json::Value;
use std::collections::BTreeMap;

/// Every path of `v` with the type found there (`a.b[]`: the entries of a list).
fn shape(v: &Value, path: &str, out: &mut BTreeMap<String, &'static str>) {
    let kind = match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(items) => {
            for item in items {
                shape(item, &format!("{path}[]"), out);
            }
            "list"
        }
        Value::Object(map) => {
            for (k, item) in map {
                shape(item, &if path.is_empty() { k.clone() } else { format!("{path}.{k}") }, out);
            }
            "object"
        }
    };
    out.insert(path.to_string(), kind);
}

/// The paths whose type differs between `real` and `fake`, or that only one has.
fn shape_diff(real: &Value, fake: &Value) -> Vec<String> {
    let (mut r, mut f) = (BTreeMap::new(), BTreeMap::new());
    shape(real, "", &mut r);
    shape(fake, "", &mut f);
    let mut out = Vec::new();
    for (path, kind) in &r {
        match f.get(path) {
            None => out.push(format!("only real: {path} ({kind})")),
            Some(k) if k != kind => out.push(format!("type: {path} real {kind}, fake {k}")),
            Some(_) => {}
        }
    }
    out.extend(f.keys().filter(|p| !r.contains_key(*p)).map(|p| format!("only fake: {p}")));
    out
}

#[test]
fn shapes_are_compared_without_values() {
    let real = serde_json::json!({"ok": true, "team": {"id": "T9", "name": "secret"}, "list": [{"a": 1}]});
    let fake = serde_json::json!({"ok": true, "team": {"id": 3}, "list": [{"b": "x"}]});
    let diff = shape_diff(&real, &fake);
    assert_eq!(
        diff,
        [
            "only real: list[].a (number)",
            "type: team.id real string, fake number",
            "only real: team.name (string)",
            "only fake: list[].b"
        ]
    );
    assert!(diff.iter().all(|d| !d.contains("T9") && !d.contains("secret")), "never a value");
}

#[test]
#[ignore = "owner-run only: SLAKIO_TEST_REAL=1, never in CI"]
fn real_workspace_shapes_match_the_fake() {
    if std::env::var("SLAKIO_TEST_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set SLAKIO_TEST_REAL=1 to run the contract test against a real workspace");
        return;
    }
    panic!("this version cannot reach Slack: the contract test runs once sign-in exists");
}
