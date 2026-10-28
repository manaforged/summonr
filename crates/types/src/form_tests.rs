use serde_json::json;

use super::*;

fn spec_form() -> Form {
    let params = json!({
        "mode": "form",
        "message": "Tell us about you",
        "requestedSchema": {
            "type": "object",
            "properties": {
                "email": {"type": "string", "format": "email", "title": "Email"},
                "born": {"type": "string", "format": "date"},
                "age": {"type": "integer", "minimum": 18},
                "agree": {"type": "boolean", "default": false},
                "color": {"type": "string", "oneOf": [
                    {"const": "#FF0000", "title": "Red"},
                    {"const": "#00FF00", "title": "Green"}
                ]},
                "tags": {"type": "array", "maxItems": 2, "items": {"anyOf": [
                    {"const": "a", "title": "Alpha"},
                    {"const": "b", "title": "Beta"},
                    {"const": "c", "title": "Gamma"}
                ]}}
            },
            "required": ["email"]
        }
    });
    Form::parse("fixture", &params).unwrap()
}

fn field<'a>(form: &'a Form, name: &str) -> &'a Field {
    form.fields.iter().find(|field| field.name == name).unwrap()
}

#[test]
fn the_spec_schema_types_parse_and_read_typed_answers() {
    let form = spec_form();
    assert_eq!(form.fields.len(), 6);
    assert_eq!(
        field(&form, "email").read("a@b.io"),
        Ok(Some(json!("a@b.io")))
    );
    drop(field(&form, "email").read("nobody").unwrap_err());
    drop(field(&form, "email").read("").unwrap_err());
    assert_eq!(
        field(&form, "born").read("2024-02-29"),
        Ok(Some(json!("2024-02-29")))
    );
    drop(field(&form, "born").read("2023-02-29").unwrap_err());
    drop(field(&form, "age").read("17").unwrap_err());
    drop(field(&form, "age").read("18.5").unwrap_err());
    assert_eq!(field(&form, "agree").read(""), Ok(Some(json!(false))));
    assert_eq!(field(&form, "agree").read("yes"), Ok(Some(json!(true))));
    assert_eq!(
        field(&form, "color").read("green"),
        Ok(Some(json!("#00FF00")))
    );
    assert_eq!(field(&form, "color").read("1"), Ok(Some(json!("#FF0000"))));
    assert_eq!(
        field(&form, "tags").read("Alpha, 3"),
        Ok(Some(json!(["a", "c"])))
    );
    drop(field(&form, "tags").read("a,b,c").unwrap_err());
}

#[test]
fn a_url_or_nested_request_is_not_a_form() {
    let url = json!({"mode": "url", "url": "https://x.io", "message": "go"});
    assert!(Form::parse("fixture", &url).is_none());
    let nested = json!({"message": "m", "requestedSchema": {"type": "object", "properties": {
        "inner": {"type": "object"}
    }}});
    assert!(Form::parse("fixture", &nested).is_none());
}

#[test]
fn an_answer_is_checked_against_the_whole_form() {
    let form = spec_form();
    let content = |value: serde_json::Value| value.as_object().unwrap().clone();
    form.check(&content(json!({"email": "a@b.io", "age": 30})))
        .unwrap();
    drop(form.check(&content(json!({"age": 30}))).unwrap_err());
    drop(
        form.check(&content(json!({"email": "a@b.io", "extra": 1})))
            .unwrap_err(),
    );
    drop(
        form.check(&content(json!({"email": "a@b.io", "color": "#0000FF"})))
            .unwrap_err(),
    );
}
