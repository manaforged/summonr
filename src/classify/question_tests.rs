use super::*;

use crate::classify::tests::endpoint;
use crate::endpoint::Wire;

#[test]
fn a_noul_can_carry_the_boundary_the_api_documents() {
    let question = Question::bounded("is it transient", "retrying works", "retrying repeats");
    let body = serde_json::to_value(&question).unwrap();
    assert_eq!(body["type"], "noul");
    assert_eq!(body["criteria"]["true"], "retrying works");
    assert_eq!(body["criteria"]["false"], "retrying repeats");
}

#[test]
fn a_plain_noul_sends_no_criteria_key_at_all() {
    let body = serde_json::to_value(Question::noul("is it broken")).unwrap();
    assert!(
        body.get("criteria").is_none(),
        "an absent boundary must not be sent as null: {body}"
    );
}

#[test]
fn an_option_may_be_described_with_structure_not_only_a_string() {
    let question = Question::choice(
        "classify",
        [(
            "antibot",
            serde_json::json!({
                "what": "a challenge page",
                "not_for": "a named auth error",
                "examples": ["Access Denied", "Just a moment"],
            }),
        )],
    );
    let body = serde_json::to_value(&question).unwrap();
    assert!(
        body["criteria"]["antibot"].is_object(),
        "structure must survive to the wire: {body}"
    );
    assert_eq!(body["criteria"]["antibot"]["not_for"], "a named auth error");
}

#[test]
fn a_score_outside_the_documented_range_is_refused_before_sending() {
    let one = Question::score("how bad", ["only"]);
    let error = one.check("risk").unwrap_err().to_string();
    assert!(error.contains("2 to 10"), "got {error}");
    let many: Vec<String> = (0..11).map(|n| format!("level {n}")).collect();
    assert!(Question::score("how bad", many).check("risk").is_err());
    assert!(
        Question::score("how bad", ["a", "b"]).check("risk").is_ok(),
        "two levels is the documented minimum"
    );
}

#[test]
fn a_choice_beyond_the_documented_cardinality_is_refused() {
    let wide: Vec<(String, &str)> = (0..=MOST_OPTIONS)
        .map(|n| (format!("option{n}"), "x"))
        .collect();
    let error = Question::choice("pick", wide)
        .check("kind")
        .unwrap_err()
        .to_string();
    assert!(error.contains("255"), "got {error}");
    let empty: Vec<(String, &str)> = Vec::new();
    assert!(Question::choice("pick", empty).check("kind").is_err());
}

#[test]
fn state_may_be_an_object_because_the_api_takes_one() {
    let questions = BTreeMap::from([("q".to_string(), Question::noul("is it urgent"))]);
    let body = crate::provider::decisions::body(
        "m",
        &serde_json::json!({"message": "charged twice", "order_id": "A-104"}),
        &questions,
    );
    assert_eq!(body["state"]["order_id"], "A-104");
}

#[test]
fn a_request_carries_the_headers_the_endpoint_declares() {
    let mut seat = endpoint("vendor", Wire::Decisions);
    seat.headers
        .insert("X-Vendor-Project".to_string(), "website".to_string());
    assert_eq!(
        seat.headers.get("X-Vendor-Project").map(String::as_str),
        Some("website"),
        "a provider that needs a header must be configurable without code"
    );
}

#[test]
fn the_fan_out_is_bounded_and_keeps_the_order_it_was_given() {
    assert_ne!(AT_ONCE, 0, "a zero width would never poll anything");
    let work: Vec<_> = (0..AT_ONCE * 5)
        .map(|index| async move { Ok::<usize, Error>(index) })
        .collect();
    let done = futures::executor::block_on(in_flight(work)).unwrap();
    assert_eq!(done.len(), AT_ONCE * 5);
    assert_eq!(
        done,
        (0..AT_ONCE * 5).collect::<Vec<_>>(),
        "answers must line up with the states they came from"
    );
}

#[test]
fn a_failure_anywhere_in_the_fan_out_surfaces() {
    let work: Vec<_> = (0..5usize)
        .map(|index| async move {
            if index == 3 {
                Err(Error::Config("the third one failed".to_string()))
            } else {
                Ok(index)
            }
        })
        .collect();
    let error = futures::executor::block_on(in_flight(work)).unwrap_err();
    assert!(error.to_string().contains("third one failed"));
}
