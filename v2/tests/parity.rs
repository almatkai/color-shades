use crate::Generator;
use serde_json::Value;
#[test]
fn executable_original_typescript_library_oracle() {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/oracle.json")).unwrap();
    let mut generator = Generator::default();
    for record in fixture["records"].as_array().unwrap() {
        assert_eq!(
            serde_json::to_value(&generator).unwrap(),
            record["before"],
            "state before {:?}",
            record["input"]
        );
        let result = generator.generate(
            record["input"].as_str().unwrap(),
            fixture["date"].as_str().unwrap(),
        );
        if let Some(error) = record["error"].as_str() {
            assert_eq!(result.unwrap_err(), error, "input {:?}", record["input"]);
        } else {
            let p = result.unwrap();
            assert_eq!(
                serde_json::to_value(&p).unwrap(),
                record["palette"],
                "input {:?}",
                record["input"]
            );
            assert_eq!(
                p.json().unwrap(),
                record["json"],
                "JSON {:?}",
                record["input"]
            );
            assert_eq!(
                p.variables().unwrap(),
                record["variables"],
                "variables {:?}",
                record["input"]
            );
        }
    }
}
#[test]
fn ties_retain_previous_stable_sort_order() {
    let mut left = Generator::default();
    left.generate("00ff00", "d").unwrap();
    let a = left.generate("ffff00", "d").unwrap();
    let mut right = Generator::default();
    right.generate("ff0000", "d").unwrap();
    let b = right.generate("ffff00", "d").unwrap();
    assert_ne!(a.colors[&600], b.colors[&600]);
}
