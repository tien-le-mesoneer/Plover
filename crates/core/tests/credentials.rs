//! Credential spans in body text. Fixtures are synthetic.

use plover_core::credentials::{CredentialSpan, SpanKind, body_credential_spans, jwt_spans};

const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig";

/// `(text of the span, name, kind)` for each span.
fn spans(text: &str) -> Vec<(&str, Option<String>, SpanKind)> {
    body_credential_spans(text)
        .into_iter()
        .map(|CredentialSpan { range, name, kind }| (&text[range], name, kind))
        .collect()
}

#[test]
fn json_string_and_number_spans() {
    let text = r#"{"user":"a","password":"p@ss","pin":1234,"n":{"token":"t"},"newPassword":"x"}"#;
    let found = spans(text);
    assert_eq!(
        found,
        vec![
            ("p@ss", Some("password".into()), SpanKind::JsonString),
            ("t", Some("token".into()), SpanKind::JsonString),
        ]
    );
    // `pin` is not a credential field; a numeric credential is found by its name.
    let numeric = spans(r#"{"Password":1234}"#);
    assert_eq!(
        numeric,
        vec![("1234", Some("Password".into()), SpanKind::JsonNumber)]
    );
}

#[test]
fn json_string_with_escapes() {
    let text = r#"{"password":"a\"b"}"#;
    assert_eq!(
        spans(text),
        vec![(r#"a\"b"#, Some("password".into()), SpanKind::JsonString)]
    );
}

#[test]
fn json_empty_value_gives_no_span() {
    assert!(spans(r#"{"password":""}"#).is_empty());
}

#[test]
fn invalid_json_gives_only_jwt_spans() {
    let text = format!(r#"{{"password": oops {JWT}"#);
    assert_eq!(spans(&text), vec![(JWT, None, SpanKind::Jwt)]);
}

#[test]
fn xml_element_and_attribute() {
    let text = r#"<a><wsse:Password Type="x">s3cret</wsse:Password><cred password='pw1'/></a>"#;
    assert_eq!(
        spans(text),
        vec![
            ("s3cret", Some("Password".into()), SpanKind::XmlText),
            ("pw1", Some("password".into()), SpanKind::XmlAttr),
        ]
    );
    assert!(spans("<passwords>x</passwords>").is_empty());
    assert!(spans("<password></password>").is_empty());
    assert!(spans("<password>x</other>").is_empty());
}

#[test]
fn xml_cdata_and_entities_round_trip() {
    assert_eq!(
        spans("<password><![CDATA[a<b]]></password>"),
        vec![("a<b", Some("password".into()), SpanKind::XmlText)]
    );
    assert_eq!(
        spans("<password>a&amp;b</password>"),
        vec![("a&amp;b", Some("password".into()), SpanKind::XmlText)]
    );
}

#[test]
fn xml_with_declaration_and_leading_space() {
    let text = "\n  <?xml version=\"1.0\"?><Envelope><token>tok-AAAA</token></Envelope>";
    assert_eq!(
        spans(text),
        vec![("tok-AAAA", Some("token".into()), SpanKind::XmlText)]
    );
}

#[test]
fn jwt_inside_text_and_json_value() {
    let text = format!("token={JWT} rest");
    assert_eq!(spans(&text), vec![(JWT, None, SpanKind::Jwt)]);
    let json = format!(r#"{{"note":"{JWT}"}}"#);
    assert_eq!(spans(&json), vec![(JWT, None, SpanKind::Jwt)]);
}

#[test]
fn jwt_inside_named_span_not_duplicated() {
    let json = format!(r#"{{"token":"{JWT}"}}"#);
    assert_eq!(
        spans(&json),
        vec![(JWT, Some("token".into()), SpanKind::JsonString)]
    );
}

#[test]
fn jwt_spans_need_two_parts_and_a_boundary() {
    let text = format!("a {JWT}. xeyJhbGciOiJIUzI1NiJ9.eyJ eyJhbGciOiJIUzI1NiJ9 end");
    let found: Vec<&str> = jwt_spans(&text).into_iter().map(|r| &text[r]).collect();
    assert_eq!(found, vec![JWT]);
    // Unsigned token: the signature part may be empty.
    let unsigned = "eyJhbGciOiJub25lIn0.eyJzdWIiOiIxIn0.";
    let found: Vec<&str> = jwt_spans(unsigned)
        .into_iter()
        .map(|r| &unsigned[r])
        .collect();
    assert_eq!(found, vec![unsigned]);
}
