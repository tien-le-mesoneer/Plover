//! HAR preview and import planning. Fixtures are synthetic (`example.test`, fake tokens).

use std::collections::{BTreeMap, BTreeSet};

use plover_core::bru::model::RequestFile;
use plover_core::bru::syntax::BruDocument;
use plover_core::har::{
    ExistingSecrets, HarRow, HiddenReason, ImportPlan, parse_har, plan_import, preview,
};
use plover_core::request::{Auth, Body, HttpRequest, KeyValue, Method, MultipartValue};
use plover_core::store::sanitize_name;

const CHROME: &str = include_str!("fixtures/har/chrome.har");
const SECRETS: &str = include_str!("fixtures/har/secrets.har");
const FIREFOX: &str = include_str!("fixtures/har/firefox-minimal.har");
const SOAP: &str = include_str!("fixtures/har/xml-soap.har");
/// Every secret value in `xml-soap.har`, as written in the HAR text.
const SOAP_VALUES: &[&str] = &[
    "pw-AAAA",
    "pw-BBBB",
    "cd<ATA-1",
    "at&amp;tr-2",
    "en&amp;t-3",
    "eyJhbGciOiJIUzI1NiJ9",
];

/// Every secret value in `secrets.har`.
const SECRET_VALUES: &[&str] = &[
    "pw-SECRET-1",
    "cs-SECRET-2",
    "basic-SECRET-3",
    "cookie-SECRET-4",
    "key-SECRET-5",
    "csrf-SECRET-6",
    "cs-SECRET-7",
    "digest-SECRET-8",
    "pw-SECRET-9",
    "tok-AAAA1111",
    "tok-BBBB2222",
    "tok-QUERY-0001",
    "eyJhbGciOiJIUzI1NiJ9",
];

fn rows(text: &str) -> Vec<HarRow> {
    preview(&parse_har(text).unwrap())
}

fn plan_all(text: &str, secrets: &ExistingSecrets) -> ImportPlan {
    let har = parse_har(text).unwrap();
    let all: Vec<usize> = (0..har.log.entries.len()).collect();
    plan_import(&har, &all, &BTreeSet::new(), 0.0, secrets)
}

fn no_secrets() -> ExistingSecrets {
    ExistingSecrets::default()
}

fn find<'a>(plan: &'a ImportPlan, url_part: &str) -> &'a HttpRequest {
    &plan
        .requests
        .iter()
        .find(|(_, r)| r.url.contains(url_part))
        .unwrap_or_else(|| panic!("no request for {url_part}"))
        .1
}

fn header<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

#[test]
fn preview_hides_assets_by_default() {
    let rows = rows(CHROME);
    let hidden: Vec<(String, Option<HiddenReason>)> =
        rows.iter().map(|r| (r.url.clone(), r.hidden)).collect();
    let reason = |url: &str| {
        hidden
            .iter()
            .find(|(u, _)| u == url)
            .unwrap_or_else(|| panic!("{url} missing"))
            .1
    };
    assert_eq!(reason("https://app.example.test/"), None);
    assert_eq!(
        reason("https://app.example.test/static/app.js"),
        Some(HiddenReason::Script)
    );
    assert_eq!(
        reason("https://cdn.example.test/logo.png"),
        Some(HiddenReason::Image)
    );
    assert_eq!(
        reason("https://app.example.test/static/site.css"),
        Some(HiddenReason::Style)
    );
    assert_eq!(
        reason("https://fonts.example.test/a.woff2"),
        Some(HiddenReason::Font)
    );
    assert_eq!(
        reason("https://api.example.test/v1/orders"),
        Some(HiddenReason::Preflight)
    );
    assert_eq!(
        reason("https://app.example.test/static/app.js.map"),
        Some(HiddenReason::SourceMap)
    );
    assert_eq!(
        reason("data:image/png;base64,AAAA"),
        Some(HiddenReason::DataUrl)
    );
    assert_eq!(
        reason("chrome-extension://abcdef/content.js"),
        Some(HiddenReason::Extension)
    );
    assert_eq!(
        reason("https://media.example.test/v.mp4"),
        Some(HiddenReason::Media)
    );
    // No MIME type: the extension decides.
    assert_eq!(
        reason("https://app.example.test/img/photo.jpg?w=10"),
        Some(HiddenReason::Image)
    );
    assert_eq!(
        reason("wss://api.example.test/live"),
        Some(HiddenReason::WebSocket)
    );
    let shown: Vec<&HarRow> = rows.iter().filter(|r| r.hidden.is_none()).collect();
    assert_eq!(shown.len(), 3, "{shown:?}");
    let first = &rows[0];
    assert_eq!(first.host, "app.example.test");
    assert_eq!(first.status, Some(200));
    assert_eq!(first.mime, "text/html");
    assert_eq!(first.size, Some(10));
}

