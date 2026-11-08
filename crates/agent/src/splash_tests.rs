use super::*;

#[test]
fn a_base_url_with_a_password_names_only_its_host() {
    assert_eq!(
        host_of(Some("https://alice:secret-pass@127.0.0.1:9/v1")),
        Some("127.0.0.1:9")
    );
    assert_eq!(
        host_of(Some("http://api.example.com/v1")),
        Some("api.example.com")
    );
}
