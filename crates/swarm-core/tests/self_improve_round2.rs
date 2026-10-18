use serde_json::json;
use swarm_core::{Budget, DispatcherError, Needs, TaskContract, Verdict};

fn contract(kind: &str, checks: Vec<serde_json::Value>) -> TaskContract {
    TaskContract {
        kind: kind.to_string(),
        description: None,
        goal: None,
        checks,
        needs: Needs::default(),
        budget: Budget::default(),
    }
}

#[test]
fn validate_rejects_empty_checks_and_kind() {
    let ok = contract("root", vec![json!({"type": "manual", "run": "cargo test"})]);
    assert!(ok.validate().is_ok());

    let no_checks = contract("root", vec![]);
    assert!(matches!(
        no_checks.validate(),
        Err(DispatcherError::ContractInvalid)
    ));

    let blank_kind = contract("   ", vec![json!({"type": "cmd"})]);
    assert!(matches!(
        blank_kind.validate(),
        Err(DispatcherError::ContractInvalid)
    ));

    let empty_kind = contract("", vec![json!({"type": "cmd"})]);
    assert!(matches!(
        empty_kind.validate(),
        Err(DispatcherError::ContractInvalid)
    ));
}

#[test]
fn contract_json_roundtrip_keeps_checks_array() {
    let original = contract(
        "code.feature",
        vec![
            json!({"type": "cmd", "run": "true"}),
            json!({"type": "manual"}),
        ],
    );
    let text = serde_json::to_string(&original).expect("serialize");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse to value");
    assert!(
        value.get("checks").is_some_and(|v| v.is_array()),
        "checks must serialize as a JSON array: {text}"
    );
    assert_eq!(
        value["checks"].as_array().map(Vec::len),
        Some(2),
        "checks array length must survive serialization: {text}"
    );
    let back: TaskContract = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back.kind, original.kind);
    assert_eq!(back.checks, original.checks);
    assert!(back.validate().is_ok());
}

#[test]
fn verdict_as_str_matches_persisted_strings() {
    assert_eq!(Verdict::Approved.as_str(), "approved");
    assert_eq!(Verdict::ChangesRequested.as_str(), "changes_requested");
    assert_eq!(Verdict::Rejected.as_str(), "rejected");
    for (v, s) in [
        (Verdict::Approved, "approved"),
        (Verdict::ChangesRequested, "changes_requested"),
        (Verdict::Rejected, "rejected"),
    ] {
        assert_eq!(v.as_str(), s);
        assert!(!s.contains(' ') && s == s.to_lowercase());
    }
}