#[test]
fn preview_collapses_duplicates() {
    let rows = rows(CHROME);
    let orders: Vec<&HarRow> = rows
        .iter()
        .filter(|r| r.url == "https://api.example.test/v1/orders?page=1")
        .collect();
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].duplicates, 3);
    assert_eq!(orders[0].index, 6);
    // Same URL, different method: separate rows.
    assert!(
        rows.iter()
            .any(|r| r.method == "POST" && r.url == "https://api.example.test/v1/orders")
    );
}

#[test]
fn bearer_becomes_auth_bearer_with_secret() {
    let plan = plan_all(SECRETS, &no_secrets());
    let orders = find(&plan, "/v1/orders?page=1");
    assert_eq!(
        orders.auth,
        Auth::Bearer {
            token: "{{authToken}}".into()
        }
    );
    assert_eq!(header(orders, "authorization"), None);
    assert_eq!(plan.new_secrets["authToken"], "tok-AAAA1111");
}

#[test]
fn basic_becomes_auth_basic_with_two_secrets() {
    let plan = plan_all(SECRETS, &no_secrets());
    let token = find(&plan, "/v1/token");
    assert_eq!(
        token.auth,
        Auth::Basic {
            username: "{{authUser}}".into(),
            password: "{{authPassword}}".into()
        }
    );
    assert_eq!(plan.new_secrets["authUser"], "app-user");
    assert_eq!(plan.new_secrets["authPassword"], "basic-SECRET-3");
}

#[test]
fn other_scheme_becomes_header_var() {
    let plan = plan_all(SECRETS, &no_secrets());
    let digest = find(&plan, "/v1/digest");
    assert_eq!(header(digest, "authorization"), Some("{{authorization}}"));
    assert_eq!(digest.auth, Auth::None);
    let me = find(&plan, "/v1/me");
    assert_eq!(header(me, "cookie"), Some("{{cookie}}"));
    assert_eq!(
        plan.new_secrets["cookie"],
        "sid=cookie-SECRET-4; theme=dark"
    );
}

#[test]
fn listed_headers_and_query_params_become_secrets() {
    let plan = plan_all(SECRETS, &no_secrets());
    let me = find(&plan, "/v1/me");
    assert_eq!(header(me, "x-api-key"), Some("{{xApiKey}}"));
    assert_eq!(plan.new_secrets["xApiKey"], "key-SECRET-5");
    let orders = find(&plan, "/v1/orders?page=1");
    assert_eq!(header(orders, "x-csrf-token"), Some("{{xCsrfToken}}"));
    assert_eq!(
        orders.url,
        "https://api.example.test/v1/orders?page=1&access_token={{accessToken}}"
    );
    assert_eq!(
        orders.query,
        vec![
            KeyValue::new("page", "1"),
            KeyValue::new("access_token", "{{accessToken}}")
        ]
    );
    assert_eq!(plan.new_secrets["accessToken"], "tok-QUERY-0001");
    // A JWT in an unlisted header is moved too.
    let session = find(&plan, "/v1/session");
    assert_eq!(header(session, "x-session"), Some("{{xSession}}"));
}

