use serde_json::json;

use super::has_error;

#[test]
fn an_error_after_a_warning_still_counts_as_an_error() {
    let report = json!({"items": [
        {"severity": 2, "message": "unused variable"},
        {"severity": 1, "message": "mismatched types"},
    ]});
    assert!(has_error(&report));
    assert!(!has_error(
        &json!({"items": [{"severity": 2, "message": "unused"}]})
    ));
}

#[tokio::test]
async fn a_frame_larger_than_the_cap_is_refused_before_it_is_read() {
    let header = "Content-Length: 67108864\r\n\r\n";
    let mut reader = tokio::io::BufReader::new(header.as_bytes());
    let error = super::read_msg(&mut reader).await.unwrap_err().to_string();
    assert!(error.contains("larger than"), "{error}");
}
