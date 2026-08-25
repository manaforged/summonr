use super::fixture;
use crate::durable::bound::{Bytes, Items, Text};
use crate::durable::context::{ImagePayload, InputOrigin, InputRoute, UserPayload};
use crate::durable::encoding::{Commit, EncodingError, FACT_LIMIT};
use crate::durable::fact::{Fact, FactData};
use crate::durable::workspace::PathKey;

#[test]
fn bounded_values_reject_oversized_deserialization() {
    let text = format!("\"{}\"", "x".repeat(5));
    serde_json::from_str::<Text<4>>(&text).expect_err("oversized text must fail");
    serde_json::from_str::<Items<u8, 1>>("[1,2]").expect_err("oversized list must fail");
    serde_json::from_str::<Bytes<1>>("\"YWI=\"").expect_err("oversized bytes must fail");

    let invalid_path = r#"{"platform":"unix","components":[{"units":"unix","value":"Li4="}]}"#;
    serde_json::from_str::<PathKey>(invalid_path).expect_err("invalid path must fail");
}

#[test]
fn measured_input_ceiling_accepts_image_limit_and_preserves_metadata_reserve() {
    let image = vec![0x5a; 10 * 1_048_576];
    let legal = input_fact(image.clone(), "x".repeat(512 * 1_024));
    Commit::new(
        2,
        fixture::id(2),
        Some(fixture::digest("head")),
        None,
        vec![legal],
    )
    .expect("10 MiB image plus 512 KiB text must fit format 2");

    let oversized = input_fact(image, "x".repeat(800 * 1_024));
    let error = Commit::new(
        2,
        fixture::id(2),
        Some(fixture::digest("head")),
        None,
        vec![oversized],
    )
    .expect_err("payload must retain at least 2 MiB of line headroom");
    assert!(matches!(error, EncodingError::Payload { .. }));
}

#[test]
fn commit_rejects_unbounded_fact_count_before_encoding() {
    let fact = Fact {
        id: fixture::fact_id(1),
        data: FactData::CommandRejected {
            code: fixture::text("busy"),
            diagnostic: fixture::text("busy"),
        },
    };
    let error = Commit::new(
        2,
        fixture::id(2),
        Some(fixture::digest("head")),
        None,
        vec![fact; FACT_LIMIT + 1],
    )
    .expect_err("fact count must be bounded");
    assert!(matches!(error, EncodingError::Facts { .. }));
}

fn input_fact(image: Vec<u8>, text: String) -> Fact {
    Fact {
        id: fixture::fact_id(3),
        data: FactData::InputAccepted {
            input: fixture::id(30),
            origin: InputOrigin::User,
            route: InputRoute::Queue,
            payload: UserPayload {
                text: Text::new(text).expect("fixture text stays below its field bound"),
                images: Items::new(vec![ImagePayload {
                    media_type: fixture::text("image/png"),
                    data: Bytes::new(image).expect("fixture image stays at its field bound"),
                }])
                .expect("fixture image count must fit"),
            },
        },
    }
}