#[test]
fn same_value_shares_variable_different_value_numbers() {
    let plan = plan_all(SECRETS, &no_secrets());
    let page2 = find(&plan, "/v1/orders?page=2");
    assert_eq!(
        page2.auth,
        Auth::Bearer {
            token: "{{authToken}}".into()
        }
    );
    let me = find(&plan, "/v1/me");
    assert_eq!(
        me.auth,
        Auth::Bearer {
            token: "{{authToken2}}".into()
        }
    );
    assert_eq!(plan.new_secrets["authToken2"], "tok-BBBB2222");
}

#[test]
fn existing_secret_reused_plain_name_skipped() {
    let reuse = ExistingSecrets {
        values: BTreeMap::from([("authToken".to_owned(), "tok-AAAA1111".to_owned())]),
        plain_names: BTreeSet::new(),
    };
    let plan = plan_all(SECRETS, &reuse);
    assert!(!plan.new_secrets.contains_key("authToken"));
    assert_eq!(plan.new_secrets["authToken2"], "tok-BBBB2222");
    let orders = find(&plan, "/v1/orders?page=1");
    assert_eq!(
        orders.auth,
        Auth::Bearer {
            token: "{{authToken}}".into()
        }
    );

    let plain = ExistingSecrets {
        values: BTreeMap::new(),
        plain_names: BTreeSet::from(["authToken".to_owned()]),
    };
    let plan = plan_all(SECRETS, &plain);
    assert!(!plan.new_secrets.contains_key("authToken"));
    assert_eq!(plan.new_secrets["authToken2"], "tok-AAAA1111");
    assert_eq!(plan.new_secrets["authToken3"], "tok-BBBB2222");

    let different = ExistingSecrets {
        values: BTreeMap::from([("authToken".to_owned(), "other".to_owned())]),
        plain_names: BTreeSet::new(),
    };
    let plan = plan_all(SECRETS, &different);
    assert_eq!(plan.new_secrets["authToken2"], "tok-AAAA1111");
}

#[test]
fn body_credentials_become_secrets() {
    let plan = plan_all(SECRETS, &no_secrets());
    let login = find(&plan, "/v1/login");
    assert_eq!(
        login.body,
        Body::Json(
            "{\"username\":\"alice\",\"password\":\"{{password}}\",\"auth\":{\"client_secret\":\"{{clientSecret}}\",\"n\":5},\"token\":7,\"newPassword\":\"keep-me\"}"
                .into()
        )
    );
    let Body::Json(text) = &login.body else {
        unreachable!()
    };
    serde_json::from_str::<serde_json::Value>(text).unwrap();
    assert_eq!(plan.new_secrets["password"], "pw-SECRET-1");
    assert_eq!(plan.new_secrets["clientSecret"], "cs-SECRET-2");

    let token = find(&plan, "/v1/token");
    assert_eq!(
        token.body,
        Body::FormUrlEncoded(vec![
            KeyValue::new("grant_type", "client_credentials"),
            KeyValue::new("client_secret", "{{clientSecret2}}"),
            KeyValue::new("scope", "a b"),
        ])
    );

    let upload = find(&plan, "/v1/upload");
    let Body::Multipart(fields) = &upload.body else {
        panic!("{:?}", upload.body)
    };
    assert_eq!(fields[0].value, MultipartValue::Text("report".into()));
    assert_eq!(
        fields[1].value,
        MultipartValue::Text("{{password2}}".into())
    );
    assert_eq!(fields[2].value, MultipartValue::Files(vec![String::new()]));
    assert!(
        plan.notices.iter().any(|n| n.contains("report.pdf")),
        "{:?}",
        plan.notices
    );
}

#[test]
fn xml_body_with_jwt_flagged_and_rewritten() {
    let rows = rows(SECRETS);
    let soap = rows.iter().find(|r| r.url.ends_with("/v1/soap")).unwrap();
    assert!(soap.has_secrets);
    let plan = plan_all(SECRETS, &no_secrets());
    let req = find(&plan, "/v1/soap");
    let Body::Xml(text) = &req.body else {
        panic!("{:?}", req.body)
    };
    assert!(!text.contains("eyJhbGciOiJIUzI1NiJ9"), "{text}");
    assert!(text.starts_with("<Envelope><Token>{{token"), "{text}");
    assert!(
        rows.iter()
            .filter(|r| !r.url.ends_with("/v1/soap"))
            .all(|r| r.has_secrets),
        "every other fixture entry carries a credential"
    );
    assert!(!preview(&parse_har(CHROME).unwrap())[0].has_secrets);
}

