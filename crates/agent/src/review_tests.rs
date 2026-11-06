use super::*;

#[test]
fn a_passing_verdict_passes_whatever_shape_its_findings_take() {
    for reply in [
        r#"{"pass": true}"#,
        r#"{"pass": true, "findings": null}"#,
        r#"{"pass": true, "findings": []}"#,
    ] {
        let verdict = parse(reply);
        assert!(verdict.pass, "{reply} failed the review");
        assert!(verdict.findings.is_empty(), "{reply}: {}", verdict.findings);
    }
}