fn body_text(req: &HttpRequest) -> &str {
    match &req.body {
        Body::Xml(t) | Body::Text(t) | Body::Json(t) => t,
        other => panic!("{other:?}"),
    }
}

#[test]
fn xml_body_credentials_become_secrets() {
    assert!(rows(SOAP).iter().all(|r| r.has_secrets));
    let plan = plan_all(SOAP, &no_secrets());
    let text = body_text(find(&plan, "/v1/soap-login"));
    assert!(
        text.contains(r#"<wsse:Password Type="text">{{password}}</wsse:Password>"#),
        "{text}"
    );
    assert!(text.contains("<wsse:Username>u</wsse:Username>"), "{text}");
    assert_eq!(plan.new_secrets["password"], "pw-AAAA");
}

#[test]
fn xml_two_passwords_numbered() {
    let plan = plan_all(SOAP, &no_secrets());
    let text = body_text(find(&plan, "/v1/soap-two"));
    assert_eq!(
        text,
        "<Envelope><wsse:Password>{{password}}</wsse:Password><wsse:Password>{{password2}}</wsse:Password><wsse:Password>{{password}}</wsse:Password></Envelope>"
    );
    assert_eq!(plan.new_secrets["password2"], "pw-BBBB");
}

#[test]
fn text_body_jwt_becomes_body_token() {
    let plan = plan_all(SOAP, &no_secrets());
    assert_eq!(body_text(find(&plan, "/v1/text")), "id={{bodyToken}}");
    assert_eq!(
        plan.new_secrets["bodyToken"],
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig"
    );
}

#[test]
fn json_body_jwt_in_plain_key() {
    let plan = plan_all(SOAP, &no_secrets());
    let text = body_text(find(&plan, "/v1/note"));
    assert_eq!(text, r#"{"note":"{{bodyToken}}","n":1}"#);
    serde_json::from_str::<serde_json::Value>(text).unwrap();
}

#[test]
fn imported_xml_sends_identical_bytes() {
    let har = parse_har(SOAP).unwrap();
    let plan = plan_all(SOAP, &no_secrets());
    for (entry, (_, req)) in har.log.entries.iter().zip(&plan.requests) {
        let mut filled = body_text(req).to_owned();
        for (name, value) in &plan.new_secrets {
            filled = filled.replace(&format!("{{{{{name}}}}}"), value);
        }
        let captured = entry
            .request
            .post_data
            .as_ref()
            .unwrap()
            .text
            .as_deref()
            .unwrap();
        assert_eq!(filled, captured, "{}", entry.request.url);
    }
}

#[test]
fn pseudo_and_executor_headers_dropped() {
    let plan = plan_all(CHROME, &no_secrets());
    for (_, req) in &plan.requests {
        for h in &req.headers {
            let n = h.name.to_ascii_lowercase();
            assert!(
                !n.starts_with(':')
                    && !["host", "content-length", "connection", "accept-encoding"]
                        .contains(&n.as_str()),
                "{n} kept"
            );
        }
    }
    assert_eq!(
        header(find(&plan, "app.example.test/"), "accept"),
        Some("*/*")
    );
}

#[test]
fn no_secret_value_in_any_written_bru() {
    let plan = plan_all(SECRETS, &no_secrets());
    let soap = plan_all(SOAP, &no_secrets());
    let all: Vec<&(String, HttpRequest)> = plan.requests.iter().chain(&soap.requests).collect();
    assert_eq!(all.len(), 14);
    for (name, req) in all {
        let text = RequestFile {
            doc: BruDocument::new(),
            request: req.clone(),
        }
        .to_bru();
        RequestFile::parse(&text).unwrap();
        for secret in SECRET_VALUES.iter().chain(SOAP_VALUES) {
            assert!(!text.contains(secret), "{name} contains {secret}:\n{text}");
        }
    }
    // `newPassword` is not a credential field and stays.
    assert!(
        plan.requests
            .iter()
            .any(|(_, r)| matches!(&r.body, Body::Json(t) if t.contains("keep-me")))
    );
}

#[test]
fn no_credentials_gives_auth_inherit() {
    let plan = plan_all(CHROME, &no_secrets());
    assert!(plan.requests.iter().all(|(_, r)| r.auth == Auth::Inherit));
    assert!(plan.new_secrets.is_empty());
}

#[test]
fn seq_starts_after_existing() {
    let har = parse_har(SECRETS).unwrap();
    let plan = plan_import(&har, &[3, 1, 1], &BTreeSet::new(), 7.0, &no_secrets());
    let seqs: Vec<f64> = plan.requests.iter().map(|(_, r)| r.seq).collect();
    assert_eq!(seqs, vec![8.0, 9.0]);
    // Capture order, each entry once.
    assert!(plan.requests[0].1.url.contains("page=1"));
    assert!(plan.requests[1].1.url.ends_with("/v1/me"));
}

#[test]
fn minimal_har_imports() {
    let har = parse_har(FIREFOX).unwrap();
    let rows = preview(&har);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].status, None);
    assert!(rows.iter().all(|r| r.hidden.is_none()));
    let plan = plan_import(&har, &[0, 1], &BTreeSet::new(), 0.0, &no_secrets());
    assert_eq!(plan.requests.len(), 2);
    assert_eq!(plan.requests[1].1.method, Method::Post);
    assert_eq!(plan.requests[1].1.body, Body::None);
}

#[test]
fn invalid_har_is_error() {
    assert!(parse_har("not json").is_err());
    assert!(parse_har("{\"log\":{}}").is_err());
    assert!(parse_har("[]").is_err());
}

/// Bruno's `validateName` (`@usebruno/common/utils/naming.ts`).
fn bruno_valid(name: &str) -> bool {
    let invalid = |c: char| "<>:\"/\\|?*".contains(c) || c < ' ';
    let upper = name.to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    let first = name.chars().next();
    let last = name.chars().last();
    !name.is_empty()
        && name.chars().count() <= 255
        && !reserved
        && first.is_some_and(|c| !c.is_whitespace() && c != '-' && !invalid(c))
        && !name.chars().any(invalid)
        && last.is_some_and(|c| c != '.' && !c.is_whitespace() && !invalid(c))
}

#[test]
fn import_names_are_valid_and_unique() {
    let long = "a".repeat(300);
    let entries: Vec<String> = [
        format!("https://example.test/{long}"),
        "https://example.test/".to_owned(),
        "https://example.test/CON".to_owned(),
        "https://example.test/a?:*|b".to_owned(),
        "https://example.test/x".to_owned(),
        "https://example.test/x".to_owned(),
        "https://example.test/x/".to_owned(),
        "https://example.test/%2e%2e".to_owned(),
    ]
    .iter()
    .map(|url| {
        format!(
            "{{\"request\":{{\"method\":\"GET\",\"url\":\"{url}\",\"headers\":[]}},\"response\":{{\"status\":200,\"content\":{{}}}}}}"
        )
    })
    .collect();
    let text = format!("{{\"log\":{{\"entries\":[{}]}}}}", entries.join(","));
    let har = parse_har(&text).unwrap();
    let existing = BTreeSet::from(["get x.bru".to_owned()]);
    let all: Vec<usize> = (0..entries.len()).collect();
    let plan = plan_import(&har, &all, &existing, 0.0, &no_secrets());
    let names: Vec<&String> = plan.requests.iter().map(|(n, _)| n).collect();
    let mut stems = BTreeSet::new();
    for name in &names {
        let stem = sanitize_name(name);
        assert!(
            bruno_valid(&stem),
            "invalid file name {stem:?} for {name:?}"
        );
        assert!(stems.insert(stem.to_lowercase()), "duplicate {stem}");
        assert!(!existing.contains(&format!("{}.bru", stem.to_lowercase())));
    }
    assert_eq!(names[1], "GET example.test");
    assert!(names.iter().any(|n| n.starts_with("GET x ")), "{names:?}");
}
